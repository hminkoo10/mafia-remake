use anyhow::{Context as AnyhowContext, Result};
use chrono::{SecondsFormat, Utc};
use dashmap::DashMap;
use mafia_remake::game::MafiaGame;
use mafia_remake::model::{Player, Role, Winner};
use mafia_remake::{config, stats};
use poise::serenity_prelude as serenity;
use serde_json::{Value, json};
pub(crate) mod web_settings;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::AtomicU64;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use tokio::sync::{Notify, RwLock};

const MAX_GAME_PLAYERS: usize = 24;
const DAY_EXTENSION_VOTE_SECONDS: u64 = 10;
const DISCUSSION_EXTENSION_SECONDS: u64 = 60;
const CONFIRM_VOTE_SECONDS: u64 = 15;
const COMPLETED_REPLAY_LIMIT: usize = 100;
const GAME_NOTIFICATION_ROLE: &str = "게임알림";
const SPECTATOR_ROLE: &str = "관전자";
const DEAD_PLAYER_ROLE: &str = "사망자";
const SHAMAN_CHAT_CHANNEL_NAME: &str = "영매-채팅방";

const PRIVATE_CHAT_ROLES: &[Role] = &[
    Role::Mafia,
    Role::Police,
    Role::Agent,
    Role::Vigilante,
    Role::Doctor,
    Role::CultLeader,
    Role::Lover,
];

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

/// 진행 중인 판의 배팅 잠금: 판 키 → 유저별 배팅액. 코인은 서버와 상관없이 하나라서
/// 모든 판의 잠금을 합쳐 본다. 판 시작 때 배팅을 확정하는 통계 쓰기 잠금 안에서 넣고,
/// 정산은 같은 통계 쓰기 잠금 안에서 이 판의 잠금을 직접 풀었을 때만 한다. 코인 선물과
/// 새 판의 배팅 확정도 통계 쓰기 잠금 안에서 읽으므로, 보유 코인은 항상 잠긴 배팅액 합
/// 이상으로 남는다. 정산 없이 끝난 판(중지·정리·오류)은 게임을 목록에서 뺄 때 푼다. 중지가
/// 먼저 풀었으면 이미 결과 발표 중이던 판도 배팅을 정산하지 않는다(중지된 판).
type BetLocks = Arc<std::sync::Mutex<HashMap<String, HashMap<u64, i64>>>>;

fn lock_game_bets(locks: &BetLocks, game_key: &str, bets: &HashMap<u64, i64>) {
    if bets.is_empty() {
        return;
    }
    locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(game_key.to_string(), bets.clone());
}

/// 이 판의 배팅 잠금을 푼다. 이 호출이 이 판의 잠금을 실제로 풀었으면 true.
fn release_game_bets(locks: &BetLocks, game_key: &str) -> bool {
    locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(game_key)
        .is_some()
}

