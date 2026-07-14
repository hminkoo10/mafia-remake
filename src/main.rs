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
use std::{path::PathBuf, sync::Arc, time::Instant};
use tokio::sync::{Notify, RwLock};

const RECRUITMENT_SECONDS: u64 = 60;
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
    completed_replays: Arc<RwLock<VecDeque<Value>>>,
    completed_replays_path: Arc<PathBuf>,
    recruitments: Arc<DashMap<serenity::GuildId, Arc<RwLock<Recruitment>>>>,
    web_sessions: Arc<DashMap<String, web_settings::WebSettingsSession>>,
    web_base_url: Arc<String>,
    bot_user_id: serenity::UserId,
}

#[derive(Debug, Clone, Default)]
struct ContractorContractDraft {
    target_ids: [Option<u64>; 2],
    guessed_roles: [Option<Role>; 2],
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
    anonymous_shaman_input_channel_ids: HashMap<u64, serenity::ChannelId>,
    anonymous_shaman_input_channel_owners: HashMap<serenity::ChannelId, u64>,
    anonymous_role_input_channel_ids: HashMap<(u64, Role), serenity::ChannelId>,
    anonymous_role_input_channels: HashMap<serenity::ChannelId, (u64, Role)>,
    anonymous_role_input_status_message_ids: HashMap<(u64, Role), serenity::MessageId>,
    anonymous_role_status_texts: HashMap<(u64, Role), String>,
    anonymous_channel_topics: HashMap<serenity::ChannelId, String>,
    anonymous_webhook_urls: HashMap<serenity::ChannelId, String>,
    original_game_channel_overwrites:
        HashMap<serenity::RoleId, Option<serenity::PermissionOverwrite>>,
    game_channel_overwrites: HashMap<serenity::RoleId, Option<serenity::PermissionOverwrite>>,
    member_channel_overwrites: HashMap<u64, Option<serenity::PermissionOverwrite>>,
    original_slowmode_delays: HashMap<serenity::ChannelId, u16>,
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
        } else if role == Role::Scientist {
            "citizen"
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
    role_counts: HashMap<Role, usize>,
    special_roles: Vec<Role>,
    max_players: usize,
    minimum_players: usize,
    joined_ids: HashSet<u64>,
    joined_names: HashMap<u64, String>,
    spectator_ids: HashSet<u64>,
    spectator_names: HashMap<u64, String>,
    accepting: bool,
    cancelled: bool,
    done: Arc<Notify>,
}

mod activity;
mod channel;
mod commands;
mod embed;
mod runner;

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

fn explicit_url_port(url: &str) -> Option<u16> {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = without_scheme.split('/').next().unwrap_or_default();
    let (_, port) = authority.rsplit_once(':')?;
    port.parse().ok()
}

#[cfg(test)]
mod main_tests {
    use super::*;

    #[test]
    fn scientist_initial_replay_team_is_citizen() {
        assert_eq!(RunningGame::role_team_key(Role::Scientist), "citizen");
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let workspace_root = embed::load_workspace_env()?;
    let token =
        std::env::var("DISCORD_TOKEN").context(".env 파일에 DISCORD_TOKEN을 설정하세요.")?;
    let config_path = workspace_root.join("config.json");
    let api_keys_path = workspace_root.join("api_keys.json");
    let stats_path = workspace_root.join("stats.json");
    let completed_replays_path = workspace_root.join("replays.json");
    let config = config::load_config(&config_path)?;
    let api_keys = web_settings::load_api_key_store(&api_keys_path)?;
    let stats = stats::load_stats(&stats_path).unwrap_or_default();
    let mut loaded_replays =
        web_settings::load_completed_replays(&completed_replays_path).unwrap_or_default();
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
    let completed_replays: Arc<RwLock<VecDeque<Value>>> = Arc::new(RwLock::new(loaded_replays));
    let recruitments: Arc<DashMap<serenity::GuildId, Arc<RwLock<Recruitment>>>> =
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
        )
        .await;
    });

    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::GUILD_MEMBERS
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_PRESENCES;

    let games_setup = games.clone();
    let completed_replays_setup = completed_replays.clone();
    let recruitments_setup = recruitments.clone();
    let config_setup = config_arc.clone();
    let api_keys_setup = api_keys_arc.clone();
    let stats_setup = stats_arc.clone();
    let web_sessions_setup = web_sessions.clone();
    let config_path_setup = config_path_arc.clone();
    let api_keys_path_setup = api_keys_path_arc.clone();
    let stats_path_setup = stats_path_arc.clone();
    let completed_replays_path_setup = completed_replays_path_arc.clone();
    let activity_discord_update_setup = activity_discord_update_tx.clone();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::start_game(),
                commands::stop_game(),
                commands::cleanup_stuck_game(),
                commands::disable_mafia_game(),
                commands::enable_mafia_game(),
                commands::add_to_blacklist(),
                commands::remove_from_blacklist(),
                commands::show_blacklist(),
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
                commands::rating_log(),
                commands::show_leaderboard(),
                commands::reset_leaderboard(),
                commands::show_term_info(),
                commands::show_term_descriptions(),
                commands::show_role_info(),
                commands::show_abilities(),
                commands::show_role_descriptions(),
            ],
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
                let mut activity_update_rx = activity_discord_update_setup.subscribe();
                let activity_update_ctx = ctx.clone();
                let activity_update_games = games_setup.clone();
                tokio::spawn(async move {
                    while let Ok(update) = activity_update_rx.recv().await {
                        match update {
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
                        }
                    }
                });
                let data = Data {
                    config: config_setup.clone(),
                    config_path: config_path_setup.clone(),
                    stats: stats_setup.clone(),
                    stats_path: stats_path_setup,
                    games: games_setup.clone(),
                    completed_replays: completed_replays_setup.clone(),
                    completed_replays_path: completed_replays_path_setup,
                    recruitments: recruitments_setup.clone(),
                    web_sessions: web_sessions_setup.clone(),
                    web_base_url: Arc::new(web_base_url.clone()),
                    bot_user_id: ready.user.id,
                };
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
    client.start().await?;
    Ok(())
}