/// 진행 중인 모든 판에서 아직 정산되지 않은 이 유저의 배팅액 합.
fn locked_bet(locks: &BetLocks, user_id: u64) -> i64 {
    let locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks
        .values()
        .filter_map(|bets| bets.get(&user_id))
        .fold(0_i64, |sum, bet| sum.saturating_add((*bet).max(0)))
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
enum AnonymousNameMode {
    #[name = "동물"]
    Animal,
    #[name = "숫자"]
    Number,
}

impl AnonymousNameMode {
    const fn value(self) -> &'static str {
        match self {
            Self::Animal => "animal",
            Self::Number => "number",
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
enum LeaderboardMetric {
    #[name = "승리수"]
    Wins,
    #[name = "승률"]
    Winrate,
    #[name = "연승"]
    Streak,
    #[name = "판수"]
    Games,
    #[name = "마피아팀 횟수"]
    Mafia,
    #[name = "게임시간"]
    Playtime,
    #[name = "레이팅"]
    Rating,
    #[name = "코인"]
    Coins,
    #[name = "스타플레이어"]
    Star,
}

impl LeaderboardMetric {
    const fn value(self) -> &'static str {
        match self {
            Self::Wins => "wins",
            Self::Winrate => "winrate",
            Self::Streak => "streak",
            Self::Games => "games",
            Self::Mafia => "mafia",
            Self::Playtime => "playtime",
            Self::Rating => "rating",
            Self::Coins => "coins",
            Self::Star => "star",
        }
    }
}

#[derive(Clone)]
struct Data {
    config: Arc<RwLock<config::BotConfig>>,
    config_path: Arc<PathBuf>,
    stats: Arc<RwLock<stats::StatsFile>>,
    stats_path: Arc<PathBuf>,
    games: Arc<DashMap<serenity::GuildId, Arc<RwLock<RunningGame>>>>,
    /// 진행 중인 판의 배팅 잠금 (코인 선물이 배팅액을 빼돌리지 못하게).
    bet_locks: BetLocks,
    completed_replays: Arc<RwLock<VecDeque<Value>>>,
    completed_replays_path: Arc<PathBuf>,
    recruitments: Arc<DashMap<serenity::GuildId, Arc<RwLock<Recruitment>>>>,
    recruitment_update_versions: Arc<DashMap<serenity::GuildId, Arc<AtomicU64>>>,
    web_sessions: Arc<DashMap<String, web_settings::WebSettingsSession>>,
    web_base_url: Arc<String>,
    bot_user_id: serenity::UserId,
    casino: casino_hub::SharedHub,
    casino_base_url: Arc<String>,
    /// 주식 시장.
    stocks: stock_hub::SharedStocks,
    /// 서버 활동 누적 (게임 연동 종목의 실적 재료).
    activity: Arc<stock_hub::ServerActivity>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ChannelRoleIds {
    pub(crate) everyone: serenity::RoleId,
    pub(crate) participant: Option<serenity::RoleId>,
    pub(crate) spectator: Option<serenity::RoleId>,
    pub(crate) manager: Option<serenity::RoleId>,
    pub(crate) dead: Option<serenity::RoleId>,
    pub(crate) bot: serenity::UserId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PersonalChannelKind {
    Memo,
    Dead,
    Shaman,
    Role(Role),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ContractorContractDraft {
    pub(crate) target_ids: [Option<u64>; 2],
    pub(crate) guessed_roles: [Option<Role>; 2],
}

#[derive(Debug)]
struct RunningGame {
    guild_id: serenity::GuildId,
    channel_id: serenity::ChannelId,
    participant_user_ids: HashSet<u64>,
    spectator_user_ids: HashSet<u64>,
    game: MafiaGame,
    reveal_death_roles: bool,
    anonymous_enabled: bool,
    started_at: Instant,
    started_at_iso: String,
    ended_at_iso: Option<String>,
    activity_game_key: String,
    phase_deadline: Option<Instant>,
    initial_roles: HashMap<u64, Role>,
    memos: HashMap<u64, HashMap<u64, Vec<String>>>,
    game_status_message_id: Option<serenity::MessageId>,
    game_status_text: Option<String>,
    anonymous_aliases: HashMap<u64, String>,
    anonymous_original_names: HashMap<u64, String>,
    anonymous_input_channel_ids: HashMap<u64, serenity::ChannelId>,
    anonymous_input_channel_owners: HashMap<serenity::ChannelId, u64>,
    anonymous_dead_input_channel_ids: HashMap<u64, serenity::ChannelId>,
    anonymous_dead_input_channel_owners: HashMap<serenity::ChannelId, u64>,
    dead_chat_unlocked_ids: HashSet<u64>,
    pending_dead_chat_user_ids: HashSet<u64>,
    dead_role_chat_visible_from_days: HashMap<u64, u32>,
    anonymous_shaman_input_channel_ids: HashMap<u64, serenity::ChannelId>,
    anonymous_shaman_input_channel_owners: HashMap<serenity::ChannelId, u64>,
    anonymous_role_input_channel_ids: HashMap<(u64, Role), serenity::ChannelId>,
    anonymous_role_input_channels: HashMap<serenity::ChannelId, (u64, Role)>,
    anonymous_role_input_status_message_ids: HashMap<(u64, Role), serenity::MessageId>,
    anonymous_role_status_texts: HashMap<(u64, Role), String>,
    anonymous_webhooks: HashMap<serenity::ChannelId, serenity::Webhook>,
    anonymous_webhook_creation_locks: HashMap<serenity::ChannelId, Arc<tokio::sync::Mutex<()>>>,
    channel_role_ids: Option<ChannelRoleIds>,
    source_category_id: Option<Option<serenity::ChannelId>>,
    permission_overwrite_cache: HashMap<(u64, u64, bool), serenity::PermissionOverwrite>,
    verified_member_ids: HashSet<u64>,
    personal_channel_creation_locks:
        HashMap<(u64, PersonalChannelKind), Arc<tokio::sync::Mutex<()>>>,
    original_game_channel_overwrites:
        HashMap<serenity::RoleId, Option<serenity::PermissionOverwrite>>,
    game_channel_overwrites: HashMap<serenity::RoleId, Option<serenity::PermissionOverwrite>>,
    member_channel_overwrites: HashMap<u64, Option<serenity::PermissionOverwrite>>,
    original_slowmode_delays: HashMap<serenity::ChannelId, u16>,
    channel_slowmode_cache: HashMap<serenity::ChannelId, u16>,
    private_channel_ids: HashMap<Role, serenity::ChannelId>,
    private_role_status_message_ids: HashMap<Role, serenity::MessageId>,
    private_role_status_texts: HashMap<Role, String>,
    memo_channel_ids: HashMap<u64, serenity::ChannelId>,
    shaman_channel_id: Option<serenity::ChannelId>,
    shaman_status_message_id: Option<serenity::MessageId>,
    shaman_status_text: Option<String>,
    frog_game_channel_overwrites: HashMap<u64, Option<serenity::PermissionOverwrite>>,
    madam_seduction_channel_overwrites: HashMap<u64, Option<serenity::PermissionOverwrite>>,
    day_chat_open: bool,
    final_defense_user_id: Option<u64>,
    day_skip_voter_ids: HashSet<u64>,
    day_skip_confirmed: bool,
    day_extension_voter_ids: HashSet<u64>,
    day_extension_active: bool,
    day_extension_confirmed: bool,
    night_timed_events_due: bool,
    contractor_contract_drafts: HashMap<u64, ContractorContractDraft>,
    /// Activity 프론트엔드에 표시할 밤 행동 결과 (user_id → 결과 텍스트)
    activity_night_results: HashMap<u64, String>,
    replay_events: Vec<Value>,
    next_replay_sequence: u64,
    night_notify: Arc<Notify>,
    vote_notify: Arc<Notify>,
    confirm_notify: Arc<Notify>,
    day_notify: Arc<Notify>,
    /// 최후변론 대상자가 `발언 종료`를 누르면 깨어나 바로 찬반 투표로 넘어간다.
    final_defense_notify: Arc<Notify>,
    final_defense_ended: bool,
    /// 판 시작 시 확정한 배팅액 (user_id → 원). 보유 코인 안으로 잘라 둔다.
    bets: HashMap<u64, i64>,
    /// 스타플레이어 투표 (투표자 → 후보). 게임 종료 직후 10초 동안만 받는다.
    star_votes: HashMap<u64, u64>,
    star_vote_open: bool,
    star_vote_notify: Arc<Notify>,
    stats_recorded: bool,
}

impl RunningGame {
    fn elapsed_ms(&self) -> u64 {
        self.started_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }

    fn role_team_key(role: Role) -> &'static str {
        if role == Role::Joker {
            "joker"
        } else if role == Role::CultLeader || role == Role::Fanatic {
            "cult"
        } else if role.is_mafia_team() {
            "mafia"
        } else {
            "citizen"
        }
    }

    fn player_team_key(&self, player: &Player) -> &'static str {
        if player.role == Role::Joker {
            "joker"
        } else if self.game.is_cult_team(player) || player.role == Role::Fanatic {
            "cult"
        } else if self.game.is_mafia_team(player) {
            "mafia"
        } else {
            "citizen"
        }
    }

    fn replay_player_value(&self, user_id: u64) -> Option<Value> {
        self.game.get_player(user_id).map(|player| {
            json!({
                "user_id": player.user_id,
                "name": player.name.clone(),
                "role": player.role.value(),
                "role_key": format!("{:?}", player.role),
                "team": self.player_team_key(player),
                "alive": player.alive,
            })
        })
    }

    fn replay_target_values(&self, target_ids: &[u64]) -> Vec<Value> {
        target_ids
            .iter()
            .filter_map(|user_id| self.replay_player_value(*user_id))
            .collect()
    }

    fn record_replay_event(
        &mut self,
        kind: impl Into<String>,
        actor_id: Option<u64>,
        target_ids: &[u64],
        details: Value,
    ) {
        let seq = self.next_replay_sequence;
        self.next_replay_sequence = self.next_replay_sequence.saturating_add(1);
        self.replay_events.push(json!({
            "seq": seq,
            "id": format!("e_{seq:06}"),
            "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            "elapsed_ms": self.elapsed_ms(),
            "day_number": self.game.day_number,
            "phase": self.game.phase.value(),
            "phase_key": format!("{:?}", self.game.phase),
            "kind": kind.into(),
            "actor": actor_id.and_then(|user_id| self.replay_player_value(user_id)),
            "target_user_ids": target_ids.to_vec(),
            "targets": self.replay_target_values(target_ids),
            "details": details,
        }));
    }

    fn replay_vote_counts(&self, counts: &HashMap<Option<u64>, i32>) -> Vec<Value> {
        let mut values = counts
            .iter()
            .map(|(target_id, count)| {
                json!({
                    "target_user_id": *target_id,
                    "target": target_id.and_then(|user_id| self.replay_player_value(user_id)),
                    "choice": if target_id.is_some() { "player" } else { "skip" },
                    "count": *count,
                })
            })
            .collect::<Vec<_>>();
        values.sort_by_key(|value| {
            (
                value["choice"].as_str().unwrap_or_default().to_string(),
                value["target_user_id"].as_u64().unwrap_or_default(),
            )
        });
        values
    }

    fn replay_confirm_vote_counts(&self, counts: &HashMap<bool, i32>) -> Vec<Value> {
        let mut values = counts
            .iter()
            .map(|(approved, count)| json!({"approve": *approved, "count": *count}))
            .collect::<Vec<_>>();
        values.sort_by_key(|value| value["approve"].as_bool().unwrap_or(false));
        values
    }

    fn replay_text_results(&self, results: &HashMap<u64, String>) -> Vec<Value> {
        let mut values = results
            .iter()
            .map(|(user_id, text)| {
                json!({
                    "user_id": user_id,
                    "player": self.replay_player_value(*user_id),
                    "text": text.clone(),
                })
            })
            .collect::<Vec<_>>();
        values.sort_by_key(|value| value["user_id"].as_u64().unwrap_or_default());
        values
    }

    fn replay_participants(&self) -> Vec<Value> {
        let mut players = self
            .game
            .players
            .iter()
            .map(|player| {
                let initial_role = self
                    .initial_roles
                    .get(&player.user_id)
                    .copied()
                    .unwrap_or(player.role);
                let death_order = self
                    .game
                    .death_order
                    .iter()
                    .position(|user_id| *user_id == player.user_id)
                    .map(|index| index + 1);
                json!({
                    "user_id": player.user_id,
                    "name": player.name.clone(),
                    "initial_role": initial_role.value(),
                    "initial_role_key": format!("{:?}", initial_role),
                    "initial_team": Self::role_team_key(initial_role),
                    "final_role": player.role.value(),
                    "final_role_key": format!("{:?}", player.role),
                    "final_team": self.player_team_key(player),
                    "alive": player.alive,
                    "death_order": death_order,
                })
            })
            .collect::<Vec<_>>();
        players.sort_by_key(|player| player["name"].as_str().unwrap_or_default().to_lowercase());
        players
    }

    fn replay_rating_log(rating_log: &[stats::GameRatingLogItem]) -> Vec<Value> {
        rating_log
            .iter()
            .map(|item| {
                json!({
                    "user_id": item.user_id,
                    "name": item.name.clone(),
                    "role": item.role.clone(),
                    "before": item.before,
                    "after": item.after,
                    "delta": item.delta,
                    "team_delta": item.team_delta,
                    "role_delta": item.role_delta,
                    "streak_delta": item.streak_delta,
                    "win_streak": item.win_streak,
                    "best_win_streak": item.best_win_streak,
                    "reasons": item.reasons.clone(),
                })
            })
            .collect()
    }

    fn replay_snapshot(
        &self,
        status: &str,
        winner: Option<Winner>,
        rating_log: &[stats::GameRatingLogItem],
    ) -> Value {
        json!({
            "game_key": self.activity_game_key.clone(),
            "game_id": self.activity_game_key.clone(),
            "guild_id": self.guild_id.get(),
            "channel_id": self.channel_id.get(),
            "status": status,
            "started_at": self.started_at_iso.clone(),
            "ended_at": self.ended_at_iso.clone(),
            "phase": self.game.phase.value(),
            "phase_key": format!("{:?}", self.game.phase),
            "day_number": self.game.day_number,
            "elapsed_seconds": self.started_at.elapsed().as_secs(),
            "winner": winner.map(|winner| winner.value()),
            "winner_key": winner.map(|winner| format!("{:?}", winner)),
            "participants": self.replay_participants(),
            "events": self.replay_events.clone(),
            "rating_log": Self::replay_rating_log(rating_log),
        })
    }

    fn replay_summary(&self, status: &str, winner: Option<Winner>) -> Value {
        json!({
            "game_key": self.activity_game_key.clone(),
            "game_id": self.activity_game_key.clone(),
            "guild_id": self.guild_id.get(),
            "channel_id": self.channel_id.get(),
            "status": status,
            "started_at": self.started_at_iso.clone(),
            "ended_at": self.ended_at_iso.clone(),
            "phase": self.game.phase.value(),
            "phase_key": format!("{:?}", self.game.phase),
            "day_number": self.game.day_number,
            "elapsed_seconds": self.started_at.elapsed().as_secs(),
            "winner": winner.map(|winner| winner.value()),
            "winner_key": winner.map(|winner| format!("{:?}", winner)),
            "participant_count": self.game.players.len(),
            "event_count": self.replay_events.len(),
        })
    }
}

#[derive(Debug, Clone)]
struct Recruitment {
    host_user_id: serenity::UserId,
    participant_role_id: serenity::RoleId,
    spectator_role_id: Option<serenity::RoleId>,
    role_counts: HashMap<Role, usize>,
    special_roles: Vec<Role>,
    max_players: usize,
    minimum_players: usize,
    joined_ids: HashSet<u64>,
    joined_names: HashMap<u64, String>,
    spectator_ids: HashSet<u64>,
    spectator_names: HashMap<u64, String>,
    /// 이번 모집에서 봇이 관전자 역할을 준 뒤 회수를 확인하지 못한 유저. 인터랙션의 멤버
    /// 정보는 클릭 시점 스냅샷이라 방금 준 역할이 안 보일 수 있으므로, 참가할 때 이 목록도 본다.
    spectator_role_granted: HashSet<u64>,
    accepting: bool,
    cancelled: bool,
    /// 주최자가 자동시작 버튼으로 정한 인원. 참가자가 이 수에 도달하면 즉시 시작한다.
    auto_start_players: Option<usize>,
    recruitment_seconds: u64,
    done: Arc<Notify>,
}

mod activity;
mod casino_hub;
mod casino_web;
mod channel;
mod commands;
mod embed;
mod http_pool;
mod runner;
mod stock_hub;
mod stock_web;

async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::InteractionCreate {
            interaction: serenity::Interaction::Component(component),
        } => {
            if let Err(error) = commands::handle_component(ctx, data, component).await {
                eprintln!("component error: {error:?}");
            }
        }
        serenity::FullEvent::InteractionCreate {
            interaction: serenity::Interaction::Modal(modal),
        } => {
            if let Err(error) = commands::handle_modal(ctx, data, modal).await {
                eprintln!("modal error: {error:?}");
            }
        }
        serenity::FullEvent::Message { new_message } => {
            if let Err(error) = commands::handle_message_event(ctx, data, new_message).await {
                eprintln!("message event error: {error:?}");
            }
        }
        _ => {}
    }
    Ok(())
}

fn preserve_entry_point_command(command: &serenity::Command) -> serenity::CreateCommand {
    let mut builder = serenity::CreateCommand::new(command.name.clone())
        .description(command.description.clone())
        .kind(serenity::CommandType::PrimaryEntryPoint)
        .handler(
            command
                .handler
                .unwrap_or(serenity::EntryPointHandlerType::DiscordLaunchActivity),
        )
        .nsfw(command.nsfw);

    if let Some(localizations) = &command.name_localizations {
        for (locale, name) in localizations {
            builder = builder.name_localized(locale, name);
        }
    }
    if let Some(localizations) = &command.description_localizations {
        for (locale, description) in localizations {
            builder = builder.description_localized(locale, description);
        }
    }
    if let Some(permissions) = command.default_member_permissions {
        builder = builder.default_member_permissions(permissions);
    }
    if !command.integration_types.is_empty() {
        builder = builder.integration_types(command.integration_types.clone());
    }
    if let Some(contexts) = &command.contexts {
        builder = builder.contexts(contexts.clone());
    }

    builder
}

async fn register_global_commands_preserving_activity(
    ctx: &serenity::Context,
    commands: &[poise::Command<Data, Error>],
) -> serenity::Result<usize> {
    let mut builders = poise::builtins::create_application_commands(commands);
    let slash_count = builders.len();

    for command in serenity::Command::get_global_commands(ctx).await? {
        if command.kind == serenity::CommandType::PrimaryEntryPoint {
            builders.push(preserve_entry_point_command(&command));
        }
    }

    serenity::Command::set_global_commands(ctx, builders).await?;
    Ok(slash_count)
}

fn warn_cloudflare_https_port(base_url: Option<&str>) {
    let Some(base_url) = base_url.map(str::trim) else {
        return;
    };
    if !base_url.starts_with("https://") {
        return;
    }
    let Some(port) = explicit_url_port(base_url) else {
        return;
    };
    if matches!(port, 80 | 8080 | 8880 | 2052 | 2082 | 2086 | 2095) {
        eprintln!(
            "WEB_SETTINGS_BASE_URL uses https on port {port}; Cloudflare proxied HTTPS supports only 443, 2053, 2083, 2087, 2096, or 8443. Use 8443 for web settings when Activity uses 2053."
        );
    }
}

/// 설정 웹이 평문 http로 노출되거나, 공개 주소의 포트가 실제 포트와 다르면 알려 준다.
/// Cloudflare 프록시 뒤에서 https로 쓰려면 8443/2083/2087/2096 중 하나여야 한다 (8880은 http 전용).
fn warn_settings_web_exposure(base_url: Option<&str>, web_port: u16) {
    let Some(base_url) = base_url.map(str::trim) else {
        return;
    };
    let tls_available = std::env::var("ACTIVITY_TLS_CERT")
        .is_ok_and(|value| !value.trim().is_empty())
        && std::env::var("ACTIVITY_TLS_KEY").is_ok_and(|value| !value.trim().is_empty());
    if base_url.starts_with("http://") && tls_available {
        eprintln!(
            "WEB_SETTINGS_BASE_URL({base_url})이 평문 http입니다. Cloudflare 프록시 뒤에서 https로 쓰려면 \
             WEB_SETTINGS_PORT를 8443(또는 2083/2087/2096)로 바꾸고 WEB_SETTINGS_BASE_URL을 \
             https://호스트:8443 으로 설정하세요. 인증서는 ACTIVITY_TLS_CERT/KEY를 그대로 씁니다."
        );
    }
    if let Some(port) = explicit_url_port(base_url)
        && port != web_port
    {
        eprintln!(
            "WEB_SETTINGS_BASE_URL의 포트({port})와 WEB_SETTINGS_PORT({web_port})가 다릅니다. \
             프록시나 포트 포워딩이 없다면 링크가 열리지 않습니다."
        );
    }
}

fn state_file_name(path: &Path) -> Result<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .with_context(|| format!("상태 파일 이름이 없습니다: {}", path.display()))
}

/// 같은 폴더에서 아직 쓰이지 않은 "<이름>.corrupt-<unix 초>" 경로. 그 이름이 이미 있으면 뒤에
/// 번호를 붙여, 전에 남겨 둔 파일을 덮어쓰지 않는다.
fn unused_corrupt_path(path: &Path, file_name: &str) -> PathBuf {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let mut target = path.with_file_name(format!("{file_name}.corrupt-{timestamp}"));
    let mut suffix = 1_u32;
    while target.exists() {
        target = path.with_file_name(format!("{file_name}.corrupt-{timestamp}-{suffix}"));
        suffix += 1;
    }
    target
}

/// 읽지 못한 상태 파일을 같은 폴더의 "<이름>.corrupt-<unix 초>"로 옮기고 옮긴 경로를 돌려준다.
fn move_aside_corrupt_file(path: &Path) -> Result<PathBuf> {
    let file_name = state_file_name(path)?;
    let target = unused_corrupt_path(path, &file_name);
    std::fs::rename(path, &target).with_context(|| {
        format!(
            "상태 파일을 옮기지 못했습니다: {} → {}",
            path.display(),
            target.display()
        )
    })?;
    Ok(target)
}

/// 읽지 못한 상태 파일의 복사본을 같은 폴더의 "<이름>.corrupt-<unix 초>"로 남기고 그 경로를
/// 돌려준다. 원본은 건드리지 않는다. 같은 내용의 복사본이 이미 있으면 그 경로를 돌려주고 새로
/// 만들지 않아, 시작 실패로 재시작이 반복돼도 복사본이 쌓이지 않는다.
fn copy_aside_corrupt_file(path: &Path) -> Result<PathBuf> {
    let file_name = state_file_name(path)?;
    let contents = std::fs::read(path)
        .with_context(|| format!("상태 파일을 읽지 못했습니다: {}", path.display()))?;
    let prefix = format!("{file_name}.corrupt-");
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let same_name = entry.file_name().to_string_lossy().starts_with(&prefix);
            let same_size = entry
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.len() == contents.len() as u64);
            if same_name && same_size && std::fs::read(entry.path()).is_ok_and(|c| c == contents) {
                return Ok(entry.path());
            }
        }
    }
    let target = unused_corrupt_path(path, &file_name);
    let copy_error = |error: std::io::Error| {
        anyhow::Error::new(error).context(format!(
            "상태 파일의 복사본을 만들지 못했습니다: {} → {}",
            path.display(),
            target.display()
        ))
    };
    // 같은 초에 다른 프로세스가 먼저 만든 이름이면 create_new가 실패한다. 그 파일은 남의
    // 복사본이므로 지우지 않는다. 지우는 건 여기서 만들었다가 쓰다 실패한 파일뿐이다.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(copy_error)?;
    let written = std::io::Write::write_all(&mut file, &contents).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&target);
        return Err(copy_error(error));
    }
    Ok(target)
}

/// 코인·레이팅(stats.json)이나 카지노 칩(casino.json)처럼 잃으면 안 되는 상태 파일을 불러온다.
/// 파일이 없으면 로더가 빈 상태를 준다. 파일이 있는데 읽거나 파싱하지 못했을 때 빈 상태로
/// 계속하면 다음 저장이 진짜 파일을 빈 상태로 덮어쓴다. 그래서 시작을 멈춘다. 원본은 그대로
/// 둬서, 자동 재시작이 걸려 있어도 누가 파일을 고치거나 지울 때까지 계속 시작을 멈춘다
/// (옮겨 두면 다음 시작이 파일이 없는 줄 알고 빈 상태로 시작한다). 복사본은 따로 남긴다.
fn load_state_file<T>(path: &Path, load: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    load(path).map_err(|error| match copy_aside_corrupt_file(path) {
        Ok(copy) => error.context(format!(
            "{}을(를) 읽지 못해 봇을 시작하지 않습니다. 원본은 그대로 두었고 복사본을 {}에 남겼습니다. \
             파일을 고치거나, 비운 상태로 시작하려면 원본을 지운 뒤 다시 시작하세요.",
            path.display(),
            copy.display()
        )),
        Err(copy_error) => error.context(format!(
            "{}을(를) 읽지 못해 봇을 시작하지 않습니다. 복사본은 만들지 못했습니다 ({copy_error:#}). \
             원본은 그대로 두었으니 파일을 직접 확인하세요.",
            path.display()
        )),
    })
}

/// 리플레이 기록(replays.json)을 불러온다. 코인·칩과 달리 없어도 봇을 돌릴 수 있으므로,
/// 읽지 못하면 옆으로 옮겨 보존하고 크게 알린 뒤 빈 기록으로 계속한다. 옮기지 못하면 다음
/// 저장이 덮어쓰므로 시작을 멈춘다.
fn load_replays_or_move_aside(path: &Path) -> Result<VecDeque<Value>> {
    match web_settings::load_completed_replays(path) {
        Ok(replays) => Ok(replays),
        Err(error) => {
            let moved = move_aside_corrupt_file(path).with_context(|| {
                format!(
                    "{}을(를) 읽지 못했고 옆으로 옮기지도 못해 봇을 시작하지 않습니다: {error:#}",
                    path.display()
                )
            })?;
            eprintln!(
                "!!! 리플레이 기록 {}을(를) 읽지 못해 {}(으)로 옮기고 빈 기록으로 시작합니다. \
                 원인: {error:#}",
                path.display(),
                moved.display()
            );
            Ok(VecDeque::new())
        }
    }
}

fn explicit_url_port(url: &str) -> Option<u16> {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = without_scheme.split('/').next().unwrap_or_default();
    let (_, port) = authority.rsplit_once(':')?;
    port.parse().ok()
}

fn bot_commands() -> Vec<poise::Command<Data, Error>> {
    vec![
        commands::start_game(),
        commands::stop_game(),
        commands::cleanup_stuck_game(),
        commands::disable_mafia_game(),
        commands::enable_mafia_game(),
        commands::add_to_blacklist(),
        commands::remove_from_blacklist(),
        commands::show_blacklist(),
        commands::set_log_channel(),
        commands::configure_game(),
        commands::web_configure_game(),
        commands::configure_player_limit(),
        commands::configure_anonymous_mode(),
        commands::configure_extra_roles(),
        commands::configure_investigation_role(),
        commands::show_manager_status(),
        commands::show_public_status(),
        commands::memo(),
        commands::show_my_info(),
        commands::claim_attendance(),
        commands::set_bet(),
        commands::gift_coins(),
        commands::treasury_info(),
        commands::relief_command(),
        commands::manage_treasury(),
        commands::stock(),
        commands::company(),
        commands::manage_stocks(),
        commands::stock_panel(),
        commands::stock_news_channel(),
        commands::exchange_coupon(),
        commands::manage_coins(),
        commands::issue_coupons(),
        commands::redeem_coupon(),
        commands::list_coupons(),
        commands::create_casino_table(),
        commands::casino_panel(),
        commands::close_casino_table(),
        commands::list_casino_tables(),
        commands::enter_casino(),
        commands::casino_status(),
        commands::leave_casino_table(),
        commands::rating_log(),
        commands::show_rank_cutoffs(),
        commands::show_leaderboard(),
        commands::reset_leaderboard(),
        commands::show_term_info(),
        commands::show_term_descriptions(),
        commands::show_role_info(),
        commands::show_abilities(),
        commands::show_role_descriptions(),
    ]
}

#[cfg(test)]
mod main_tests {
    use super::*;

    #[test]
    fn bet_locks_follow_each_game_until_released() {
        let locks: BetLocks = Arc::new(std::sync::Mutex::new(HashMap::new()));
        lock_game_bets(&locks, "game-a", &HashMap::from([(7, 4_000), (8, 500)]));
        lock_game_bets(&locks, "game-b", &HashMap::from([(7, 1_000)]));
        // 코인은 서버와 상관없이 하나라서 모든 판의 배팅을 더한다.
        assert_eq!(locked_bet(&locks, 7), 5_000);
        assert_eq!(locked_bet(&locks, 8), 500);
        assert_eq!(locked_bet(&locks, 9), 0);
        // 판마다 따로 풀린다. 같은 서버에서 겹쳐 시작한 판도 서로의 잠금을 지우지 않는다.
        assert!(!release_game_bets(&locks, "old-game"));
        assert_eq!(locked_bet(&locks, 7), 5_000);
        assert!(release_game_bets(&locks, "game-a"));
        assert_eq!(locked_bet(&locks, 7), 1_000);
        assert_eq!(locked_bet(&locks, 8), 0);
        // 이미 풀린 판(중지가 먼저 푼 판)은 다시 풀리지 않는다 → 정산하지 않는다.
        assert!(!release_game_bets(&locks, "game-a"));
        // 배팅이 없는 판은 잠금을 남기지 않는다.
        lock_game_bets(&locks, "game-c", &HashMap::new());
        assert!(!release_game_bets(&locks, "game-c"));
    }

    fn state_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mafia-state-{name}-{}-{}",
            std::process::id(),
            mafia_remake::atomic_file::next_seq()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 폴더 안에서 `<이름>.corrupt-`로 시작하는 파일들 (옮겨 둔 사본).
    fn corrupt_copies(dir: &Path, file_name: &str) -> Vec<PathBuf> {
        let prefix = format!("{file_name}.corrupt-");
        let mut copies = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
            })
            .collect::<Vec<_>>();
        copies.sort();
        copies
    }

    #[test]
    fn corrupt_stats_file_stays_in_place_and_every_restart_stops() {
        let dir = state_temp_dir("stats-corrupt");
        let path = dir.join("stats.json");
        let broken = "{\"users\": {\"1\": {\"coins\": 5";
        std::fs::write(&path, broken).unwrap();

        let error = load_state_file(&path, |path| stats::load_stats(path))
            .expect_err("깨진 stats.json으로 시작하면 안 된다");
        assert!(format!("{error:#}").contains(".corrupt-"), "{error:#}");

        // 원본은 그대로 있어서, 자동 재시작이 다시 불러도 빈 상태로 시작하지 않는다.
        // 복사본은 한 번만 만든다.
        for _ in 0..3 {
            assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
            let copies = corrupt_copies(&dir, "stats.json");
            assert_eq!(copies.len(), 1, "{copies:?}");
            assert_eq!(std::fs::read_to_string(&copies[0]).unwrap(), broken);
            assert!(load_state_file(&path, |path| stats::load_stats(path)).is_err());
        }

        // 내용이 바뀌면 그 내용도 따로 남긴다.
        std::fs::write(&path, "{\"users\": [").unwrap();
        assert!(load_state_file(&path, |path| stats::load_stats(path)).is_err());
        assert_eq!(corrupt_copies(&dir, "stats.json").len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_or_valid_state_files_load_without_being_moved() {
        let dir = state_temp_dir("stats-ok");
        let path = dir.join("stats.json");

        // 파일이 없으면 새로 시작한다.
        let fresh = load_state_file(&path, |path| stats::load_stats(path)).unwrap();
        assert!(fresh.users.is_empty());

        let mut saved = stats::StatsFile::default();
        stats::refund_coins(&mut saved, 1, "p1", 700);
        stats::save_stats(&path, &saved).unwrap();
        let loaded = load_state_file(&path, |path| stats::load_stats(path)).unwrap();
        assert_eq!(loaded.users["1"].coins, 700);
        assert!(path.exists());
        assert!(corrupt_copies(&dir, "stats.json").is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_casino_file_is_copied_aside_and_startup_stops() {
        let dir = state_temp_dir("casino");
        let path = dir.join("casino.json");
        let stats = Arc::new(RwLock::new(stats::StatsFile::default()));
        let stats_path = Arc::new(dir.join("stats.json"));
        let load = |path: &Path| {
            casino_hub::CasinoHub::load(path.to_path_buf(), stats.clone(), stats_path.clone())
        };

        // 파일이 없으면 빈 카지노로 시작한다.
        let hub = load_state_file(&path, load).unwrap();
        assert!(hub.tables.is_empty());

        std::fs::write(&path, "{\"house\": 12, \"tables\": [").unwrap();
        assert!(load_state_file(&path, load).is_err());
        assert!(load_state_file(&path, load).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"house\": 12, \"tables\": ["
        );
        let copies = corrupt_copies(&dir, "casino.json");
        assert_eq!(copies.len(), 1, "{copies:?}");
        assert_eq!(
            std::fs::read_to_string(&copies[0]).unwrap(),
            "{\"house\": 12, \"tables\": ["
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_replays_are_moved_aside_and_the_bot_continues_empty() {
        let dir = state_temp_dir("replays");
        let path = dir.join("replays.json");
        std::fs::write(&path, "[{\"game_key\": ").unwrap();

        let replays = load_replays_or_move_aside(&path).unwrap();

        assert!(replays.is_empty());
        assert!(!path.exists());
        let copies = corrupt_copies(&dir, "replays.json");
        assert_eq!(copies.len(), 1, "{copies:?}");
        assert_eq!(
            std::fs::read_to_string(&copies[0]).unwrap(),
            "[{\"game_key\": "
        );
        // 파일이 없으면 그냥 빈 기록이다.
        assert!(load_replays_or_move_aside(&path).unwrap().is_empty());
        assert_eq!(corrupt_copies(&dir, "replays.json").len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn moving_aside_never_overwrites_an_earlier_copy() {
        let dir = state_temp_dir("move-aside");
        let path = dir.join("stats.json");
        std::fs::write(&path, "first").unwrap();
        let first = move_aside_corrupt_file(&path).unwrap();
        std::fs::write(&path, "second").unwrap();
        let second = move_aside_corrupt_file(&path).unwrap();

        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "second");
        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scientist_initial_replay_team_is_mafia() {
        assert_eq!(RunningGame::role_team_key(Role::Scientist), "mafia");
    }

    #[test]
    fn extracts_explicit_web_url_port() {
        assert_eq!(
            explicit_url_port("https://example.com:8443/web-settings"),
            Some(8443)
        );
        assert_eq!(explicit_url_port("https://example.com"), None);
        assert_eq!(explicit_url_port("http://localhost:8880"), Some(8880));
    }

    #[test]
    fn slash_commands_fit_discord_option_limit() {
        let commands = bot_commands();
        let builders = poise::builtins::create_application_commands(&commands);
        for builder in builders {
            let value = serde_json::to_value(&builder).expect("command serializes");
            let option_count = value
                .get("options")
                .and_then(|options| options.as_array())
                .map_or(0, Vec::len);
            assert!(
                option_count <= 25,
                "{} has {option_count} top-level options",
                value
                    .get("name")
                    .and_then(|name| name.as_str())
                    .unwrap_or("<unknown>")
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let workspace_root = embed::load_workspace_env()?;
    // `mafia --casino-dev`: Discord 없이 카지노 웹만 띄운다 (UI 개발·점검용).
    if std::env::args().any(|arg| arg == "--casino-dev") {
        return casino_web::run_dev_server(&workspace_root).await;
    }
    let token =
        std::env::var("DISCORD_TOKEN").context(".env 파일에 DISCORD_TOKEN을 설정하세요.")?;
    // 워커 봇 토큰이 있으면 길드 관리용 REST 호출을 여러 토큰으로 분산해 레이트리밋을 우회한다.
    http_pool::init_from_env().await;
    let config_path = workspace_root.join("config.json");
    let api_keys_path = workspace_root.join("api_keys.json");
    let stats_path = workspace_root.join("stats.json");
    let completed_replays_path = workspace_root.join("replays.json");
    let mut config = config::load_config(&config_path)?;
    // 본 서버는 .env의 HOME_GUILD_ID가 config.json보다 우선한다.
    if let Ok(value) = std::env::var("HOME_GUILD_ID")
        && let Some(home_guild_id) = config::parse_home_guild_id(&value)?
    {
        config.home_guild_id = home_guild_id;
    }
    let api_keys = web_settings::load_api_key_store(&api_keys_path)?;
    let stats = load_state_file(&stats_path, |path| stats::load_stats(path))?;
    let mut loaded_replays = load_replays_or_move_aside(&completed_replays_path)?;
    while loaded_replays.len() > COMPLETED_REPLAY_LIMIT {
        loaded_replays.pop_back();
    }
    let web_host = std::env::var("WEB_SETTINGS_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let web_port = std::env::var("WEB_SETTINGS_PORT")
        .unwrap_or_else(|_| "8800".to_string())
        .parse::<u16>()
        .context("WEB_SETTINGS_PORT는 1~65535 사이 숫자여야 합니다.")?;
    let web_settings_base_url = std::env::var("WEB_SETTINGS_BASE_URL").ok();
    warn_cloudflare_https_port(web_settings_base_url.as_deref());
    warn_settings_web_exposure(web_settings_base_url.as_deref(), web_port);
    let web_base_url_https = web_settings_base_url
        .as_deref()
        .is_some_and(|url| url.trim_start().starts_with("https://"));
    let explicit_web_tls_cert = std::env::var("WEB_SETTINGS_TLS_CERT").ok();
    let explicit_web_tls_key = std::env::var("WEB_SETTINGS_TLS_KEY").ok();
    let web_tls_cert = explicit_web_tls_cert.or_else(|| {
        web_base_url_https
            .then(|| std::env::var("ACTIVITY_TLS_CERT").ok())
            .flatten()
    });
    let web_tls_key = explicit_web_tls_key.or_else(|| {
        web_base_url_https
            .then(|| std::env::var("ACTIVITY_TLS_KEY").ok())
            .flatten()
    });
    let web_tls_enabled = web_tls_cert.is_some() && web_tls_key.is_some();
    if web_base_url_https && !web_tls_enabled {
        eprintln!("WEB_SETTINGS_BASE_URL uses https but web settings TLS cert/key are missing.");
    }
    let web_base_url = web_settings::base_url(&web_host, web_port, web_tls_enabled);

    // 공유 상태를 Discord 연결 전에 먼저 생성
    let games: Arc<DashMap<serenity::GuildId, Arc<RwLock<RunningGame>>>> = Arc::new(DashMap::new());
    let bet_locks: BetLocks = Arc::new(std::sync::Mutex::new(HashMap::new()));
    let completed_replays: Arc<RwLock<VecDeque<Value>>> = Arc::new(RwLock::new(loaded_replays));
    let recruitments: Arc<DashMap<serenity::GuildId, Arc<RwLock<Recruitment>>>> =
        Arc::new(DashMap::new());
    let recruitment_update_versions: Arc<DashMap<serenity::GuildId, Arc<AtomicU64>>> =
        Arc::new(DashMap::new());
    let config_arc = Arc::new(RwLock::new(config));
    let api_keys_arc = Arc::new(RwLock::new(api_keys));
    let stats_arc = Arc::new(RwLock::new(stats));
    let web_sessions: Arc<DashMap<String, web_settings::WebSettingsSession>> =
        Arc::new(DashMap::new());
    let config_path_arc = Arc::new(config_path);
    let api_keys_path_arc = Arc::new(api_keys_path);
    let stats_path_arc = Arc::new(stats_path);
    let completed_replays_path_arc = Arc::new(completed_replays_path);
    let (activity_discord_update_tx, _) = tokio::sync::broadcast::channel(64);

    // Activity 서버를 Discord 연결 전에 즉시 시작 (Fly.io health check 통과용)
    let activity_port = std::env::var("ACTIVITY_PORT")
        .unwrap_or_else(|_| "8802".to_string())
        .parse::<u16>()
        .unwrap_or(8802);
    let activity_client_id = std::env::var("DISCORD_CLIENT_ID").unwrap_or_default();
    let activity_client_secret = std::env::var("DISCORD_CLIENT_SECRET").unwrap_or_default();
    let activity_static = std::env::var("ACTIVITY_STATIC_DIR").ok();
    let activity_tls_cert = std::env::var("ACTIVITY_TLS_CERT").ok();
    let activity_tls_key = std::env::var("ACTIVITY_TLS_KEY").ok();
    // 카지노: 테이블 상태(casino.json)와 개인 링크 세션은 봇과 웹이 같이 쓴다.
    let casino_hub: casino_hub::SharedHub = Arc::new(load_state_file(
        &workspace_root.join("casino.json"),
        |path| {
            casino_hub::CasinoHub::load(
                path.to_path_buf(),
                stats_arc.clone(),
                stats_path_arc.clone(),
            )
        },
    )?);
    // casino.json은 변경마다 기다려 쓰지 않고 모아서 쓴다 (코인이 오가는 변경은 바로 쓴다).
    casino_hub.start_saver();
    // 코인 순환: 홀덤 레이크 등 운영 비율과, 바이인이 남겨야 할 마피아 배팅 잠금.
    casino_hub.connect_economy(config_arc.clone(), bet_locks.clone());
    // 주식 시장: 상태(stocks.json)와 코인 장부. 게임 연동 종목을 위해 서버 활동을 함께 센다.
    let server_activity = Arc::new(stock_hub::ServerActivity::default());
    casino_hub.connect_activity(server_activity.clone());
    let stock_market: stock_hub::SharedStocks = Arc::new(load_state_file(
        &workspace_root.join("stocks.json"),
        |path| {
            stock_hub::StockHub::load(
                path.to_path_buf(),
                stats_arc.clone(),
                stats_path_arc.clone(),
            )
        },
    )?);
    stock_market.connect(
        config_arc.clone(),
        bet_locks.clone(),
        server_activity.clone(),
    );
    stock_market.recover().await;
    let casino_base_url = casino_hub::casino_base_url(
        &web_host,
        activity_port,
        activity_tls_cert.is_some() && activity_tls_key.is_some(),
    );
    // 카지노 웹(/casino)과 증권 사이트(/stocks)는 Activity 서버에 함께 얹힌다.
    let casino_router = casino_web::casino_router(casino_web::CasinoWebState {
        hub: casino_hub.clone(),
        static_dir: std::env::var("CASINO_STATIC_DIR").ok(),
    })
    .merge(stock_web::stocks_router(stock_web::StocksWebState {
        sessions: casino_hub.clone(),
        stocks: stock_market.clone(),
        static_dir: std::env::var("STOCKS_STATIC_DIR").ok(),
    }));
    let activity_state = activity::ActivityState::new(
        games.clone(),
        config_arc.clone(),
        activity_client_id,
        activity_client_secret,
        activity_discord_update_tx.clone(),
    );
    let activity_host = web_host.clone();
    tokio::spawn(async move {
        activity::run_activity_server(
            activity_state,
            activity_host,
            activity_port,
            activity_static,
            activity_tls_cert,
            activity_tls_key,
            casino_router,
        )
        .await;
    });

    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::GUILD_MEMBERS
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_PRESENCES;

    let games_setup = games.clone();
    let bet_locks_setup = bet_locks.clone();
    let completed_replays_setup = completed_replays.clone();
    let recruitments_setup = recruitments.clone();
    let recruitment_update_versions_setup = recruitment_update_versions.clone();
    let config_setup = config_arc.clone();
    let api_keys_setup = api_keys_arc.clone();
    let stats_setup = stats_arc.clone();
    let web_sessions_setup = web_sessions.clone();
    let config_path_setup = config_path_arc.clone();
    let api_keys_path_setup = api_keys_path_arc.clone();
    let stats_path_setup = stats_path_arc.clone();
    let completed_replays_path_setup = completed_replays_path_arc.clone();
    let activity_discord_update_setup = activity_discord_update_tx.clone();
    let casino_setup = casino_hub.clone();
    let stocks_setup = stock_market.clone();
    let activity_setup = server_activity.clone();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: bot_commands(),
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                match register_global_commands_preserving_activity(
                    ctx,
                    &framework.options().commands,
                )
                .await
                {
                    Ok(count) => println!("Global commands registered: {count}"),
                    Err(e) => eprintln!("Global command registration warning: {e}"),
                }
                println!("Rust Mafia bot ready: {}", ready.user.name);
                // 본 서버가 비어 있으면 정한다. 코인·설정·카지노는 모든 서버가 함께 쓰므로
                // 관리 명령은 본 서버에서만 받는다 (channel::require_manager).
                {
                    let mut config_write = config_setup.write().await;
                    if config_write.home_guild_id == 0 {
                        let guild_ids = ready
                            .guilds
                            .iter()
                            .map(|guild| guild.id.get())
                            .collect::<Vec<_>>();
                        match guild_ids.as_slice() {
                            [] => {}
                            [only] => {
                                config_write.home_guild_id = *only;
                                match config::save_config(&*config_path_setup, &config_write) {
                                    Ok(()) => println!(
                                        "본 서버를 {only}(으)로 정해 config.json에 저장했습니다. 관리 명령은 이 서버에서만 받습니다."
                                    ),
                                    Err(error) => eprintln!(
                                        "본 서버를 {only}(으)로 정했지만 config.json에 저장하지 못했습니다: {error:?}"
                                    ),
                                }
                            }
                            several => eprintln!(
                                "경고: 봇이 여러 서버 {several:?}에 있어 본 서버를 정하지 못했습니다. .env에 HOME_GUILD_ID를 설정하기 전까지 관리 명령은 모든 서버에서 막힙니다."
                            ),
                        }
                    }
                }
                let data = Data {
                    config: config_setup.clone(),
                    config_path: config_path_setup.clone(),
                    stats: stats_setup.clone(),
                    stats_path: stats_path_setup.clone(),
                    games: games_setup.clone(),
                    bet_locks: bet_locks_setup.clone(),
                    completed_replays: completed_replays_setup.clone(),
                    completed_replays_path: completed_replays_path_setup.clone(),
                    recruitments: recruitments_setup.clone(),
                    recruitment_update_versions: recruitment_update_versions_setup.clone(),
                    web_sessions: web_sessions_setup.clone(),
                    web_base_url: Arc::new(web_base_url.clone()),
                    bot_user_id: ready.user.id,
                    casino: casino_setup.clone(),
                    casino_base_url: Arc::new(casino_base_url.clone()),
                    stocks: stocks_setup.clone(),
                    activity: activity_setup.clone(),
                };
                // 카지노: 시간 초과 처리 루프와 Discord 채널 중계.
                tokio::spawn(commands::run_casino_ticker(data.clone()));
                tokio::spawn(commands::run_economy_ticker(ctx.clone(), data.clone()));
                tokio::spawn(commands::run_stock_ticker(ctx.clone(), data.clone()));
                tokio::spawn(commands::run_casino_relay(ctx.clone(), data.clone()));
                let mut activity_update_rx = activity_discord_update_setup.subscribe();
                let activity_update_ctx = ctx.clone();
                let activity_update_games = games_setup.clone();
                let activity_update_data = data.clone();
                tokio::spawn(async move {
                    while let Ok(update) = activity_update_rx.recv().await {
                        match update {
                            activity::ActivityDiscordUpdate::DeliverLiveNotices { guild_id } => {
                                let Some(running) = activity_update_games
                                    .get(&guild_id)
                                    .map(|entry| entry.clone())
                                else {
                                    continue;
                                };
                                commands::deliver_live_notices(&activity_update_ctx, &running)
                                    .await;
                            }
                            activity::ActivityDiscordUpdate::PrivateRoleStatus {
                                guild_id,
                                role,
                            } => {
                                let Some(running) = activity_update_games
                                    .get(&guild_id)
                                    .map(|entry| entry.clone())
                                else {
                                    continue;
                                };
                                channel::upsert_private_role_status_message(
                                    &activity_update_ctx,
                                    &running,
                                    role,
                                )
                                .await;
                            }
                            activity::ActivityDiscordUpdate::GrantPrivateRoleAccess {
                                guild_id,
                                user_id,
                                role,
                            } => {
                                let Some(running) = activity_update_games
                                    .get(&guild_id)
                                    .map(|entry| entry.clone())
                                else {
                                    continue;
                                };
                                let player = running.read().await.game.get_player(user_id).cloned();
                                if let Some(player) = player {
                                    channel::grant_private_role_member_access(
                                        &activity_update_ctx,
                                        &activity_update_data,
                                        &running,
                                        role,
                                        &player,
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                });
                let web_state = web_settings::WebSettingsState {
                    config: config_setup,
                    config_path: config_path_setup,
                    api_keys: api_keys_setup,
                    api_keys_path: api_keys_path_setup,
                    stats: stats_setup,
                    games: games_setup,
                    completed_replays: completed_replays_setup,
                    recruitments: recruitments_setup,
                    sessions: web_sessions_setup,
                    started_at: Instant::now(),
                    bot_name: ready.user.name.clone(),
                    guild_count: ready.guilds.len(),
                    base_url: web_base_url.clone(),
                };
                let host = web_host.clone();
                tokio::spawn(async move {
                    if let Err(error) = web_settings::run_server(
                        web_state,
                        host,
                        web_port,
                        web_tls_cert,
                        web_tls_key,
                    )
                    .await
                    {
                        eprintln!("Rust web settings server error: {error:?}");
                    }
                });

                Ok(data)
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await?;
    let result = client.start().await;
    // 봇이 멈추기 전에 아직 쓰지 않은 카지노·주식 상태를 저장한다.
    casino_hub.flush_pending_save().await;
    stock_market.flush().await;
    result?;
    Ok(())
}
