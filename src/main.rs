use ab_glyph::{
    Font, FontArc, GlyphId, OutlinedGlyph, PxScale, Rect as GlyphRect, ScaleFont, point,
};
use anyhow::{Context as AnyhowContext, Result, bail};
use dashmap::DashMap;
use image::{ImageFormat, Rgb, RgbImage};
use mafia_remake::game::{GameCounts, MafiaGame};
use mafia_remake::model::{
    CITIZEN_SPECIAL_ROLES, CONTRACTOR_GUESS_ROLES, MAFIA_SPECIAL_ROLES, NEUTRAL_SPECIAL_ROLES,
    NightResult, PUBLIC_CITIZEN_SPECIAL_ROLES, PUBLIC_CULT_SPECIAL_ROLES,
    PUBLIC_MAFIA_SPECIAL_ROLES, PUBLIC_NEUTRAL_SPECIAL_ROLES, Phase, Player, Role, VoteResult,
    Winner,
};
use mafia_remake::{config, stats};
use poise::serenity_prelude as serenity;
use secrecy::ExposeSecret;
mod web_settings;

use poise::serenity_prelude::Mentionable;
use rand::seq::{IndexedRandom, SliceRandom};
use std::collections::{HashMap, HashSet};
use std::{
    io::Cursor,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Notify, RwLock};

const RECRUITMENT_SECONDS: u64 = 60;
const MAX_GAME_PLAYERS: usize = 24;
const DAY_EXTENSION_VOTE_SECONDS: u64 = 10;
const DISCUSSION_EXTENSION_SECONDS: u64 = 60;
const CONFIRM_VOTE_SECONDS: u64 = 15;
const GAME_NOTIFICATION_ROLE: &str = "게임알림";
const SPECTATOR_ROLE: &str = "관전자";
const DEAD_PLAYER_ROLE: &str = "사망자";
const SHAMAN_CHAT_CHANNEL_NAME: &str = "영매-채팅방";
const FROG_CHAT_CHANNEL_NAME: &str = "개구리-채팅방";

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
    frog_channel_id: Option<serenity::ChannelId>,
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
    night_notify: Arc<Notify>,
    vote_notify: Arc<Notify>,
    confirm_notify: Arc<Notify>,
    day_notify: Arc<Notify>,
    stats_recorded: bool,
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

fn make_embed(
    message: impl Into<String>,
    title: &str,
    color: serenity::Colour,
) -> serenity::CreateEmbed {
    let message = message.into();
    let mut lines = message.lines();
    let description = if let Some(first) = lines.next() {
        let first = if first.contains("**") {
            first.to_string()
        } else {
            format!("**{first}**")
        };
        std::iter::once(first)
            .chain(lines.map(str::to_string))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        message
    };

    serenity::CreateEmbed::new()
        .title(format!("[마피아] {title}"))
        .description(description)
        .color(color)
        .author(serenity::CreateEmbedAuthor::new("마피아 게임 알림"))
        .footer(serenity::CreateEmbedFooter::new("마피아 게임 진행 메시지"))
}

async fn reply_embed(
    ctx: Context<'_>,
    message: impl Into<String>,
    title: &str,
    color: serenity::Colour,
    ephemeral: bool,
) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default()
            .embed(make_embed(message, title, color))
            .ephemeral(ephemeral),
    )
    .await?;
    Ok(())
}

async fn send_channel_embed(
    http: &serenity::Http,
    channel_id: serenity::ChannelId,
    message: impl Into<String>,
    title: &str,
    color: serenity::Colour,
    components: Vec<serenity::CreateActionRow>,
) -> serenity::Result<serenity::Message> {
    channel_id
        .send_message(
            http,
            serenity::CreateMessage::new()
                .embed(make_embed(message, title, color))
                .components(components),
        )
        .await
}

#[allow(clippy::too_many_arguments)]
async fn send_game_embed(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    message: impl Into<String>,
    title: &str,
    color: serenity::Colour,
    components: Vec<serenity::CreateActionRow>,
    include_dead: bool,
    broadcast: bool,
) -> serenity::Result<serenity::Message> {
    let message = message.into();
    let (channel_id, anonymous_enabled, targets) = {
        let running_read = running.read().await;
        let targets = if broadcast && running_read.anonymous_enabled {
            let players = if include_dead {
                running_read.game.players.clone()
            } else {
                running_read
                    .game
                    .alive_players()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>()
            };
            players
                .into_iter()
                .filter_map(|player| {
                    running_read
                        .anonymous_input_channel_ids
                        .get(&player.user_id)
                        .copied()
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        (
            running_read.channel_id,
            running_read.anonymous_enabled,
            targets,
        )
    };
    let sent = send_channel_embed(
        &ctx.http,
        channel_id,
        message.clone(),
        title,
        color,
        components.clone(),
    )
    .await?;
    if broadcast && anonymous_enabled {
        for channel_id in targets {
            let _ = send_channel_embed(
                &ctx.http,
                channel_id,
                message.clone(),
                title,
                color,
                components.clone(),
            )
            .await;
        }
    }
    Ok(sent)
}

async fn send_player_secret(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    message: impl Into<String>,
    components: Vec<serenity::CreateActionRow>,
) -> bool {
    let message = message.into();
    let anonymous_channel_id = {
        let running_read = running.read().await;
        running_read
            .anonymous_enabled
            .then(|| {
                running_read
                    .anonymous_input_channel_ids
                    .get(&player.user_id)
                    .copied()
            })
            .flatten()
    };
    if let Some(channel_id) = anonymous_channel_id
        && send_channel_embed(
            &ctx.http,
            channel_id,
            message.clone(),
            "비밀 메시지",
            serenity::Colour::GOLD,
            components.clone(),
        )
        .await
        .is_ok()
    {
        return true;
    }
    let Ok(user) = serenity::UserId::new(player.user_id).to_user(ctx).await else {
        return false;
    };
    user.direct_message(
        ctx,
        serenity::CreateMessage::new()
            .embed(make_embed(message, "비밀 메시지", serenity::Colour::GOLD))
            .components(components),
    )
    .await
    .is_ok()
}

fn duration_text(seconds: u64) -> String {
    if seconds.is_multiple_of(60) {
        format!("{}분", seconds / 60)
    } else {
        format!("{seconds}초")
    }
}

fn day_skip_components(
    guild_id: serenity::GuildId,
    disabled: bool,
    confirmed: bool,
) -> Vec<serenity::CreateActionRow> {
    vec![serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(format!("skipday:{}", guild_id.get()))
            .label(if confirmed {
                "투표 확정"
            } else {
                "바로 투표"
            })
            .style(serenity::ButtonStyle::Primary)
            .disabled(disabled),
    ])]
}

fn day_extension_components(
    guild_id: serenity::GuildId,
    disabled: bool,
    confirmed: bool,
) -> Vec<serenity::CreateActionRow> {
    vec![serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(format!("extendday:{}", guild_id.get()))
            .label(if confirmed {
                "연장 확정"
            } else {
                "1분 연장"
            })
            .style(serenity::ButtonStyle::Secondary)
            .disabled(disabled),
    ])]
}

async fn send_component_private(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    message: impl Into<String>,
) -> serenity::Result<()> {
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Message(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(message, "마피아 게임", serenity::Colour::RED))
                    .ephemeral(true),
            ),
        )
        .await
}

async fn ack_component(ctx: &serenity::Context, component: &serenity::ComponentInteraction) {
    let _ = component
        .create_response(ctx, serenity::CreateInteractionResponse::Acknowledge)
        .await;
}

fn workspace_path(file_name: &str) -> Result<PathBuf> {
    Ok(std::env::current_dir()
        .context("현재 작업 디렉터리를 확인하지 못했습니다.")?
        .join(file_name))
}

fn display_name(member: &serenity::Member) -> String {
    member
        .nick
        .clone()
        .or_else(|| member.user.global_name.clone())
        .unwrap_or_else(|| member.user.name.clone())
}

async fn role_by_name(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    name: &str,
) -> Result<Option<serenity::Role>> {
    let roles = guild_id.roles(&ctx.http).await?;
    Ok(roles.into_values().find(|role| role.name == name))
}

const ANIMAL_ALIASES: &[&str] = &[
    "사자",
    "호랑이",
    "고양이",
    "강아지",
    "토끼",
    "판다",
    "곰",
    "여우",
    "늑대",
    "돼지",
    "원숭이",
    "코끼리",
    "기린",
    "펭귄",
    "오리",
    "병아리",
    "부엉이",
    "독수리",
    "거북이",
    "돌고래",
    "상어",
    "고래",
    "악어",
    "뱀",
    "나비",
    "벌",
    "개미",
    "달팽이",
    "문어",
    "물고기",
    "게",
    "새우",
    "오징어",
    "말",
    "얼룩말",
    "소",
    "양",
    "염소",
    "닭",
    "쥐",
    "햄스터",
    "사슴",
    "라마",
    "캥거루",
    "하마",
    "코뿔소",
    "박쥐",
    "고슴도치",
    "수달",
    "비버",
    "너구리",
    "스컹크",
    "공작",
    "앵무새",
    "백조",
    "플라밍고",
    "칠면조",
    "고릴라",
    "오랑우탄",
    "물개",
];

const NUMBER_AVATAR_COLORS: &[&str] = &[
    "e11d48", "2563eb", "16a34a", "f59e0b", "7c3aed", "0891b2", "db2777", "65a30d", "dc2626",
    "4f46e5", "0f766e", "ea580c", "9333ea", "0284c7", "ca8a04", "be123c", "1d4ed8", "15803d",
    "b45309", "6d28d9", "0369a1", "a21caf", "047857", "c2410c",
];

#[derive(Clone, Copy)]
struct ChannelRoleIds {
    everyone: serenity::RoleId,
    participant: Option<serenity::RoleId>,
    spectator: Option<serenity::RoleId>,
    manager: Option<serenity::RoleId>,
    dead: Option<serenity::RoleId>,
    bot: serenity::UserId,
}

fn sanitize_channel_part(value: &str) -> String {
    value.replace([' ', '/'], "-").to_lowercase()
}

fn private_channel_name(role: Role) -> &'static str {
    match role {
        Role::Mafia => "마피아-비밀방",
        Role::Police => "경찰-비밀방",
        Role::Agent => "요원-비밀방",
        Role::Vigilante => "자경단원-비밀방",
        Role::Doctor => "의사-비밀방",
        Role::CultLeader => "교주-비밀방",
        Role::Lover => "연인-비밀방",
        _ => "역할-비밀방",
    }
}

fn normalized_anonymous_name_mode(config: &config::BotConfig) -> &str {
    if config.anonymous_name_mode == "number" {
        "number"
    } else {
        "animal"
    }
}

fn anonymous_name_mode_text(config: &config::BotConfig) -> &'static str {
    if normalized_anonymous_name_mode(config) == "number" {
        "숫자 이름"
    } else {
        "동물 이름"
    }
}

fn animal_emoji_code(label: &str) -> Option<&'static str> {
    match label {
        "사자" => Some("1f981"),
        "호랑이" => Some("1f42f"),
        "고양이" => Some("1f431"),
        "강아지" => Some("1f436"),
        "토끼" => Some("1f430"),
        "판다" => Some("1f43c"),
        "곰" => Some("1f43b"),
        "여우" => Some("1f98a"),
        "늑대" => Some("1f43a"),
        "돼지" => Some("1f437"),
        "원숭이" => Some("1f435"),
        "코끼리" => Some("1f418"),
        "기린" => Some("1f992"),
        "펭귄" => Some("1f427"),
        "오리" => Some("1f986"),
        "병아리" => Some("1f424"),
        "부엉이" => Some("1f989"),
        "독수리" => Some("1f985"),
        "거북이" => Some("1f422"),
        "돌고래" => Some("1f42c"),
        "상어" => Some("1f988"),
        "고래" => Some("1f433"),
        "악어" => Some("1f40a"),
        "뱀" => Some("1f40d"),
        "나비" => Some("1f98b"),
        "벌" => Some("1f41d"),
        "개미" => Some("1f41c"),
        "달팽이" => Some("1f40c"),
        "문어" => Some("1f419"),
        "물고기" => Some("1f41f"),
        "게" => Some("1f980"),
        "새우" => Some("1f990"),
        "오징어" => Some("1f991"),
        "말" => Some("1f434"),
        "얼룩말" => Some("1f993"),
        "소" => Some("1f42e"),
        "양" => Some("1f411"),
        "염소" => Some("1f410"),
        "닭" => Some("1f414"),
        "쥐" => Some("1f42d"),
        "햄스터" => Some("1f439"),
        "사슴" => Some("1f98c"),
        "라마" => Some("1f999"),
        "캥거루" => Some("1f998"),
        "하마" => Some("1f99b"),
        "코뿔소" => Some("1f98f"),
        "박쥐" => Some("1f987"),
        "고슴도치" => Some("1f994"),
        "수달" => Some("1f9a6"),
        "비버" => Some("1f9ab"),
        "너구리" => Some("1f99d"),
        "스컹크" => Some("1f9a8"),
        "공작" => Some("1f99a"),
        "앵무새" => Some("1f99c"),
        "백조" => Some("1f9a2"),
        "플라밍고" => Some("1f9a9"),
        "칠면조" => Some("1f983"),
        "고릴라" => Some("1f98d"),
        "오랑우탄" => Some("1f9a7"),
        "물개" => Some("1f9ad"),
        _ => None,
    }
}

fn max_player_setting_text(config: &config::BotConfig) -> String {
    if config.max_player_count == 0 {
        format!("제한 없음(봇 최대 {MAX_GAME_PLAYERS}명)")
    } else {
        format!("{}명", effective_max_player_count(config))
    }
}

fn permission_overwrite(
    kind: serenity::PermissionOverwriteType,
    can_view: bool,
    can_chat: bool,
    can_create_threads: bool,
) -> serenity::PermissionOverwrite {
    let view_bits =
        serenity::Permissions::VIEW_CHANNEL | serenity::Permissions::READ_MESSAGE_HISTORY;
    let chat_bits = serenity::Permissions::SEND_MESSAGES
        | serenity::Permissions::SEND_MESSAGES_IN_THREADS
        | serenity::Permissions::ADD_REACTIONS;
    let thread_bits = serenity::Permissions::CREATE_PUBLIC_THREADS
        | serenity::Permissions::CREATE_PRIVATE_THREADS;

    let mut allow = serenity::Permissions::empty();
    let mut deny = serenity::Permissions::empty();
    if can_view {
        allow |= view_bits;
    } else {
        deny |= view_bits;
    }
    if can_chat {
        allow |= chat_bits;
    } else {
        deny |= chat_bits;
    }
    if can_chat && can_create_threads {
        allow |= thread_bits;
    } else {
        deny |= thread_bits;
    }

    serenity::PermissionOverwrite { allow, deny, kind }
}

fn set_chat_permission_bits(overwrite: &mut serenity::PermissionOverwrite, can_chat: bool) {
    let chat_bits = serenity::Permissions::SEND_MESSAGES
        | serenity::Permissions::SEND_MESSAGES_IN_THREADS
        | serenity::Permissions::ADD_REACTIONS;
    let thread_bits = serenity::Permissions::CREATE_PUBLIC_THREADS
        | serenity::Permissions::CREATE_PRIVATE_THREADS;
    let bits = chat_bits | thread_bits;
    overwrite.allow.remove(bits);
    overwrite.deny.remove(bits);
    if can_chat {
        overwrite.allow |= bits;
    } else {
        overwrite.deny |= bits;
    }
}

fn private_channel_overwrite(
    kind: serenity::PermissionOverwriteType,
    can_chat: bool,
) -> serenity::PermissionOverwrite {
    permission_overwrite(kind, can_chat, can_chat, can_chat)
}

fn dead_channel_overwrite(
    kind: serenity::PermissionOverwriteType,
    can_view: bool,
    can_chat: bool,
) -> serenity::PermissionOverwrite {
    permission_overwrite(kind, can_view, can_chat, can_chat)
}

fn anonymous_input_overwrite(
    kind: serenity::PermissionOverwriteType,
    can_view: bool,
    can_chat: bool,
) -> serenity::PermissionOverwrite {
    permission_overwrite(kind, can_view, can_chat, false)
}

fn spectator_channel_overwrite(role_id: serenity::RoleId) -> serenity::PermissionOverwrite {
    permission_overwrite(
        serenity::PermissionOverwriteType::Role(role_id),
        true,
        false,
        false,
    )
}

async fn channel_role_ids(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    config: &config::BotConfig,
    bot_user_id: serenity::UserId,
) -> Result<ChannelRoleIds> {
    let roles = guild_id.roles(&ctx.http).await?;
    let find_role = |name: &str| {
        roles
            .values()
            .find(|role| role.name == name)
            .map(|role| role.id)
    };
    Ok(ChannelRoleIds {
        everyone: guild_id.everyone_role(),
        participant: find_role(&config.participant_role),
        spectator: find_role(SPECTATOR_ROLE),
        manager: find_role(&config.manager_role),
        dead: find_role(DEAD_PLAYER_ROLE),
        bot: bot_user_id,
    })
}

fn add_common_hidden_overwrites(
    overwrites: &mut Vec<serenity::PermissionOverwrite>,
    roles: ChannelRoleIds,
    private: bool,
) {
    overwrites.push(private_channel_overwrite(
        serenity::PermissionOverwriteType::Role(roles.everyone),
        false,
    ));
    if let Some(role_id) = roles.participant {
        overwrites.push(private_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
        ));
    }
    if let Some(role_id) = roles.spectator {
        overwrites.push(spectator_channel_overwrite(role_id));
    }
    if let Some(role_id) = roles.manager {
        overwrites.push(private_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
        ));
    }
    overwrites.push(if private {
        private_channel_overwrite(serenity::PermissionOverwriteType::Member(roles.bot), true)
    } else {
        anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Member(roles.bot),
            true,
            true,
        )
    });
}

fn anonymous_base_overwrites(
    roles: ChannelRoleIds,
    participant_can_view: bool,
    participant_can_chat: bool,
    default_can_view: bool,
    default_can_chat: bool,
) -> Vec<serenity::PermissionOverwrite> {
    let mut overwrites = vec![anonymous_input_overwrite(
        serenity::PermissionOverwriteType::Role(roles.everyone),
        default_can_view,
        default_can_chat,
    )];
    if let Some(role_id) = roles.participant {
        overwrites.push(anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            participant_can_view,
            participant_can_chat,
        ));
    }
    if let Some(role_id) = roles.spectator {
        overwrites.push(spectator_channel_overwrite(role_id));
    }
    if let Some(role_id) = roles.manager {
        overwrites.push(anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
            false,
        ));
    }
    overwrites.push(anonymous_input_overwrite(
        serenity::PermissionOverwriteType::Member(roles.bot),
        true,
        true,
    ));
    overwrites
}

async fn source_category(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
) -> Option<serenity::ChannelId> {
    let channel = channel_id.to_channel(&ctx.http).await.ok()?.guild()?;
    match channel.kind {
        serenity::ChannelType::PublicThread
        | serenity::ChannelType::PrivateThread
        | serenity::ChannelType::NewsThread => {
            let parent_id = channel.parent_id?;
            parent_id
                .to_channel(&ctx.http)
                .await
                .ok()?
                .guild()?
                .parent_id
        }
        _ => channel.parent_id,
    }
}

#[allow(clippy::too_many_arguments)]
async fn create_text_channel_safe(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    name: &str,
    overwrites: Vec<serenity::PermissionOverwrite>,
    category: Option<serenity::ChannelId>,
    reason: &'static str,
    slowmode_delay: u64,
    topic: Option<String>,
) -> Option<serenity::GuildChannel> {
    let slowmode = slowmode_delay.min(21600) as u16;
    let mut builder = serenity::CreateChannel::new(name)
        .kind(serenity::ChannelType::Text)
        .permissions(overwrites.clone())
        .rate_limit_per_user(slowmode)
        .audit_log_reason(reason);
    if let Some(category_id) = category {
        builder = builder.category(category_id);
    }
    if let Some(topic) = topic.clone() {
        builder = builder.topic(topic.chars().take(1024).collect::<String>());
    }

    match guild_id.create_channel(&ctx.http, builder).await {
        Ok(channel) => Some(channel),
        Err(_) if category.is_some() => {
            let mut fallback = serenity::CreateChannel::new(name)
                .kind(serenity::ChannelType::Text)
                .permissions(overwrites)
                .rate_limit_per_user(slowmode)
                .audit_log_reason(reason);
            if let Some(topic) = topic {
                fallback = fallback.topic(topic.chars().take(1024).collect::<String>());
            }
            guild_id.create_channel(&ctx.http, fallback).await.ok()
        }
        Err(_) => None,
    }
}

fn status_display_name(running: &RunningGame, player: &Player) -> String {
    if running.anonymous_enabled {
        running
            .anonymous_aliases
            .get(&player.user_id)
            .cloned()
            .unwrap_or_else(|| player.name.clone())
    } else {
        player.name.clone()
    }
}

fn mafia_night_target_status_text(running: &RunningGame) -> String {
    if running.game.phase != Phase::Night {
        return String::new();
    }
    let mut actors = running
        .game
        .players
        .iter()
        .filter(|player| {
            player.alive
                && player.role == Role::Mafia
                && running.game.can_mafia_attack(player, None)
        })
        .cloned()
        .collect::<Vec<_>>();
    if actors.is_empty() {
        return String::new();
    }
    actors.sort_by_key(|player| status_display_name(running, player).to_lowercase());
    let mut lines = vec!["마피아 처치 선택 현황".to_string()];
    for actor in actors {
        let target = running
            .game
            .mafia_display_targets
            .get(&actor.user_id)
            .or_else(|| running.game.mafia_targets.get(&actor.user_id))
            .and_then(|target_id| running.game.get_player(*target_id));
        let target_name = target
            .map(|target| status_display_name(running, target))
            .unwrap_or_else(|| "미선택".to_string());
        lines.push(format!(
            "- {} → {}",
            status_display_name(running, &actor),
            target_name
        ));
    }
    lines.join("\n")
}

fn assign_anonymous_aliases(running: &mut RunningGame, config: &config::BotConfig) {
    let mut players = running
        .game
        .players
        .iter()
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    players.sort_unstable();

    let mut aliases = if normalized_anonymous_name_mode(config) == "number" {
        (1..=players.len())
            .map(|index| format!("{index}번"))
            .collect::<Vec<_>>()
    } else {
        ANIMAL_ALIASES
            .iter()
            .map(|alias| (*alias).to_string())
            .collect::<Vec<_>>()
    };
    aliases.shuffle(&mut rand::rng());
    running.anonymous_aliases = players
        .into_iter()
        .enumerate()
        .map(|(index, user_id)| {
            (
                user_id,
                aliases
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| format!("{}번", index + 1)),
            )
        })
        .collect();
}

fn apply_anonymous_player_names(running: &mut RunningGame) {
    if !running.anonymous_enabled {
        return;
    }
    if running.anonymous_original_names.is_empty() {
        running.anonymous_original_names = running
            .game
            .players
            .iter()
            .map(|player| (player.user_id, player.name.clone()))
            .collect();
    }
    for player in &mut running.game.players {
        if let Some(alias) = running.anonymous_aliases.get(&player.user_id) {
            player.name.clone_from(alias);
        }
    }
}

fn lover_chat_is_open(game: &MafiaGame) -> bool {
    game.phase == Phase::Night
        && game
            .alive_players()
            .into_iter()
            .filter(|player| player.role == Role::Lover && !game.is_frog(player))
            .count()
            >= 2
}

fn can_use_anonymous_general_chat(running: &RunningGame, player: &Player) -> bool {
    if !player.alive || running.game.is_frog(player) || running.game.is_madam_seduced(player) {
        return false;
    }
    if running.game.phase == Phase::Day && running.day_chat_open {
        return true;
    }
    running.game.phase == Phase::FinalDefense
        && running.final_defense_user_id == Some(player.user_id)
}

fn can_use_anonymous_role_chat(running: &RunningGame, player: &Player, role: Role) -> bool {
    if running.game.is_frog(player) || running.game.is_madam_seduced(player) {
        return false;
    }
    if role == Role::Lover {
        return player.alive && player.role == Role::Lover && lover_chat_is_open(&running.game);
    }
    if player.alive
        && running
            .anonymous_role_input_channel_ids
            .contains_key(&(player.user_id, role))
    {
        return true;
    }
    if role == Role::Mafia {
        return player.alive && running.game.is_known_mafia_team(player);
    }
    player.alive && player.role == role
}

fn role_chat_player_ids(game: &MafiaGame, role: Role) -> Vec<u64> {
    game.alive_players()
        .into_iter()
        .filter(|player| {
            if role == Role::Mafia {
                game.is_known_mafia_team(player)
            } else {
                player.role == role
            }
        })
        .map(|player| player.user_id)
        .collect()
}

fn anonymous_role_status_player_ids(running: &RunningGame, role: Role) -> Vec<u64> {
    let granted_ids = running
        .anonymous_role_input_channel_ids
        .keys()
        .filter_map(|(user_id, granted_role)| (*granted_role == role).then_some(*user_id))
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut players = running
        .game
        .alive_players()
        .into_iter()
        .filter(|player| !running.game.is_frog(player))
        .filter(|player| {
            granted_ids.contains(&player.user_id)
                || (role == Role::Mafia && running.game.is_known_mafia_team(player))
                || (role == Role::CultLeader && running.game.is_cult_team(player))
                || player.role == role
        })
        .filter(|player| seen.insert(player.user_id))
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    players.sort_by_key(|user_id| {
        running
            .game
            .get_player(*user_id)
            .map(|player| status_display_name(running, player).to_lowercase())
            .unwrap_or_default()
    });
    players
}

fn role_status_player_ids(running: &RunningGame, role: Role) -> Vec<u64> {
    if running.anonymous_enabled {
        anonymous_role_status_player_ids(running, role)
    } else {
        role_chat_player_ids(&running.game, role)
    }
}

fn should_create_private_role_channel(game: &MafiaGame, role: Role) -> bool {
    game.players.iter().any(|player| player.role == role)
        || (role == Role::Mafia
            && game
                .players
                .iter()
                .any(|player| player.role.is_mafia_team() && player.role != Role::Villain))
}

fn special_role_rule_text(role: Role) -> String {
    if role == Role::Lover {
        return "연인은 두 명이 함께 배정됩니다.\n연인 대화방은 밤에만 열리며, 두 연인이 모두 생존 중일 때 사용할 수 있습니다."
            .to_string();
    }
    let action = match role {
        Role::Mafia => "공격",
        Role::Doctor => "보호",
        Role::Police => "조사",
        Role::Agent => "공작",
        Role::Vigilante => "숙청",
        _ => "행동",
    };
    format!(
        "{}가 여러 명이면 같은 대상이 살아있는 {} 인원의 과반 초과를 받아야 {action}이 행사됩니다.\n동률이거나 과반에 못 미치면 그 밤 행동은 행사되지 않습니다.",
        role.value(),
        role.value()
    )
}

async fn require_manager(ctx: Context<'_>) -> Result<bool, Error> {
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버 안에서만 사용할 수 있습니다.",
            "권한 오류",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(false);
    };
    let manager_role = ctx.data().config.read().await.manager_role.clone();
    let member = guild_id
        .member(ctx.serenity_context(), ctx.author().id)
        .await?;
    let roles = guild_id.roles(ctx.serenity_context()).await?;
    let allowed = member.roles.iter().any(|role_id| {
        roles
            .get(role_id)
            .is_some_and(|role| role.name == manager_role)
    });
    if !allowed {
        reply_embed(
            ctx,
            format!("'{manager_role}' 역할을 가진 사람만 사용할 수 있습니다."),
            "권한 오류",
            serenity::Colour::RED,
            true,
        )
        .await?;
    }
    Ok(allowed)
}

fn is_blacklisted(config: &config::BotConfig, user_id: u64) -> bool {
    config.blacklist_user_ids.contains(&user_id)
}

fn enabled_special_roles(config: &config::BotConfig, pool: &[Role]) -> Vec<Role> {
    pool.iter()
        .copied()
        .filter(|role| match role {
            Role::Detective => config.enable_detective,
            Role::Shaman => config.enable_shaman,
            Role::Graverobber => config.enable_graverobber,
            Role::Spy => config.enable_spy,
            Role::Contractor => config.enable_contractor,
            Role::Witch => config.enable_witch,
            Role::Scientist => config.enable_scientist,
            Role::Madam => config.enable_madam,
            Role::Godfather => config.enable_godfather,
            Role::Joker => config.enable_joker,
            Role::Politician => config.enable_politician,
            Role::Judge => config.enable_judge,
            Role::Reporter => config.enable_reporter,
            Role::Hacker => config.enable_hacker,
            Role::Terrorist => config.enable_terrorist,
            Role::Lover => config.enable_lover,
            Role::Priest => config.enable_priest,
            Role::Soldier => config.enable_soldier,
            Role::Nurse => config.enable_nurse,
            Role::Gangster => config.enable_gangster,
            Role::Prophet => config.enable_prophet,
            Role::Psychologist => config.enable_psychologist,
            Role::Thief => config.enable_thief,
            _ => true,
        })
        .collect()
}

fn choose_special_roles(config: &config::BotConfig) -> Result<Vec<Role>> {
    let mut rng = rand::rng();
    let mut selected = Vec::new();
    for (pool, count) in [
        (CITIZEN_SPECIAL_ROLES, config.citizen_special_count as usize),
        (MAFIA_SPECIAL_ROLES, config.mafia_special_count as usize),
        (NEUTRAL_SPECIAL_ROLES, config.neutral_special_count as usize),
    ] {
        let candidates = enabled_special_roles(config, pool);
        if count > candidates.len() {
            bail!(
                "{} 중 활성화된 역할보다 선택할 특수룰 수가 많습니다.",
                pool.iter()
                    .map(|role| role.value())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        selected.extend(candidates.choose_multiple(&mut rng, count).copied());
    }
    Ok(selected)
}

fn expand_special_roles(roles: &[Role]) -> Vec<Role> {
    let mut expanded = Vec::new();
    for role in roles {
        if *role == Role::Lover {
            expanded.extend([Role::Lover, Role::Lover]);
        } else {
            expanded.push(*role);
        }
    }
    expanded
}

fn selected_role_counts(
    config: &config::BotConfig,
    special_roles: &[Role],
) -> Result<HashMap<Role, usize>> {
    let mafia_special_count = special_roles
        .iter()
        .filter(|role| role.is_mafia_team())
        .count();
    if mafia_special_count > config.default_mafia_count as usize {
        bail!(
            "마피아 특수룰 수는 전체 마피아 수보다 많을 수 없습니다. 현재 마피아 {}명, 마피아 특수 {}명입니다.",
            config.default_mafia_count,
            mafia_special_count
        );
    }
    if config.default_mafia_count as usize - mafia_special_count < 1 {
        bail!(
            "접선 전 특수 마피아만으로는 게임을 진행할 수 없습니다. 일반 마피아가 최소 1명 필요합니다."
        );
    }
    let mut counts = HashMap::new();
    counts.insert(
        Role::Mafia,
        config.default_mafia_count as usize - mafia_special_count,
    );
    counts.insert(Role::Doctor, config.default_doctor_count as usize);
    if config.default_police_count > 0 {
        let investigation = random_investigation_role(config);
        counts.insert(investigation, config.default_police_count as usize);
    }
    for role in special_roles {
        *counts.entry(*role).or_default() += if *role == Role::Lover { 2 } else { 1 };
    }
    if config.enable_cult_team {
        *counts.entry(Role::CultLeader).or_default() += 1;
        *counts.entry(Role::Fanatic).or_default() += 1;
    }
    Ok(counts)
}

fn random_investigation_role(config: &config::BotConfig) -> Role {
    let mut candidates = vec![Role::Police];
    if config.use_agent {
        candidates.push(Role::Agent);
    }
    if config.use_vigilante {
        candidates.push(Role::Vigilante);
    }
    let mut rng = rand::rng();
    *candidates.choose(&mut rng).unwrap_or(&Role::Police)
}

fn minimum_player_count(role_counts: &HashMap<Role, usize>) -> usize {
    let special_count = role_counts.values().sum::<usize>();
    let mafia_count = role_counts
        .iter()
        .filter(|(role, _)| role.is_mafia_team())
        .map(|(_, count)| *count)
        .sum::<usize>();
    3.max(special_count).max(mafia_count * 2 + 1)
}

fn effective_max_player_count(config: &config::BotConfig) -> usize {
    if config.max_player_count == 0 {
        MAX_GAME_PLAYERS
    } else {
        (config.max_player_count as usize).min(MAX_GAME_PLAYERS)
    }
}

fn count_group(role_counts: &HashMap<Role, usize>, roles: &[Role]) -> usize {
    roles
        .iter()
        .map(|role| role_counts.get(role).copied().unwrap_or(0))
        .sum()
}

fn public_role_count_text_from_counts(
    role_counts: &HashMap<Role, usize>,
    total_players: Option<usize>,
) -> String {
    let mafia_special = count_group(role_counts, PUBLIC_MAFIA_SPECIAL_ROLES);
    let mafia_total = role_counts.get(&Role::Mafia).copied().unwrap_or(0) + mafia_special;
    let doctor_total = role_counts.get(&Role::Doctor).copied().unwrap_or(0);
    let police_total = role_counts.get(&Role::Police).copied().unwrap_or(0);
    let agent_total = role_counts.get(&Role::Agent).copied().unwrap_or(0);
    let vigilante_total = role_counts.get(&Role::Vigilante).copied().unwrap_or(0);
    let citizen_special = count_group(role_counts, PUBLIC_CITIZEN_SPECIAL_ROLES);
    let neutral_special = count_group(role_counts, PUBLIC_NEUTRAL_SPECIAL_ROLES);
    let cult_total = count_group(role_counts, PUBLIC_CULT_SPECIAL_ROLES);
    let citizen_text = if let Some(total_players) = total_players {
        let citizen_total = total_players.saturating_sub(
            mafia_total
                + doctor_total
                + police_total
                + agent_total
                + vigilante_total
                + neutral_special
                + cult_total,
        );
        format!("시민 {citizen_total}명(중 특수 {citizen_special}명)")
    } else {
        format!("시민 변동(중 특수 {citizen_special}명)")
    };
    let mut parts = vec![
        format!("마피아 {mafia_total}명(중 특수 {mafia_special}명)"),
        format!("의사 {doctor_total}명"),
        format!("수사직 {}명", police_total + agent_total + vigilante_total),
        citizen_text,
    ];
    if neutral_special > 0 {
        parts.push(format!("중립 특수 {neutral_special}명"));
    }
    if cult_total > 0 {
        parts.push(format!("교주팀 {cult_total}명"));
    }
    parts.join(", ")
}

fn public_role_count_text(game: &MafiaGame) -> String {
    let mut counts = HashMap::new();
    for player in &game.players {
        *counts.entry(player.role).or_default() += 1;
    }
    format!(
        "역할 구성: {}",
        public_role_count_text_from_counts(&counts, Some(game.players.len()))
    )
}

fn public_game_settings_text(game: &MafiaGame, config: &config::BotConfig, prefix: &str) -> String {
    format!(
        "{prefix}\n{}\n최대 참가 인원: {}\n교주팀: {}\n사망 시 직업 공개: {}\n경찰 조사 성공 여부 공개: {}\n아침 생존 마피아 수 공개: {}\n채팅 슬로우모드: {}초\n익명 채팅: {}{}",
        public_role_count_text(game),
        max_player_setting_text(config),
        if config.enable_cult_team {
            "켜짐 - 교주 1명, 광신도 1명 필수 배정"
        } else {
            "꺼짐"
        },
        if config.reveal_death_roles {
            "공개"
        } else {
            "비공개"
        },
        if config.reveal_public_police_status {
            "공개"
        } else {
            "비공개"
        },
        if config.reveal_morning_mafia_count {
            "공개"
        } else {
            "비공개"
        },
        config.chat_slowmode_seconds,
        if config.anonymous_mode {
            "켜짐"
        } else {
            "꺼짐"
        },
        if config.anonymous_mode {
            format!(" ({})", anonymous_name_mode_text(config))
        } else {
            String::new()
        }
    )
}

fn game_rule_text(
    game: &MafiaGame,
    config: &config::BotConfig,
    reveal_death_roles: bool,
) -> String {
    let death_rule = if reveal_death_roles {
        "사망자의 직업은 즉시 공개됩니다."
    } else {
        "사망자의 직업은 즉시 공개되지 않습니다."
    };
    format!(
        "{}\n\n게임은 밤과 낮을 반복합니다.\n- 역할 설명: 전체 역할 설명은 `/역할설명`, 본인 역할 설명은 `/마피아능력`으로 확인할 수 있습니다.\n- 밤: 게임 채널 채팅과 반응이 비활성화되고, 밤 행동이 있는 역할은 DM으로 행동합니다.\n- 낮: 생존자는 자유롭게 토론합니다. 생존자 과반이 `바로 투표`를 누르면 토론을 끝내고 지목 투표로 넘어갑니다. 시간이 끝나면 생존자 과반으로 1분 연장을 정할 수 있고, 연장은 낮마다 1번만 가능합니다.\n- 마피아 수 공개: 아침 생존 마피아 수는 {}.\n- 투표: 생존자는 최후변론에 세울 사람 또는 스킵을 선택합니다. 지목자는 20초 동안 혼자 최후변론을 하고, 이후 찬반투표 과반 결과를 따릅니다.\n- 경찰 공개: 조사 성공 여부는 {}. 실제 조사 결과는 경찰에게만 전달됩니다.\n- 채팅: 낮 토론 슬로우모드는 {}초이며 최후변론 중에는 해제됩니다.\n- 사망자: {death_rule} 게임 채널 채팅/반응 권한은 제거되고 '{DEAD_PLAYER_ROLE}' 역할이 부여됩니다.\n\n승리 조건\n- 시민 진영: 모든 마피아를 제거하면 승리합니다.\n- 마피아 진영: 생존 마피아 수가 나머지 생존자 수 이상이면 승리합니다.\n- 교주팀: 교주팀 생존자가 비교주팀 생존자 이상이면 승리합니다.\n- 조커: 낮 투표로 처형되면 즉시 단독 승리합니다.",
        public_role_count_text(game),
        if config.reveal_morning_mafia_count {
            "공개됩니다"
        } else {
            "공개되지 않습니다"
        },
        if config.reveal_public_police_status {
            "공개됩니다"
        } else {
            "공개되지 않습니다"
        },
        config.chat_slowmode_seconds
    )
}

fn enabled_special_role_names(config: &config::BotConfig) -> String {
    let roles = [
        Role::Detective,
        Role::Shaman,
        Role::Graverobber,
        Role::Spy,
        Role::Contractor,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Madam,
        Role::Godfather,
        Role::Joker,
        Role::Politician,
        Role::Judge,
        Role::Reporter,
        Role::Hacker,
        Role::Terrorist,
        Role::Lover,
        Role::Priest,
        Role::Soldier,
        Role::Nurse,
        Role::Gangster,
        Role::Prophet,
        Role::Psychologist,
        Role::CultLeader,
        Role::Fanatic,
    ]
    .into_iter()
    .filter(|role| match role {
        Role::Detective => config.enable_detective,
        Role::Shaman => config.enable_shaman,
        Role::Graverobber => config.enable_graverobber,
        Role::Spy => config.enable_spy,
        Role::Contractor => config.enable_contractor,
        Role::Thief => config.enable_thief,
        Role::Witch => config.enable_witch,
        Role::Scientist => config.enable_scientist,
        Role::Madam => config.enable_madam,
        Role::Godfather => config.enable_godfather,
        Role::Joker => config.enable_joker,
        Role::Politician => config.enable_politician,
        Role::Judge => config.enable_judge,
        Role::Reporter => config.enable_reporter,
        Role::Hacker => config.enable_hacker,
        Role::Terrorist => config.enable_terrorist,
        Role::Lover => config.enable_lover,
        Role::Priest => config.enable_priest,
        Role::Soldier => config.enable_soldier,
        Role::Nurse => config.enable_nurse,
        Role::Gangster => config.enable_gangster,
        Role::Prophet => config.enable_prophet,
        Role::Psychologist => config.enable_psychologist,
        Role::CultLeader | Role::Fanatic => config.enable_cult_team,
        _ => false,
    })
    .map(|role| role.value())
    .collect::<Vec<_>>();
    if roles.is_empty() {
        "없음".to_string()
    } else {
        roles.join(", ")
    }
}

fn investigation_candidates_text(config: &config::BotConfig) -> String {
    let mut candidates = vec!["경찰"];
    if config.use_agent {
        candidates.push("요원");
    }
    if config.use_vigilante {
        candidates.push("자경단원");
    }
    candidates.join(", ")
}

fn current_settings_text(config: &config::BotConfig, prefix: &str) -> String {
    format!(
        "{prefix}\n게임 상태: {}\n기본 직업: 마피아 {}명, 의사 {}명, 수사직 {}명\n최대 참가 인원: {}\n특수룰 수: 시민 {}개, 마피아 {}개, 중립 {}개\n활성 특수룰: {}\n수사직 후보: {}\n교주팀: {}\n채팅 슬로우모드: {}초\n사망 시 직업 공개: {}\n경찰 조사 성공 여부 공개: {}\n아침 생존 마피아 수 공개: {}\n익명 채팅: {}\n익명 이름 방식: {}",
        if config.game_enabled {
            "활성화"
        } else {
            "비활성화"
        },
        config.default_mafia_count,
        config.default_doctor_count,
        config.default_police_count,
        max_player_setting_text(config),
        config.citizen_special_count,
        config.mafia_special_count,
        config.neutral_special_count,
        enabled_special_role_names(config),
        investigation_candidates_text(config),
        if config.enable_cult_team {
            "켜짐 - 교주 1명, 광신도 1명 필수 배정"
        } else {
            "꺼짐"
        },
        config.chat_slowmode_seconds,
        if config.reveal_death_roles {
            "공개"
        } else {
            "비공개"
        },
        if config.reveal_public_police_status {
            "공개"
        } else {
            "비공개"
        },
        if config.reveal_morning_mafia_count {
            "공개"
        } else {
            "비공개"
        },
        if config.anonymous_mode {
            "켜짐"
        } else {
            "꺼짐"
        },
        anonymous_name_mode_text(config),
    )
}

const RECRUITMENT_STATUS_OPEN: &str = "\u{BAA8}\u{C9D1} \u{C911}\u{C785}\u{B2C8}\u{B2E4}.";
const RECRUITMENT_STATUS_CANCELLED: &str =
    "\u{BAA8}\u{C9D1}\u{C774} \u{CDE8}\u{C18C}\u{B418}\u{C5C8}\u{C2B5}\u{B2C8}\u{B2E4}.";

fn recruitment_embed(
    recruitment: &Recruitment,
    config: &config::BotConfig,
    status: &str,
) -> serenity::CreateEmbed {
    let mut joined = recruitment
        .joined_names
        .values()
        .cloned()
        .collect::<Vec<_>>();
    joined.sort_by_key(|name| name.to_lowercase());
    let joined_text = if joined.is_empty() {
        "아직 참가자가 없습니다.".to_string()
    } else {
        joined
            .iter()
            .enumerate()
            .map(|(idx, name)| format!("{}. {name}", idx + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut spectators = recruitment
        .spectator_names
        .values()
        .cloned()
        .collect::<Vec<_>>();
    spectators.sort_by_key(|name| name.to_lowercase());
    let spectator_text = if spectators.is_empty() {
        "아직 관전자가 없습니다.".to_string()
    } else {
        spectators
            .iter()
            .enumerate()
            .map(|(idx, name)| format!("{}. {name}", idx + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let shortage = recruitment
        .minimum_players
        .saturating_sub(recruitment.joined_ids.len());
    let minimum_text = if shortage == 0 {
        format!("최소 시작 인원 **{}명** 충족", recruitment.minimum_players)
    } else {
        format!(
            "최소 시작 인원 **{}명**까지 **{}명** 더 필요",
            recruitment.minimum_players, shortage
        )
    };
    let remaining = recruitment
        .max_players
        .saturating_sub(recruitment.joined_ids.len());
    make_embed(
        format!(
            "최대 {RECRUITMENT_SECONDS}초 동안 참가자를 모집합니다.\n참가 버튼을 누르면 게임 참가자로 등록되고, '{}' 역할이 부여됩니다.\n관전 버튼을 누르면 '{SPECTATOR_ROLE}' 역할이 부여되고 게임 채널을 읽을 수 있습니다.\n주최자는 `시작` 버튼으로 즉시 시작하거나 `취소` 버튼으로 모집을 취소할 수 있습니다.\n\n역할 구성: {}\n사망 시 직업 공개: {}\n경찰 조사 성공 여부 공개: {}\n아침 생존 마피아 수 공개: {}\n{}\n\n최대 참가 인원 **{}명**까지 **{}명** 더 참가 가능\n\n현재 참가자 **{}/{}명**\n{}\n\n현재 관전자 **{}명**\n{}\n\n{}",
            config.participant_role,
            public_role_count_text_from_counts(&recruitment.role_counts, None),
            if config.reveal_death_roles {
                "공개"
            } else {
                "비공개"
            },
            if config.reveal_public_police_status {
                "공개"
            } else {
                "비공개"
            },
            if config.reveal_morning_mafia_count {
                "공개"
            } else {
                "비공개"
            },
            minimum_text,
            recruitment.max_players,
            remaining,
            recruitment.joined_ids.len(),
            recruitment.max_players,
            joined_text,
            recruitment.spectator_ids.len(),
            spectator_text,
            status
        ),
        "참가자 모집",
        serenity::Colour::DARK_GREEN,
    )
}

fn recruitment_components(
    guild_id: serenity::GuildId,
    disabled: bool,
) -> Vec<serenity::CreateActionRow> {
    let guild_key = guild_id.get();
    vec![serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(format!("join:{guild_key}"))
            .label("참가")
            .style(serenity::ButtonStyle::Success)
            .disabled(disabled),
        serenity::CreateButton::new(format!("spectate:{guild_key}"))
            .label("관전")
            .style(serenity::ButtonStyle::Secondary)
            .disabled(disabled),
        serenity::CreateButton::new(format!("startnow:{guild_key}"))
            .label("시작")
            .style(serenity::ButtonStyle::Primary)
            .disabled(disabled),
        serenity::CreateButton::new(format!("cancelrec:{guild_key}"))
            .label("취소")
            .style(serenity::ButtonStyle::Danger)
            .disabled(disabled),
    ])]
}

async fn update_recruitment_message(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    recruitment: &Recruitment,
    status: &str,
    disabled: bool,
) {
    let config = data.config.read().await.clone();
    if let Err(error) = component
        .channel_id
        .edit_message(
            &ctx.http,
            component.message.id,
            serenity::EditMessage::new()
                .embed(recruitment_embed(recruitment, &config, status))
                .components(recruitment_components(guild_id, disabled)),
        )
        .await
    {
        eprintln!("failed to update recruitment message: {error:?}");
    }
}

#[poise::command(
    slash_command,
    rename = "마피아시작",
    description_localized("ko", "저장된 설정대로 마피아 게임 참가자를 모집하고 시작합니다.")
)]
async fn start_game(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버 채널에서만 사용할 수 있습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let channel_id = ctx.channel_id();
    if ctx.data().games.contains_key(&guild_id) {
        reply_embed(
            ctx,
            "이미 진행 중인 게임이 있습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    if ctx.data().recruitments.contains_key(&guild_id) {
        reply_embed(
            ctx,
            "이미 참가자를 모집 중입니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let config_snapshot = ctx.data().config.read().await.clone();
    if !config_snapshot.game_enabled {
        reply_embed(
            ctx,
            "마피아 게임이 비활성화되어 있습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let Some(participant_role) = role_by_name(
        ctx.serenity_context(),
        guild_id,
        &config_snapshot.participant_role,
    )
    .await?
    else {
        reply_embed(
            ctx,
            format!(
                "'{}' 역할을 찾을 수 없습니다.",
                config_snapshot.participant_role
            ),
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };

    let special_roles = choose_special_roles(&config_snapshot)?;
    let mut role_counts = selected_role_counts(&config_snapshot, &special_roles)?;
    let minimum_players = minimum_player_count(&role_counts);
    let max_players = effective_max_player_count(&config_snapshot);
    if max_players < minimum_players {
        reply_embed(
            ctx,
            format!("현재 설정의 최소 시작 인원은 {minimum_players}명이라 최대 인원 {max_players}명으로 시작할 수 없습니다."),
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let done = Arc::new(Notify::new());
    let recruitment = Arc::new(RwLock::new(Recruitment {
        host_user_id: ctx.author().id,
        participant_role_id: participant_role.id,
        role_counts: role_counts.clone(),
        special_roles: special_roles.clone(),
        max_players,
        minimum_players,
        joined_ids: HashSet::new(),
        joined_names: HashMap::new(),
        spectator_ids: HashSet::new(),
        spectator_names: HashMap::new(),
        accepting: true,
        cancelled: false,
        done: done.clone(),
    }));
    ctx.data()
        .recruitments
        .insert(guild_id, recruitment.clone());

    let mention = role_by_name(ctx.serenity_context(), guild_id, GAME_NOTIFICATION_ROLE)
        .await?
        .map(|role| role.mention().to_string());
    let rec = recruitment.read().await;
    let mut reply = poise::CreateReply::default()
        .embed(recruitment_embed(&rec, &config_snapshot, "모집 중입니다."))
        .components(recruitment_components(guild_id, false));
    if let Some(mention) = mention {
        reply = reply.content(mention);
    }
    drop(rec);
    ctx.send(reply).await?;

    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(RECRUITMENT_SECONDS)) => {}
        _ = done.notified() => {}
    }

    let mut rec = recruitment.write().await;
    rec.accepting = false;
    let cancelled = rec.cancelled || rec.joined_ids.len() < rec.minimum_players;
    rec.cancelled = cancelled;
    let player_data = rec
        .joined_ids
        .iter()
        .map(|id| {
            (
                *id,
                rec.joined_names
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| id.to_string()),
            )
        })
        .collect::<Vec<_>>();
    if cancelled {
        ctx.data().recruitments.remove(&guild_id);
        reply_embed(
            ctx,
            "참가자 모집이 취소되었습니다.",
            "참가자 모집 취소",
            serenity::Colour::RED,
            false,
        )
        .await?;
        return Ok(());
    }
    let mut game_special_roles = expand_special_roles(&rec.special_roles);
    if config_snapshot.enable_cult_team {
        game_special_roles.extend([Role::CultLeader, Role::Fanatic]);
        *role_counts.entry(Role::CultLeader).or_default() += 1;
        *role_counts.entry(Role::Fanatic).or_default() += 1;
    }
    let participant_user_ids = rec.joined_ids.clone();
    let spectator_user_ids = rec.spectator_ids.clone();
    drop(rec);
    ctx.data().recruitments.remove(&guild_id);

    let game = MafiaGame::new_with_counts(
        player_data,
        GameCounts {
            mafia_count: *role_counts.get(&Role::Mafia).unwrap_or(&0),
            doctor_count: *role_counts.get(&Role::Doctor).unwrap_or(&0),
            police_count: *role_counts.get(&Role::Police).unwrap_or(&0),
            agent_count: *role_counts.get(&Role::Agent).unwrap_or(&0),
            vigilante_count: *role_counts.get(&Role::Vigilante).unwrap_or(&0),
            joker_count: 0,
            special_roles: game_special_roles,
        },
    )?;
    let initial_roles = game.players.iter().map(|p| (p.user_id, p.role)).collect();
    let running = Arc::new(RwLock::new(RunningGame {
        guild_id,
        channel_id,
        participant_user_ids,
        spectator_user_ids,
        reveal_death_roles: config_snapshot.reveal_death_roles,
        anonymous_enabled: config_snapshot.anonymous_mode,
        game,
        started_at: Instant::now(),
        initial_roles,
        memos: HashMap::new(),
        game_status_message_id: None,
        game_status_text: None,
        anonymous_aliases: HashMap::new(),
        anonymous_original_names: HashMap::new(),
        anonymous_input_channel_ids: HashMap::new(),
        anonymous_input_channel_owners: HashMap::new(),
        anonymous_dead_input_channel_ids: HashMap::new(),
        anonymous_dead_input_channel_owners: HashMap::new(),
        anonymous_shaman_input_channel_ids: HashMap::new(),
        anonymous_shaman_input_channel_owners: HashMap::new(),
        anonymous_role_input_channel_ids: HashMap::new(),
        anonymous_role_input_channels: HashMap::new(),
        anonymous_role_input_status_message_ids: HashMap::new(),
        anonymous_role_status_texts: HashMap::new(),
        anonymous_channel_topics: HashMap::new(),
        anonymous_webhook_urls: HashMap::new(),
        original_game_channel_overwrites: HashMap::new(),
        game_channel_overwrites: HashMap::new(),
        member_channel_overwrites: HashMap::new(),
        original_slowmode_delays: HashMap::new(),
        private_channel_ids: HashMap::new(),
        private_role_status_message_ids: HashMap::new(),
        private_role_status_texts: HashMap::new(),
        memo_channel_ids: HashMap::new(),
        shaman_channel_id: None,
        shaman_status_message_id: None,
        shaman_status_text: None,
        frog_channel_id: None,
        frog_game_channel_overwrites: HashMap::new(),
        madam_seduction_channel_overwrites: HashMap::new(),
        day_chat_open: false,
        final_defense_user_id: None,
        day_skip_voter_ids: HashSet::new(),
        day_skip_confirmed: false,
        day_extension_voter_ids: HashSet::new(),
        day_extension_active: false,
        day_extension_confirmed: false,
        night_timed_events_due: false,
        contractor_contract_drafts: HashMap::new(),
        night_notify: Arc::new(Notify::new()),
        vote_notify: Arc::new(Notify::new()),
        confirm_notify: Arc::new(Notify::new()),
        day_notify: Arc::new(Notify::new()),
        stats_recorded: false,
    }));
    ctx.data().games.insert(guild_id, running.clone());
    let data = ctx.data().clone();
    let serenity_ctx = ctx.serenity_context().clone();
    tokio::spawn(async move {
        if let Err(error) = game_loop(serenity_ctx, data, running).await {
            eprintln!("Rust game loop error: {error:?}");
        }
    });

    let running = ctx.data().games.get(&guild_id).unwrap();
    let game = &running.read().await.game;
    reply_embed(
        ctx,
        format!(
            "게임을 시작합니다. 참가자 {}명에게 역할을 DM으로 보냅니다.\n{}",
            game.players.len(),
            public_role_count_text(game)
        ),
        "게임 시작",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

async fn setup_game_channels(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let config = data.config.read().await.clone();
    let (guild_id, channel_id) = {
        let running_read = running.read().await;
        (running_read.guild_id, running_read.channel_id)
    };
    let roles = channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await?;
    let category = source_category(ctx, channel_id).await;

    set_spectator_game_channel_access(ctx, running, roles).await;
    create_anonymous_chat_channels(ctx, running, &config, roles, category).await?;
    hide_original_game_channel_for_anonymous(ctx, running, roles).await;
    create_private_role_channels(ctx, running, roles, category).await?;
    sync_cult_team_channel_access(ctx, data, running).await;
    create_memo_channels(ctx, running, roles, category).await?;
    create_shaman_chat_channel(ctx, running, roles, category).await?;
    create_frog_chat_channel(ctx, running, roles, category).await?;
    Ok(())
}

async fn hide_original_game_channel_for_anonymous(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
) {
    let (anonymous_enabled, channel_id) = {
        let running_read = running.read().await;
        (running_read.anonymous_enabled, running_read.channel_id)
    };
    if !anonymous_enabled {
        return;
    }
    let Some(participant_role_id) = roles.participant else {
        return;
    };
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let original = channel
        .permission_overwrites
        .iter()
        .find(|overwrite| {
            overwrite.kind == serenity::PermissionOverwriteType::Role(participant_role_id)
        })
        .cloned();
    {
        let mut running_write = running.write().await;
        running_write
            .original_game_channel_overwrites
            .entry(participant_role_id)
            .or_insert(original);
    }
    let _ = channel_id
        .create_permission(
            &ctx.http,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Role(participant_role_id),
                false,
                false,
            ),
        )
        .await;
    let _ = channel_id
        .create_permission(
            &ctx.http,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(roles.bot),
                true,
                true,
            ),
        )
        .await;
}

async fn set_spectator_game_channel_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
) {
    let Some(spectator_role_id) = roles.spectator else {
        return;
    };
    let channel_id = running.read().await.channel_id;
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let kind = serenity::PermissionOverwriteType::Role(spectator_role_id);
    let original = channel
        .permission_overwrites
        .iter()
        .find(|overwrite| overwrite.kind == kind)
        .cloned();
    {
        let mut running_write = running.write().await;
        running_write
            .game_channel_overwrites
            .entry(spectator_role_id)
            .or_insert_with(|| original.clone());
    }
    let _ = channel_id
        .create_permission(&ctx.http, spectator_channel_overwrite(spectator_role_id))
        .await;
}

async fn create_anonymous_chat_channels(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    config: &config::BotConfig,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<()> {
    {
        let mut running_write = running.write().await;
        if !running_write.anonymous_enabled {
            return Ok(());
        }
        assign_anonymous_aliases(&mut running_write, config);
        apply_anonymous_player_names(&mut running_write);
    }

    let players = { running.read().await.game.players.clone() };
    for player in players {
        let (guild_id, alias, can_chat) = {
            let running_read = running.read().await;
            let Some(player_state) = running_read.game.get_player(player.user_id) else {
                continue;
            };
            (
                running_read.guild_id,
                running_read
                    .anonymous_aliases
                    .get(&player.user_id)
                    .cloned()
                    .unwrap_or_else(|| player.name.clone()),
                can_use_anonymous_general_chat(&running_read, player_state),
            )
        };
        if guild_id
            .member(ctx, serenity::UserId::new(player.user_id))
            .await
            .is_err()
        {
            continue;
        }

        let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
        overwrites.push(anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
            true,
            can_chat,
        ));
        let Some(input_channel) = create_text_channel_safe(
            ctx,
            guild_id,
            &format!("{}-채팅", sanitize_channel_part(&alias)),
            overwrites,
            category,
            "마피아 게임 개인 익명 입력 채널 생성",
            config.chat_slowmode_seconds,
            None,
        )
        .await
        else {
            continue;
        };
        {
            let mut running_write = running.write().await;
            running_write
                .anonymous_input_channel_ids
                .insert(player.user_id, input_channel.id);
            running_write
                .anonymous_input_channel_owners
                .insert(input_channel.id, player.user_id);
        }
        let _ = send_channel_embed(
            &ctx.http,
            input_channel.id,
            format!(
                "당신의 익명 이름은 **{alias}** 입니다.\n이 개인 채널이 일반 채팅을 대체합니다.\n여기에 쓰면 모든 참가자의 개인 채팅방에 익명으로 전달됩니다."
            ),
            "익명 입력 채널",
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await;
    }
    Ok(())
}

fn role_channel_status_text(running: &RunningGame, role: Role) -> String {
    let mut players = role_status_player_ids(running, role)
        .into_iter()
        .filter_map(|user_id| running.game.get_player(user_id))
        .collect::<Vec<_>>();
    players.sort_by_key(|player| status_display_name(running, player).to_lowercase());
    let mut text = if players.is_empty() {
        "현재 생존: 없음".to_string()
    } else {
        format!(
            "현재 생존: {}",
            players
                .into_iter()
                .map(|player| status_display_name(running, player))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    if role == Role::Mafia {
        let mafia_status = mafia_night_target_status_text(running);
        if !mafia_status.is_empty() {
            text = format!("{text}\n\n{mafia_status}");
        }
    }
    text
}

fn status_player_list<'a>(
    running: &RunningGame,
    players: impl IntoIterator<Item = &'a Player>,
) -> String {
    let mut names = players
        .into_iter()
        .map(|player| status_display_name(running, player))
        .collect::<Vec<_>>();
    if names.is_empty() {
        return "없음".to_string();
    }
    names.sort_by_key(|name| name.to_lowercase());
    let shown = names.iter().take(40).cloned().collect::<Vec<_>>();
    let suffix = if names.len() > shown.len() {
        format!(" 외 {}명", names.len() - shown.len())
    } else {
        String::new()
    };
    format!("{}{suffix}", shown.join(", "))
}

fn game_status_text(running: &RunningGame) -> String {
    let alive = running.game.alive_players();
    let dead = running.game.dead_players();
    format!(
        "{}일차 / 현재 단계: {}\n생존자 **{}명** / 사망자 **{}명**\n\n생존자 목록\n{}\n\n사망자 목록\n{}",
        running.game.day_number,
        running.game.phase.value(),
        alive.len(),
        dead.len(),
        status_player_list(running, alive.iter().copied()),
        status_player_list(running, dead.iter().copied())
    )
}

async fn upsert_game_status(ctx: &serenity::Context, running: &Arc<RwLock<RunningGame>>) {
    let (channel_id, message_id, status_text, unchanged) = {
        let running_read = running.read().await;
        let status_text = game_status_text(&running_read);
        let unchanged = running_read
            .game_status_text
            .as_ref()
            .is_some_and(|cached| cached == &status_text);
        (
            running_read.channel_id,
            running_read.game_status_message_id,
            status_text,
            unchanged,
        )
    };
    if unchanged {
        return;
    }
    if let Some(message_id) = message_id {
        let edit_result = channel_id
            .edit_message(
                &ctx.http,
                message_id,
                serenity::EditMessage::new().embed(make_embed(
                    status_text.clone(),
                    "게임 현황",
                    serenity::Colour::DARK_GREEN,
                )),
            )
            .await;
        if edit_result.is_ok() {
            running.write().await.game_status_text = Some(status_text);
            return;
        }
    }
    if let Ok(message) = send_channel_embed(
        &ctx.http,
        channel_id,
        status_text.clone(),
        "게임 현황",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await
    {
        let mut running_write = running.write().await;
        running_write.game_status_message_id = Some(message.id);
        running_write.game_status_text = Some(status_text);
    }
}

fn final_team_text(game: &MafiaGame, player: &Player) -> &'static str {
    if game.is_cult_team(player) {
        "교주팀"
    } else if game.is_mafia_team(player) {
        "마피아팀"
    } else if player.role == Role::Joker {
        "중립"
    } else {
        "시민팀"
    }
}

fn final_role_reveal_text(running: &RunningGame) -> String {
    let role_detail = |player: &Player| {
        let state = if player.alive { "" } else { " (사망)" };
        format!(
            "{}{} / 최종 진영: {}",
            player.role.value(),
            state,
            final_team_text(&running.game, player)
        )
    };
    let mut players = running.game.players.clone();
    if running.anonymous_enabled {
        players.sort_by_key(|player| {
            running
                .anonymous_aliases
                .get(&player.user_id)
                .unwrap_or(&player.name)
                .to_lowercase()
        });
        players
            .iter()
            .map(|player| {
                let alias = running
                    .anonymous_aliases
                    .get(&player.user_id)
                    .map(String::as_str)
                    .unwrap_or("익명");
                let real_name = running
                    .anonymous_original_names
                    .get(&player.user_id)
                    .map(String::as_str)
                    .unwrap_or(&player.name);
                format!("- {alias} = {real_name}: {}", role_detail(player))
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        players.sort_by_key(|player| player.name.to_lowercase());
        players
            .iter()
            .map(|player| format!("- {}: {}", player.name, role_detail(player)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn private_role_status_player_ids(running: &RunningGame, player: &Player) -> (String, Vec<u64>) {
    if running.game.is_cult_team(player) {
        return (
            "내 교주팀".to_string(),
            running
                .game
                .players
                .iter()
                .filter(|target| running.game.is_cult_team(target))
                .map(|target| target.user_id)
                .collect(),
        );
    }
    if running.game.is_known_mafia_team(player) {
        return (
            "내 마피아팀".to_string(),
            running
                .game
                .players
                .iter()
                .filter(|target| running.game.is_known_mafia_team(target))
                .map(|target| target.user_id)
                .collect(),
        );
    }
    (
        format!("내 역할({})", player.role.value()),
        running
            .game
            .players
            .iter()
            .filter(|target| target.role == player.role)
            .map(|target| target.user_id)
            .collect(),
    )
}

fn command_status_text(running: &RunningGame, requester_id: u64) -> String {
    let message = game_status_text(running);
    let Some(player) = running.game.get_player(requester_id) else {
        return message;
    };
    if !running.anonymous_enabled {
        return message;
    }
    let (label, same_group_ids) = private_role_status_player_ids(running, player);
    let same_group = same_group_ids
        .into_iter()
        .filter_map(|user_id| running.game.get_player(user_id))
        .collect::<Vec<_>>();
    let alive = same_group
        .iter()
        .copied()
        .filter(|target| target.alive)
        .collect::<Vec<_>>();
    let dead = same_group
        .iter()
        .copied()
        .filter(|target| !target.alive)
        .collect::<Vec<_>>();
    format!(
        "{message}\n\n{label} 현황\n생존 **{}명** / 사망 **{}명**\n생존: {}\n사망: {}",
        alive.len(),
        dead.len(),
        status_player_list(running, alive),
        status_player_list(running, dead)
    )
}

async fn create_anonymous_role_channels(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<Vec<Role>> {
    let mut failed_roles = Vec::new();
    for &role in PRIVATE_CHAT_ROLES {
        let (guild_id, should_create, player_ids, status_text) = {
            let running_read = running.read().await;
            (
                running_read.guild_id,
                should_create_private_role_channel(&running_read.game, role),
                role_chat_player_ids(&running_read.game, role),
                role_channel_status_text(&running_read, role),
            )
        };
        if !should_create {
            continue;
        }
        let mut created_for_role = false;
        for user_id in player_ids {
            let (alias, can_chat) = {
                let running_read = running.read().await;
                let Some(player) = running_read.game.get_player(user_id) else {
                    continue;
                };
                (
                    running_read
                        .anonymous_aliases
                        .get(&user_id)
                        .cloned()
                        .unwrap_or_else(|| player.name.clone()),
                    can_use_anonymous_role_chat(&running_read, player, role),
                )
            };
            if guild_id
                .member(ctx, serenity::UserId::new(user_id))
                .await
                .is_err()
            {
                continue;
            }
            let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
            overwrites.push(anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id)),
                true,
                can_chat,
            ));
            let topic = format!("{} 익명 채팅 | {status_text}", role.value());
            let Some(channel) = create_text_channel_safe(
                ctx,
                guild_id,
                &format!("{}-{}-채팅", sanitize_channel_part(&alias), role.value()),
                overwrites,
                category,
                "마피아 게임 역할별 익명 입력 채널 생성",
                0,
                Some(topic.clone()),
            )
            .await
            else {
                continue;
            };
            {
                let mut running_write = running.write().await;
                running_write
                    .anonymous_role_input_channel_ids
                    .insert((user_id, role), channel.id);
                running_write
                    .anonymous_role_input_channels
                    .insert(channel.id, (user_id, role));
                running_write
                    .anonymous_channel_topics
                    .insert(channel.id, topic.chars().take(1024).collect::<String>());
            }
            let _ = send_channel_embed(
                &ctx.http,
                channel.id,
                format!(
                    "{} 전용 익명 입력 채널입니다.\n이곳에 쓰면 같은 {} 채팅 참가자에게 익명으로 전달됩니다.\n\n{}",
                    role.value(),
                    role.value(),
                    special_role_rule_text(role)
                ),
                "역할 익명 채널",
                serenity::Colour::DARK_GREEN,
                vec![],
            )
            .await;
            if let Ok(message) = send_channel_embed(
                &ctx.http,
                channel.id,
                status_text.clone(),
                &format!("{} 채팅 현황", role.value()),
                serenity::Colour::DARK_GREEN,
                vec![],
            )
            .await
            {
                let mut running_write = running.write().await;
                running_write
                    .anonymous_role_input_status_message_ids
                    .insert((user_id, role), message.id);
                running_write
                    .anonymous_role_status_texts
                    .insert((user_id, role), status_text.clone());
            }
            created_for_role = true;
        }
        if !created_for_role && should_create {
            failed_roles.push(role);
        }
    }
    Ok(failed_roles)
}

async fn create_private_role_channels(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<()> {
    if running.read().await.anonymous_enabled {
        let failed_roles = create_anonymous_role_channels(ctx, running, roles, category).await?;
        if !failed_roles.is_empty() {
            let channel_id = running.read().await.channel_id;
            let _ = send_channel_embed(
                &ctx.http,
                channel_id,
                format!(
                    "익명 역할 개인 채팅방 생성 실패: {}",
                    failed_roles
                        .into_iter()
                        .map(|role| role.value())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                "마피아 게임",
                serenity::Colour::RED,
                vec![],
            )
            .await;
        }
        return Ok(());
    }

    let mut failed_roles = Vec::new();
    for &role in PRIVATE_CHAT_ROLES {
        let (guild_id, should_create, players, status_text) = {
            let running_read = running.read().await;
            (
                running_read.guild_id,
                should_create_private_role_channel(&running_read.game, role),
                running_read
                    .game
                    .players
                    .iter()
                    .filter(|player| player.role == role)
                    .cloned()
                    .collect::<Vec<_>>(),
                role_channel_status_text(&running_read, role),
            )
        };
        if !should_create {
            continue;
        }

        let mut overwrites = Vec::new();
        add_common_hidden_overwrites(&mut overwrites, roles, true);
        for player in players {
            if guild_id
                .member(ctx, serenity::UserId::new(player.user_id))
                .await
                .is_err()
            {
                continue;
            }
            let can_open = role != Role::Lover || {
                let running_read = running.read().await;
                lover_chat_is_open(&running_read.game)
            };
            overwrites.push(private_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_open,
            ));
        }

        let Some(private_channel) = create_text_channel_safe(
            ctx,
            guild_id,
            private_channel_name(role),
            overwrites,
            category,
            "마피아 게임 역할별 비공개 채팅방 생성",
            0,
            None,
        )
        .await
        else {
            failed_roles.push(role);
            continue;
        };
        running
            .write()
            .await
            .private_channel_ids
            .insert(role, private_channel.id);
        let _ = send_channel_embed(
            &ctx.http,
            private_channel.id,
            format!(
                "{} 전용 비공개 채팅방입니다. 살아있는 {}만 볼 수 있습니다.\n\n{}",
                role.value(),
                role.value(),
                special_role_rule_text(role)
            ),
            "역할 비공개 채널",
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await;
        if let Ok(message) = send_channel_embed(
            &ctx.http,
            private_channel.id,
            status_text.clone(),
            &format!("{} 채팅 현황", role.value()),
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await
        {
            let mut running_write = running.write().await;
            running_write
                .private_role_status_message_ids
                .insert(role, message.id);
            running_write
                .private_role_status_texts
                .insert(role, status_text);
        }
    }

    if !failed_roles.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "역할별 비공개 채널 생성에 실패했습니다: {}\n봇에게 채널 관리 권한이 있는지 확인하세요.",
                failed_roles
                    .into_iter()
                    .map(|role| role.value())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    Ok(())
}

async fn upsert_private_role_status_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    role: Role,
) {
    let (channel_id, message_id, status_text, unchanged) = {
        let running_read = running.read().await;
        let Some(channel_id) = running_read.private_channel_ids.get(&role).copied() else {
            return;
        };
        let status_text = role_channel_status_text(&running_read, role);
        let unchanged = running_read
            .private_role_status_texts
            .get(&role)
            .is_some_and(|cached| cached == &status_text);
        (
            channel_id,
            running_read
                .private_role_status_message_ids
                .get(&role)
                .copied(),
            status_text,
            unchanged,
        )
    };
    if unchanged {
        return;
    }
    let title = format!("{} 채팅 현황", role.value());
    if let Some(message_id) = message_id {
        let edit_result = channel_id
            .edit_message(
                &ctx.http,
                message_id,
                serenity::EditMessage::new().embed(make_embed(
                    status_text.clone(),
                    &title,
                    serenity::Colour::DARK_GREEN,
                )),
            )
            .await;
        if edit_result.is_ok() {
            running
                .write()
                .await
                .private_role_status_texts
                .insert(role, status_text);
            return;
        }
    }
    if let Ok(message) = send_channel_embed(
        &ctx.http,
        channel_id,
        status_text.clone(),
        &title,
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await
    {
        let mut running_write = running.write().await;
        running_write
            .private_role_status_message_ids
            .insert(role, message.id);
        running_write
            .private_role_status_texts
            .insert(role, status_text);
    }
}

async fn upsert_anonymous_role_status_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    role: Role,
    key: (u64, Role),
) {
    let (message_id, status_text, unchanged) = {
        let running_read = running.read().await;
        let status_text = role_channel_status_text(&running_read, role);
        let unchanged = running_read
            .anonymous_role_status_texts
            .get(&key)
            .is_some_and(|cached| cached == &status_text);
        (
            running_read
                .anonymous_role_input_status_message_ids
                .get(&key)
                .copied(),
            status_text,
            unchanged,
        )
    };
    if unchanged {
        return;
    }
    let title = format!("{} 채팅 현황", role.value());
    if let Some(message_id) = message_id {
        let edit_result = channel_id
            .edit_message(
                &ctx.http,
                message_id,
                serenity::EditMessage::new().embed(make_embed(
                    status_text.clone(),
                    &title,
                    serenity::Colour::DARK_GREEN,
                )),
            )
            .await;
        if edit_result.is_ok() {
            running
                .write()
                .await
                .anonymous_role_status_texts
                .insert(key, status_text);
            return;
        }
    }
    if let Ok(message) = send_channel_embed(
        &ctx.http,
        channel_id,
        status_text.clone(),
        &title,
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await
    {
        let mut running_write = running.write().await;
        running_write
            .anonymous_role_input_status_message_ids
            .insert(key, message.id);
        running_write
            .anonymous_role_status_texts
            .insert(key, status_text);
    }
}

async fn sync_anonymous_role_statuses(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    update_messages: bool,
) {
    let updates = {
        let running_read = running.read().await;
        if !running_read.anonymous_enabled {
            return;
        }
        let mut updates = Vec::new();
        for &role in PRIVATE_CHAT_ROLES {
            if !should_create_private_role_channel(&running_read.game, role) {
                continue;
            }
            let topic = format!(
                "{} 익명 채팅 | {}",
                role.value(),
                role_channel_status_text(&running_read, role)
            )
            .chars()
            .take(1024)
            .collect::<String>();
            for (&(user_id, input_role), &channel_id) in
                &running_read.anonymous_role_input_channel_ids
            {
                if input_role == role {
                    updates.push((user_id, role, channel_id, topic.clone()));
                }
            }
        }
        updates
    };
    for (user_id, role, channel_id, topic) in updates {
        let needs_topic_update = {
            let running_read = running.read().await;
            running_read.anonymous_channel_topics.get(&channel_id) != Some(&topic)
        };
        if needs_topic_update
            && channel_id
                .edit(&ctx.http, serenity::EditChannel::new().topic(topic.clone()))
                .await
                .is_ok()
        {
            running
                .write()
                .await
                .anonymous_channel_topics
                .insert(channel_id, topic);
        }
        if update_messages {
            upsert_anonymous_role_status_message(ctx, running, channel_id, role, (user_id, role))
                .await;
        }
    }
}

fn shaman_chat_status_text(running: &RunningGame) -> &'static str {
    if running.anonymous_enabled {
        "사망자와 영매가 접신하는 채팅입니다.\n영매는 이 채널만 볼 수 있으며, 밤에만 말할 수 있습니다.\n익명 모드에서는 각자의 영매 개인 채널을 사용하세요."
    } else {
        "사망자와 영매가 접신하는 채팅입니다.\n영매는 이 채널만 볼 수 있으며, 밤에만 말할 수 있습니다."
    }
}

async fn upsert_shaman_chat_status(ctx: &serenity::Context, running: &Arc<RwLock<RunningGame>>) {
    let (channel_id, message_id, status_text, unchanged) = {
        let running_read = running.read().await;
        let Some(channel_id) = running_read.shaman_channel_id else {
            return;
        };
        let status_text = shaman_chat_status_text(&running_read).to_string();
        let unchanged = running_read
            .shaman_status_text
            .as_ref()
            .is_some_and(|cached| cached == &status_text);
        (
            channel_id,
            running_read.shaman_status_message_id,
            status_text,
            unchanged,
        )
    };
    if unchanged {
        return;
    }
    if let Some(message_id) = message_id {
        let edit_result = channel_id
            .edit_message(
                &ctx.http,
                message_id,
                serenity::EditMessage::new().embed(make_embed(
                    status_text.clone(),
                    "영매 채팅 상태",
                    serenity::Colour::DARK_GREEN,
                )),
            )
            .await;
        if edit_result.is_ok() {
            running.write().await.shaman_status_text = Some(status_text);
            return;
        }
    }
    if let Ok(message) = send_channel_embed(
        &ctx.http,
        channel_id,
        status_text.clone(),
        "영매 채팅 상태",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await
    {
        let mut running_write = running.write().await;
        running_write.shaman_status_message_id = Some(message.id);
        running_write.shaman_status_text = Some(status_text);
    }
}

async fn ensure_memo_channel(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Option<serenity::ChannelId> {
    if let Some(channel_id) = running
        .read()
        .await
        .memo_channel_ids
        .get(&player.user_id)
        .copied()
    {
        return Some(channel_id);
    }
    let (guild_id, display_name) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            status_display_name(&running_read, player),
        )
    };
    if guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
        .is_err()
    {
        return None;
    }
    let mut overwrites = Vec::new();
    add_common_hidden_overwrites(&mut overwrites, roles, true);
    overwrites.push(private_channel_overwrite(
        serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
        true,
    ));
    let channel = create_text_channel_safe(
        ctx,
        guild_id,
        &format!("{}-메모", sanitize_channel_part(&display_name)),
        overwrites,
        category,
        "마피아 게임 개인 메모 채널 생성",
        0,
        None,
    )
    .await?;
    running
        .write()
        .await
        .memo_channel_ids
        .insert(player.user_id, channel.id);
    let _ = send_channel_embed(
        &ctx.http,
        channel.id,
        "개인 메모 채널입니다.\n`/메모 참가자 메모내용`으로 참가자별 메모를 저장하고, `/메모 참가자`로 저장한 메모를 다시 볼 수 있습니다.",
        "메모 채널",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
    Some(channel.id)
}

async fn ensure_anonymous_dead_input_channel(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
    can_chat: bool,
) -> Option<serenity::ChannelId> {
    if !running.read().await.anonymous_enabled {
        return None;
    }
    let (guild_id, alias, existing_channel_id) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read
                .anonymous_aliases
                .get(&player.user_id)
                .cloned()
                .unwrap_or_else(|| player.name.clone()),
            running_read
                .anonymous_dead_input_channel_ids
                .get(&player.user_id)
                .copied(),
        )
    };
    if guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
        .is_err()
    {
        return None;
    }
    if let Some(channel_id) = existing_channel_id {
        let _ = channel_id
            .create_permission(
                &ctx.http,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(
                        player.user_id,
                    )),
                    true,
                    can_chat,
                ),
            )
            .await;
        return Some(channel_id);
    }

    let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
    overwrites.push(anonymous_input_overwrite(
        serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
        true,
        can_chat,
    ));
    let channel = create_text_channel_safe(
        ctx,
        guild_id,
        &format!("{}-사망자-채팅", sanitize_channel_part(&alias)),
        overwrites,
        category,
        "마피아 게임 사망자 개인 채팅 채널 생성",
        0,
        None,
    )
    .await?;
    {
        let mut running_write = running.write().await;
        running_write
            .anonymous_dead_input_channel_ids
            .insert(player.user_id, channel.id);
        running_write
            .anonymous_dead_input_channel_owners
            .insert(channel.id, player.user_id);
    }
    let _ = send_channel_embed(
        &ctx.http,
        channel.id,
        "사망자 개인 채팅 채널입니다.\n여기에 쓰면 사망자 채팅을 볼 수 있는 사람들의 사망자 개인 채널로만 전달됩니다.",
        "사망자 개인 채팅",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
    Some(channel.id)
}

async fn ensure_anonymous_shaman_input_channel(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
    can_chat: bool,
) -> Option<serenity::ChannelId> {
    if !running.read().await.anonymous_enabled {
        return None;
    }
    let (guild_id, alias, existing_channel_id) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read
                .anonymous_aliases
                .get(&player.user_id)
                .cloned()
                .unwrap_or_else(|| player.user_id.to_string()),
            running_read
                .anonymous_shaman_input_channel_ids
                .get(&player.user_id)
                .copied(),
        )
    };
    if guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
        .is_err()
    {
        return None;
    }
    if let Some(channel_id) = existing_channel_id {
        let _ = channel_id
            .create_permission(
                &ctx.http,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(
                        player.user_id,
                    )),
                    true,
                    can_chat,
                ),
            )
            .await;
        return Some(channel_id);
    }

    let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
    overwrites.push(anonymous_input_overwrite(
        serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
        true,
        can_chat,
    ));
    let channel = create_text_channel_safe(
        ctx,
        guild_id,
        &format!("{}-영매-채팅", sanitize_channel_part(&alias)),
        overwrites,
        category,
        "마피아 게임 익명 영매 입력 채널 생성",
        0,
        None,
    )
    .await?;
    {
        let mut running_write = running.write().await;
        running_write
            .anonymous_shaman_input_channel_ids
            .insert(player.user_id, channel.id);
        running_write
            .anonymous_shaman_input_channel_owners
            .insert(channel.id, player.user_id);
    }
    let _ = send_channel_embed(
        &ctx.http,
        channel.id,
        "영매 익명 채팅 개인 채널입니다.\n여기에 쓰면 영매 채팅을 볼 수 있는 사람들의 영매 개인 채널로만 전달됩니다.",
        "익명 영매 채팅",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
    Some(channel.id)
}

async fn create_memo_channels(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<()> {
    let players = { running.read().await.game.players.clone() };
    let mut failed_names = Vec::new();
    for player in players {
        if ensure_memo_channel(ctx, running, &player, roles, category)
            .await
            .is_none()
        {
            let running_read = running.read().await;
            failed_names.push(status_display_name(&running_read, &player));
        }
    }
    if !failed_names.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!("개인 메모 채널 생성 실패: {}", failed_names.join(", ")),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    Ok(())
}

async fn create_shaman_chat_channel(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<()> {
    let (guild_id, has_shaman, anonymous_enabled, shamans) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read
                .game
                .players
                .iter()
                .any(|player| player.role == Role::Shaman),
            running_read.anonymous_enabled,
            running_read
                .game
                .alive_players()
                .into_iter()
                .filter(|player| player.role == Role::Shaman)
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    if !has_shaman {
        return Ok(());
    }
    let mut overwrites = vec![dead_channel_overwrite(
        serenity::PermissionOverwriteType::Role(roles.everyone),
        false,
        false,
    )];
    if let Some(role_id) = roles.participant {
        overwrites.push(dead_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
            false,
        ));
    }
    if let Some(role_id) = roles.dead {
        overwrites.push(dead_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            true,
            !anonymous_enabled,
        ));
    }
    if let Some(role_id) = roles.spectator {
        overwrites.push(spectator_channel_overwrite(role_id));
    }
    if let Some(role_id) = roles.manager {
        overwrites.push(dead_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
            false,
        ));
    }
    overwrites.push(dead_channel_overwrite(
        serenity::PermissionOverwriteType::Member(roles.bot),
        true,
        true,
    ));
    for player in shamans {
        if guild_id
            .member(ctx, serenity::UserId::new(player.user_id))
            .await
            .is_ok()
        {
            overwrites.push(dead_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                true,
                false,
            ));
        }
    }

    let Some(channel) = create_text_channel_safe(
        ctx,
        guild_id,
        SHAMAN_CHAT_CHANNEL_NAME,
        overwrites,
        category,
        "마피아 게임 영매 채팅방 생성",
        0,
        None,
    )
    .await
    else {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            "영매 채팅방 생성에 실패했습니다. 봇에게 채널 관리 권한이 있는지 확인하세요.",
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
        return Ok(());
    };
    running.write().await.shaman_channel_id = Some(channel.id);
    let _ = send_channel_embed(
        &ctx.http,
        channel.id,
        "영매와 사망자가 접신하는 채팅방입니다.\n사망자는 이곳에서 대화할 수 있고, 영매는 밤에만 말할 수 있습니다.\n영매는 사망자 채팅방을 볼 수 없습니다.",
        "영매 채팅방",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
    upsert_shaman_chat_status(ctx, running).await;
    if anonymous_enabled {
        let shamans = {
            let running_read = running.read().await;
            running_read
                .game
                .alive_players()
                .into_iter()
                .filter(|player| player.role == Role::Shaman)
                .cloned()
                .collect::<Vec<_>>()
        };
        for shaman in shamans {
            let _ = ensure_anonymous_shaman_input_channel(
                ctx, running, &shaman, roles, category, false,
            )
            .await;
        }
    }
    Ok(())
}

async fn create_frog_chat_channel(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<()> {
    let (guild_id, has_witch) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read
                .game
                .players
                .iter()
                .any(|player| player.role == Role::Witch),
        )
    };
    if !has_witch {
        return Ok(());
    }
    let mut overwrites = vec![dead_channel_overwrite(
        serenity::PermissionOverwriteType::Role(roles.everyone),
        false,
        false,
    )];
    if let Some(role_id) = roles.participant {
        overwrites.push(dead_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
            false,
        ));
    }
    if let Some(role_id) = roles.spectator {
        overwrites.push(spectator_channel_overwrite(role_id));
    }
    if let Some(role_id) = roles.manager {
        overwrites.push(dead_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
            false,
        ));
    }
    overwrites.push(dead_channel_overwrite(
        serenity::PermissionOverwriteType::Member(roles.bot),
        true,
        true,
    ));
    let Some(channel) = create_text_channel_safe(
        ctx,
        guild_id,
        FROG_CHAT_CHANNEL_NAME,
        overwrites,
        category,
        "마피아 게임 개구리 채팅방 생성",
        0,
        None,
    )
    .await
    else {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            "개구리 채팅방 생성에 실패했습니다. 봇에게 채널 관리 권한이 있는지 확인하세요.",
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
        return Ok(());
    };
    running.write().await.frog_channel_id = Some(channel.id);
    let _ = send_channel_embed(
        &ctx.http,
        channel.id,
        "개구리 전용 채팅방입니다.\n저주에 걸린 참가자가 이곳에 쓴 말은 게임 채널에 개굴 소리로 전달됩니다.",
        "개구리 채팅방",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
    Ok(())
}

async fn set_frog_channel_member_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    can_view: bool,
    can_chat: bool,
) {
    let Some(channel_id) = running.read().await.frog_channel_id else {
        return;
    };
    let _ = channel_id
        .create_permission(
            &ctx.http,
            dead_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_view,
                can_chat,
            ),
        )
        .await;
}

async fn set_frog_game_channel_permission(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    can_chat: bool,
) {
    let channel_id = running.read().await.channel_id;
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id));
    let original = channel
        .permission_overwrites
        .iter()
        .find(|overwrite| overwrite.kind == kind)
        .cloned();
    {
        let mut running_write = running.write().await;
        running_write
            .frog_game_channel_overwrites
            .entry(player.user_id)
            .or_insert_with(|| original.clone());
    }
    let mut overwrite = original.unwrap_or(serenity::PermissionOverwrite {
        allow: serenity::Permissions::empty(),
        deny: serenity::Permissions::empty(),
        kind,
    });
    set_chat_permission_bits(&mut overwrite, can_chat);
    let _ = channel_id.create_permission(&ctx.http, overwrite).await;
}

async fn restore_frog_game_channel_permission(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
) {
    let (channel_id, original) = {
        let mut running_write = running.write().await;
        (
            running_write.channel_id,
            running_write
                .frog_game_channel_overwrites
                .remove(&player.user_id),
        )
    };
    let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id));
    match original {
        Some(Some(overwrite)) => {
            let _ = channel_id.create_permission(&ctx.http, overwrite).await;
        }
        Some(None) => {
            let _ = channel_id.delete_permission(&ctx.http, kind).await;
        }
        None => {}
    }
}

async fn restore_all_frog_game_channel_permissions(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let players = {
        let running_read = running.read().await;
        running_read
            .frog_game_channel_overwrites
            .keys()
            .filter_map(|user_id| running_read.game.get_player(*user_id))
            .cloned()
            .collect::<Vec<_>>()
    };
    for player in players {
        restore_frog_game_channel_permission(ctx, running, &player).await;
    }
}

async fn sync_madam_seduction_permissions(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    if running.read().await.anonymous_enabled {
        sync_anonymous_general_chat_permissions(ctx, running).await;
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    let (channel_id, seduced_ids) = {
        let running_read = running.read().await;
        (
            running_read.channel_id,
            running_read
                .game
                .alive_players()
                .into_iter()
                .filter(|player| running_read.game.is_madam_seduced(player))
                .map(|player| player.user_id)
                .collect::<HashSet<_>>(),
        )
    };
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        running
            .write()
            .await
            .madam_seduction_channel_overwrites
            .clear();
        return;
    };
    for user_id in &seduced_ids {
        let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(*user_id));
        let original = channel
            .permission_overwrites
            .iter()
            .find(|overwrite| overwrite.kind == kind)
            .cloned();
        {
            let mut running_write = running.write().await;
            running_write
                .madam_seduction_channel_overwrites
                .entry(*user_id)
                .or_insert_with(|| original.clone());
        }
        let mut overwrite = original.unwrap_or(serenity::PermissionOverwrite {
            allow: serenity::Permissions::empty(),
            deny: serenity::Permissions::empty(),
            kind,
        });
        set_chat_permission_bits(&mut overwrite, false);
        let _ = channel_id.create_permission(&ctx.http, overwrite).await;
    }

    let restore_ids = {
        let running_read = running.read().await;
        running_read
            .madam_seduction_channel_overwrites
            .keys()
            .filter(|user_id| !seduced_ids.contains(user_id))
            .copied()
            .collect::<Vec<_>>()
    };
    for user_id in restore_ids {
        restore_madam_seduction_permission(ctx, running, user_id).await;
    }
}

async fn restore_madam_seduction_permission(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    user_id: u64,
) {
    let (channel_id, original) = {
        let mut running_write = running.write().await;
        (
            running_write.channel_id,
            running_write
                .madam_seduction_channel_overwrites
                .remove(&user_id),
        )
    };
    let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id));
    match original {
        Some(Some(overwrite)) => {
            let _ = channel_id.create_permission(&ctx.http, overwrite).await;
        }
        Some(None) => {
            let _ = channel_id.delete_permission(&ctx.http, kind).await;
        }
        None => {}
    }
}

async fn restore_all_madam_seduction_permissions(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let user_ids = {
        let running_read = running.read().await;
        running_read
            .madam_seduction_channel_overwrites
            .keys()
            .copied()
            .collect::<Vec<_>>()
    };
    for user_id in user_ids {
        restore_madam_seduction_permission(ctx, running, user_id).await;
    }
}

async fn set_shaman_channel_member_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    can_view: bool,
    can_chat: bool,
) {
    let Some(channel_id) = running.read().await.shaman_channel_id else {
        return;
    };
    let _ = channel_id
        .create_permission(
            &ctx.http,
            dead_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_view,
                can_chat,
            ),
        )
        .await;
    upsert_shaman_chat_status(ctx, running).await;
}

async fn sync_shaman_chat_access(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (has_shaman_channel, anonymous_enabled, source_channel_id, players) = {
        let running_read = running.read().await;
        (
            running_read.shaman_channel_id.is_some(),
            running_read.anonymous_enabled,
            running_read.channel_id,
            running_read
                .game
                .players
                .iter()
                .filter(|player| {
                    player.role == Role::Shaman
                        || running_read
                            .anonymous_shaman_input_channel_ids
                            .contains_key(&player.user_id)
                })
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    if !has_shaman_channel {
        return;
    }
    let anonymous_context = if anonymous_enabled {
        let roles = running_channel_roles(ctx, data, running).await;
        let category = source_category(ctx, source_channel_id).await;
        roles.map(|roles| (roles, category))
    } else {
        None
    };
    for player in players {
        let can_shaman_chat = {
            let running_read = running.read().await;
            running_read
                .game
                .get_player(player.user_id)
                .is_some_and(|player| can_use_anonymous_shaman_chat(&running_read, player))
        };
        if player.role == Role::Shaman {
            set_shaman_channel_member_access(
                ctx,
                running,
                &player,
                true,
                !anonymous_enabled && can_shaman_chat,
            )
            .await;
        }
        if let Some((roles, category)) = anonymous_context {
            let _ = ensure_anonymous_shaman_input_channel(
                ctx,
                running,
                &player,
                roles,
                category,
                can_shaman_chat,
            )
            .await;
        }
    }
}

async fn set_anonymous_role_channel_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    role: Role,
    player: &Player,
    can_view: bool,
    can_chat: bool,
) {
    let (guild_id, source_channel_id, existing_channel_id, alias, status_text) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read.channel_id,
            running_read
                .anonymous_role_input_channel_ids
                .get(&(player.user_id, role))
                .copied(),
            running_read
                .anonymous_aliases
                .get(&player.user_id)
                .cloned()
                .unwrap_or_else(|| player.name.clone()),
            role_channel_status_text(&running_read, role),
        )
    };
    if guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
        .is_err()
    {
        return;
    }
    let channel_id = if let Some(channel_id) = existing_channel_id {
        channel_id
    } else if can_view {
        let category = source_category(ctx, source_channel_id).await;
        let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
        overwrites.push(anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
            true,
            can_chat,
        ));
        let Some(channel) = create_text_channel_safe(
            ctx,
            guild_id,
            &format!("{}-{}-채팅", sanitize_channel_part(&alias), role.value()),
            overwrites,
            category,
            "마피아 게임 익명 역할 채팅 권한 동기화",
            0,
            Some(format!("{} 익명 채팅 | {status_text}", role.value())),
        )
        .await
        else {
            return;
        };
        {
            let mut running_write = running.write().await;
            running_write
                .anonymous_role_input_channel_ids
                .insert((player.user_id, role), channel.id);
            running_write
                .anonymous_role_input_channels
                .insert(channel.id, (player.user_id, role));
            running_write.anonymous_channel_topics.insert(
                channel.id,
                format!("{} 익명 채팅 | {status_text}", role.value())
                    .chars()
                    .take(1024)
                    .collect::<String>(),
            );
        }
        let (message, title) = if can_chat {
            (
                format!(
                    "{} 역할 개인 채팅 채널입니다.\n여기에 쓰면 같은 역할의 개인 채팅방에 익명으로 전달됩니다.\n이 채널 하나에서 역할 대화와 밤 행동을 처리하세요.",
                    role.value()
                ),
                "익명 역할 입력",
            )
        } else {
            (
                format!(
                    "{} 역할 보기 전용 채널입니다.\n이 채널에서 역할 대화를 확인할 수 있습니다.",
                    role.value()
                ),
                "익명 역할 채팅",
            )
        };
        let _ = send_channel_embed(
            &ctx.http,
            channel.id,
            message,
            title,
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await;
        if let Ok(status_message) = send_channel_embed(
            &ctx.http,
            channel.id,
            status_text.clone(),
            &format!("{} 채팅 현황", role.value()),
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await
        {
            let mut running_write = running.write().await;
            running_write
                .anonymous_role_input_status_message_ids
                .insert((player.user_id, role), status_message.id);
            running_write
                .anonymous_role_status_texts
                .insert((player.user_id, role), status_text.clone());
        }
        channel.id
    } else {
        return;
    };
    let _ = channel_id
        .create_permission(
            &ctx.http,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_view,
                can_chat,
            ),
        )
        .await;
}

async fn set_private_role_member_view_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    role: Role,
    player: &Player,
    can_view: bool,
    can_chat: bool,
) {
    let can_chat = {
        let running_read = running.read().await;
        can_chat && !running_read.game.is_madam_seduced(player)
    };
    let Some(channel_id) = running.read().await.private_channel_ids.get(&role).copied() else {
        return;
    };
    let _ = channel_id
        .create_permission(
            &ctx.http,
            dead_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_view,
                can_chat,
            ),
        )
        .await;
    upsert_private_role_status_message(ctx, running, role).await;
}

async fn set_private_role_member_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    role: Role,
    player: &Player,
    can_chat: bool,
) {
    let can_chat = {
        let running_read = running.read().await;
        can_chat && !running_read.game.is_madam_seduced(player)
    };
    let Some(channel_id) = running.read().await.private_channel_ids.get(&role).copied() else {
        return;
    };
    let _ = channel_id
        .create_permission(
            &ctx.http,
            private_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_chat,
            ),
        )
        .await;
    upsert_private_role_status_message(ctx, running, role).await;
}

async fn disable_private_role_channels_for_player(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
) {
    let anonymous_updates = {
        let running_read = running.read().await;
        if running_read.anonymous_enabled {
            Some(
                running_read
                    .anonymous_role_input_channel_ids
                    .iter()
                    .filter_map(|(&(user_id, role), &channel_id)| {
                        (user_id == player.user_id).then_some((role, channel_id))
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        }
    };
    if let Some(updates) = anonymous_updates {
        for (role, channel_id) in updates {
            let _ = channel_id
                .create_permission(
                    &ctx.http,
                    anonymous_input_overwrite(
                        serenity::PermissionOverwriteType::Member(serenity::UserId::new(
                            player.user_id,
                        )),
                        false,
                        false,
                    ),
                )
                .await;
            upsert_anonymous_role_status_message(
                ctx,
                running,
                channel_id,
                role,
                (player.user_id, role),
            )
            .await;
        }
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    let roles = {
        let running_read = running.read().await;
        running_read
            .private_channel_ids
            .keys()
            .copied()
            .collect::<Vec<_>>()
    };
    for role in roles {
        set_private_role_member_access(ctx, running, role, player, false).await;
    }
}

async fn grant_private_role_member_access(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    role: Role,
    player: &Player,
) {
    let anonymous_enabled = running.read().await.anonymous_enabled;
    if anonymous_enabled {
        let Some(roles) = running_channel_roles(ctx, data, running).await else {
            return;
        };
        let can_access = {
            let running_read = running.read().await;
            player.alive
                && !running_read.game.is_frog(player)
                && !running_read.game.is_madam_seduced(player)
        };
        set_anonymous_role_channel_access(
            ctx, running, roles, role, player, can_access, can_access,
        )
        .await;
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    set_private_role_member_access(ctx, running, role, player, true).await;
}

async fn running_channel_roles(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Option<ChannelRoleIds> {
    let config = data.config.read().await.clone();
    let guild_id = running.read().await.guild_id;
    channel_role_ids(ctx, guild_id, &config, data.bot_user_id)
        .await
        .ok()
}

async fn sync_lover_chat_access(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (has_lover, anonymous_enabled, can_open, players) = {
        let running_read = running.read().await;
        (
            running_read
                .game
                .players
                .iter()
                .any(|player| player.role == Role::Lover),
            running_read.anonymous_enabled,
            lover_chat_is_open(&running_read.game),
            running_read.game.players.clone(),
        )
    };
    if !has_lover {
        return;
    }
    if anonymous_enabled {
        let Some(roles) = running_channel_roles(ctx, data, running).await else {
            return;
        };
        for player in players.iter().filter(|player| player.role == Role::Lover) {
            let can_access = {
                let running_read = running.read().await;
                can_open
                    && player.alive
                    && !running_read.game.is_frog(player)
                    && !running_read.game.is_madam_seduced(player)
            };
            set_anonymous_role_channel_access(
                ctx,
                running,
                roles,
                Role::Lover,
                player,
                can_access,
                can_access,
            )
            .await;
        }
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    for player in players.iter().filter(|player| player.role == Role::Lover) {
        let can_access = {
            let running_read = running.read().await;
            can_open && player.alive && !running_read.game.is_frog(player)
        };
        set_private_role_member_access(ctx, running, Role::Lover, player, can_access).await;
    }
}

async fn sync_cult_team_channel_access(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (has_cult_team, anonymous_enabled, players) = {
        let running_read = running.read().await;
        (
            running_read
                .game
                .players
                .iter()
                .any(|player| matches!(player.role, Role::CultLeader | Role::Fanatic)),
            running_read.anonymous_enabled,
            running_read.game.players.clone(),
        )
    };
    if !has_cult_team {
        return;
    }
    if anonymous_enabled {
        let Some(roles) = running_channel_roles(ctx, data, running).await else {
            return;
        };
        for player in &players {
            let (can_view, can_chat) = {
                let running_read = running.read().await;
                let can_view = player.alive
                    && !running_read.game.is_frog(player)
                    && running_read.game.is_cult_team(player);
                let can_chat = can_view
                    && player.role == Role::CultLeader
                    && !running_read.game.is_madam_seduced(player);
                (can_view, can_chat)
            };
            set_anonymous_role_channel_access(
                ctx,
                running,
                roles,
                Role::CultLeader,
                player,
                can_view,
                can_chat,
            )
            .await;
        }
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    for player in &players {
        let (can_view, can_chat) = {
            let running_read = running.read().await;
            let can_view = player.alive
                && !running_read.game.is_frog(player)
                && running_read.game.is_cult_team(player);
            let can_chat = can_view
                && player.role == Role::CultLeader
                && !running_read.game.is_madam_seduced(player);
            (can_view, can_chat)
        };
        set_private_role_member_view_access(
            ctx,
            running,
            Role::CultLeader,
            player,
            can_view,
            can_chat,
        )
        .await;
    }
}

async fn sync_scientist_mafia_permissions(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let scientist_players = {
        let running_read = running.read().await;
        running_read
            .game
            .players
            .iter()
            .filter(|player| {
                player.role == Role::Scientist
                    && running_read
                        .game
                        .scientist_contacted
                        .contains(&player.user_id)
                    && (player.alive
                        || running_read
                            .game
                            .scientist_pending_revive_ids
                            .contains(&player.user_id))
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    if scientist_players.is_empty() {
        return;
    }
    let anonymous_enabled = running.read().await.anonymous_enabled;
    if anonymous_enabled {
        let Some(roles) = running_channel_roles(ctx, data, running).await else {
            return;
        };
        for player in &scientist_players {
            set_anonymous_role_channel_access(ctx, running, roles, Role::Mafia, player, true, true)
                .await;
        }
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    for player in &scientist_players {
        set_private_role_member_access(ctx, running, Role::Mafia, player, true).await;
    }
}

async fn restore_revived_player_roles(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    player: &Player,
) {
    let guild_id = running.read().await.guild_id;
    if let Ok(member) = guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
    {
        if let Some(participant_role_id) = roles.participant {
            let _ = member.add_role(ctx, participant_role_id).await;
        }
        if let Some(dead_role_id) = roles.dead {
            let _ = member.remove_role(ctx, dead_role_id).await;
        }
    }
    set_shaman_channel_member_access(ctx, running, player, false, false).await;
    set_frog_channel_member_access(ctx, running, player, false, false).await;
    let anonymous_channel_ids = {
        let running_read = running.read().await;
        [
            running_read
                .anonymous_dead_input_channel_ids
                .get(&player.user_id)
                .copied(),
            running_read
                .anonymous_shaman_input_channel_ids
                .get(&player.user_id)
                .copied(),
        ]
    };
    for channel_id in anonymous_channel_ids.into_iter().flatten() {
        let _ = channel_id
            .create_permission(
                &ctx.http,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(
                        player.user_id,
                    )),
                    false,
                    false,
                ),
            )
            .await;
    }
    restore_frog_game_channel_permission(ctx, running, player).await;
    let grant_roles = {
        let running_read = running.read().await;
        let mut roles = Vec::new();
        if PRIVATE_CHAT_ROLES.contains(&player.role)
            && (player.role != Role::Lover || lover_chat_is_open(&running_read.game))
        {
            roles.push(player.role);
        }
        if running_read.game.is_known_mafia_team(player) {
            roles.push(Role::Mafia);
        }
        roles.sort_by_key(|role| role.value());
        roles.dedup();
        roles
    };
    for role in grant_roles {
        if running.read().await.anonymous_enabled {
            let can_access = {
                let running_read = running.read().await;
                player.alive
                    && !running_read.game.is_frog(player)
                    && !running_read.game.is_madam_seduced(player)
            };
            set_anonymous_role_channel_access(
                ctx, running, roles, role, player, can_access, can_access,
            )
            .await;
        } else {
            set_private_role_member_access(ctx, running, role, player, true).await;
        }
    }
    sync_anonymous_general_chat_permissions(ctx, running).await;
    sync_anonymous_role_statuses(ctx, running, true).await;
}

async fn apply_purification_side_effects(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    purified_user_ids: &[u64],
) {
    if purified_user_ids.is_empty() {
        return;
    }
    let config = data.config.read().await.clone();
    let (guild_id, channel_id, anonymous_enabled) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read.channel_id,
            running_read.anonymous_enabled,
        )
    };
    let roles = match channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await {
        Ok(roles) => roles,
        Err(_) => return,
    };
    let category = if anonymous_enabled {
        source_category(ctx, channel_id).await
    } else {
        None
    };
    for user_id in purified_user_ids {
        let player = running.read().await.game.get_player(*user_id).cloned();
        let Some(player) = player else {
            continue;
        };
        set_shaman_channel_member_access(ctx, running, &player, true, false).await;
        if anonymous_enabled {
            let _ =
                ensure_anonymous_dead_input_channel(ctx, running, &player, roles, category, false)
                    .await;
            let _ = ensure_anonymous_shaman_input_channel(
                ctx, running, &player, roles, category, false,
            )
            .await;
        }
    }
}

fn anonymous_vote_summary(game: &MafiaGame, result: &VoteResult) -> String {
    if result.vote_counts.is_empty() {
        return "투표 없음".to_string();
    }
    let mut rows = result
        .vote_counts
        .iter()
        .map(|(target_id, count)| {
            let name = target_id.map_or_else(
                || "스킵".to_string(),
                |id| {
                    game.get_player(id)
                        .map(|player| player.name.clone())
                        .unwrap_or_else(|| id.to_string())
                },
            );
            (name, *count)
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.to_lowercase().cmp(&right.0.to_lowercase()))
    });
    rows.into_iter()
        .map(|(name, count)| format!("- {name}: {count}표"))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn handle_madam_seduction_result(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    result: &VoteResult,
) {
    if result.madam_seduced.is_empty() {
        return;
    }
    for player in &result.madam_seduced {
        let _ = send_player_secret(
            ctx,
            running,
            player,
            "마담에게 유혹당했습니다. 다음 낮이 될 때까지 능력을 사용할 수 없고 말할 수 없습니다.\n마피아팀이라면 능력 사용은 가능하지만, 유혹 중에는 마피아 비밀방에도 말할 수 없습니다.",
            vec![],
        )
        .await;
        disable_private_role_channels_for_player(ctx, running, player).await;
    }
    let known_mafia_players = {
        let running_read = running.read().await;
        running_read
            .game
            .alive_players()
            .into_iter()
            .filter(|player| running_read.game.is_known_mafia_team(player))
            .cloned()
            .collect::<Vec<_>>()
    };
    for player in known_mafia_players {
        grant_private_role_member_access(ctx, data, running, Role::Mafia, &player).await;
    }
    let contacted_madams = {
        let running_read = running.read().await;
        running_read
            .game
            .alive_players()
            .into_iter()
            .filter(|player| {
                player.role == Role::Madam
                    && running_read.game.madam_contacted.contains(&player.user_id)
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    for madam in contacted_madams {
        grant_private_role_member_access(ctx, data, running, Role::Mafia, &madam).await;
        let _ = send_player_secret(
            ctx,
            running,
            &madam,
            "[접대] 마피아팀과 접선했습니다. 이제 마피아 비밀방에서 밤 대화가 가능합니다.",
            vec![],
        )
        .await;
    }
    sync_madam_seduction_permissions(ctx, running).await;
}

async fn cleanup_game(ctx: &serenity::Context, data: &Data, running: &Arc<RwLock<RunningGame>>) {
    restore_channel_slowmode(ctx, running).await;
    restore_member_game_channel_chat(ctx, running).await;
    restore_game_channel_chat(ctx, running).await;
    restore_all_frog_game_channel_permissions(ctx, running).await;
    restore_all_madam_seduction_permissions(ctx, running).await;
    let channel_ids = {
        let running_read = running.read().await;
        let mut channel_ids = Vec::new();
        channel_ids.extend(running_read.private_channel_ids.values().copied());
        channel_ids.extend(running_read.memo_channel_ids.values().copied());
        channel_ids.extend(running_read.anonymous_input_channel_ids.values().copied());
        channel_ids.extend(
            running_read
                .anonymous_dead_input_channel_ids
                .values()
                .copied(),
        );
        channel_ids.extend(
            running_read
                .anonymous_shaman_input_channel_ids
                .values()
                .copied(),
        );
        channel_ids.extend(
            running_read
                .anonymous_role_input_channel_ids
                .values()
                .copied(),
        );
        if let Some(channel_id) = running_read.shaman_channel_id {
            channel_ids.push(channel_id);
        }
        if let Some(channel_id) = running_read.frog_channel_id {
            channel_ids.push(channel_id);
        }
        channel_ids
    };

    let mut seen = HashSet::new();
    for channel_id in channel_ids {
        if seen.insert(channel_id) {
            let _ = channel_id.delete(&ctx.http).await;
        }
    }

    let (guild_id, participant_user_ids, spectator_user_ids) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read
                .participant_user_ids
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            running_read
                .spectator_user_ids
                .iter()
                .copied()
                .collect::<Vec<_>>(),
        )
    };
    let config = data.config.read().await.clone();
    if let Ok(roles) = channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await {
        for user_id in participant_user_ids {
            if let Ok(member) = guild_id.member(ctx, serenity::UserId::new(user_id)).await {
                if let Some(role_id) = roles.participant {
                    let _ = member.remove_role(ctx, role_id).await;
                }
                if let Some(role_id) = roles.dead {
                    let _ = member.remove_role(ctx, role_id).await;
                }
            }
        }
        if let Some(role_id) = roles.spectator {
            for user_id in spectator_user_ids {
                if let Ok(member) = guild_id.member(ctx, serenity::UserId::new(user_id)).await {
                    let _ = member.remove_role(ctx, role_id).await;
                }
            }
        }
    }

    let (source_channel_id, original_overwrites) = {
        let running_read = running.read().await;
        (
            running_read.channel_id,
            running_read.original_game_channel_overwrites.clone(),
        )
    };
    for (role_id, overwrite) in original_overwrites {
        match overwrite {
            Some(overwrite) => {
                let _ = source_channel_id
                    .create_permission(&ctx.http, overwrite)
                    .await;
            }
            None => {
                let _ = source_channel_id
                    .delete_permission(&ctx.http, serenity::PermissionOverwriteType::Role(role_id))
                    .await;
            }
        }
    }

    let mut running_write = running.write().await;
    if !running_write.anonymous_original_names.is_empty() {
        let original_names = running_write.anonymous_original_names.clone();
        for player in &mut running_write.game.players {
            if let Some(original) = original_names.get(&player.user_id) {
                player.name.clone_from(original);
            }
        }
    }
    running_write.private_channel_ids.clear();
    running_write.private_role_status_message_ids.clear();
    running_write.private_role_status_texts.clear();
    running_write.game_status_message_id = None;
    running_write.game_status_text = None;
    running_write.memo_channel_ids.clear();
    running_write.anonymous_input_channel_ids.clear();
    running_write.anonymous_input_channel_owners.clear();
    running_write.anonymous_dead_input_channel_ids.clear();
    running_write.anonymous_dead_input_channel_owners.clear();
    running_write.anonymous_shaman_input_channel_ids.clear();
    running_write.anonymous_shaman_input_channel_owners.clear();
    running_write.anonymous_role_input_channel_ids.clear();
    running_write.anonymous_role_input_channels.clear();
    running_write
        .anonymous_role_input_status_message_ids
        .clear();
    running_write.anonymous_role_status_texts.clear();
    running_write.anonymous_channel_topics.clear();
    running_write.anonymous_aliases.clear();
    running_write.anonymous_original_names.clear();
    running_write.anonymous_webhook_urls.clear();
    running_write.original_game_channel_overwrites.clear();
    running_write.game_channel_overwrites.clear();
    running_write.member_channel_overwrites.clear();
    running_write.original_slowmode_delays.clear();
    running_write.shaman_channel_id = None;
    running_write.shaman_status_message_id = None;
    running_write.shaman_status_text = None;
    running_write.frog_channel_id = None;
    running_write.frog_game_channel_overwrites.clear();
    running_write.madam_seduction_channel_overwrites.clear();
}

async fn sync_anonymous_general_chat_permissions(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let updates = {
        let running_read = running.read().await;
        if !running_read.anonymous_enabled {
            return;
        }
        running_read
            .game
            .players
            .iter()
            .filter_map(|player| {
                let channel_id = running_read
                    .anonymous_input_channel_ids
                    .get(&player.user_id)
                    .copied()?;
                Some((
                    channel_id,
                    player.user_id,
                    can_use_anonymous_general_chat(&running_read, player),
                ))
            })
            .collect::<Vec<_>>()
    };
    for (channel_id, user_id, can_chat) in updates {
        let _ = channel_id
            .create_permission(
                &ctx.http,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id)),
                    true,
                    can_chat,
                ),
            )
            .await;
    }
}

async fn set_game_channel_chat(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    mut participants_can_chat: bool,
) {
    let anonymous_enabled = running.read().await.anonymous_enabled;
    if anonymous_enabled {
        sync_anonymous_general_chat_permissions(ctx, running).await;
        participants_can_chat = false;
    }
    let Some(roles) = running_channel_roles(ctx, data, running).await else {
        return;
    };
    let channel_id = running.read().await.channel_id;
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let mut targets = vec![(roles.everyone, false)];
    if let Some(participant_role_id) = roles.participant {
        targets.push((participant_role_id, participants_can_chat));
    }
    for (role_id, can_chat) in targets {
        let kind = serenity::PermissionOverwriteType::Role(role_id);
        let current = channel
            .permission_overwrites
            .iter()
            .find(|overwrite| overwrite.kind == kind)
            .cloned();
        {
            let mut running_write = running.write().await;
            if !running_write.game_channel_overwrites.contains_key(&role_id) {
                let original = running_write
                    .original_game_channel_overwrites
                    .get(&role_id)
                    .cloned()
                    .unwrap_or_else(|| current.clone());
                running_write
                    .game_channel_overwrites
                    .insert(role_id, original);
            }
        }
        let mut overwrite = current.unwrap_or(serenity::PermissionOverwrite {
            allow: serenity::Permissions::empty(),
            deny: serenity::Permissions::empty(),
            kind,
        });
        set_chat_permission_bits(&mut overwrite, can_chat);
        let _ = channel_id.create_permission(&ctx.http, overwrite).await;
    }
}

async fn set_member_game_channel_chat(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    can_chat: bool,
) {
    if running.read().await.anonymous_enabled {
        sync_anonymous_general_chat_permissions(ctx, running).await;
        return;
    }
    let channel_id = running.read().await.channel_id;
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id));
    let current = channel
        .permission_overwrites
        .iter()
        .find(|overwrite| overwrite.kind == kind)
        .cloned();
    {
        let mut running_write = running.write().await;
        running_write
            .member_channel_overwrites
            .entry(player.user_id)
            .or_insert_with(|| current.clone());
    }
    let mut overwrite = current.unwrap_or(serenity::PermissionOverwrite {
        allow: serenity::Permissions::empty(),
        deny: serenity::Permissions::empty(),
        kind,
    });
    set_chat_permission_bits(&mut overwrite, can_chat);
    let _ = channel_id.create_permission(&ctx.http, overwrite).await;
}

async fn restore_member_game_channel_chat(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (channel_id, originals) = {
        let mut running_write = running.write().await;
        (
            running_write.channel_id,
            std::mem::take(&mut running_write.member_channel_overwrites),
        )
    };
    for (user_id, original) in originals {
        let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id));
        match original {
            Some(overwrite) => {
                let _ = channel_id.create_permission(&ctx.http, overwrite).await;
            }
            None => {
                let _ = channel_id.delete_permission(&ctx.http, kind).await;
            }
        }
    }
}

async fn restore_game_channel_chat(ctx: &serenity::Context, running: &Arc<RwLock<RunningGame>>) {
    let (channel_id, originals) = {
        let mut running_write = running.write().await;
        (
            running_write.channel_id,
            std::mem::take(&mut running_write.game_channel_overwrites),
        )
    };
    for (role_id, original) in originals {
        let kind = serenity::PermissionOverwriteType::Role(role_id);
        match original {
            Some(overwrite) => {
                let _ = channel_id.create_permission(&ctx.http, overwrite).await;
            }
            None => {
                let _ = channel_id.delete_permission(&ctx.http, kind).await;
            }
        }
    }
}

fn push_unique_channel_id(ids: &mut Vec<serenity::ChannelId>, channel_id: serenity::ChannelId) {
    if !ids.contains(&channel_id) {
        ids.push(channel_id);
    }
}

fn slowmode_channel_ids(running: &RunningGame) -> Vec<serenity::ChannelId> {
    let mut ids = Vec::new();
    push_unique_channel_id(&mut ids, running.channel_id);
    for channel_id in running.anonymous_input_channel_ids.values() {
        push_unique_channel_id(&mut ids, *channel_id);
    }
    for channel_id in running.anonymous_dead_input_channel_ids.values() {
        push_unique_channel_id(&mut ids, *channel_id);
    }
    for channel_id in running.anonymous_shaman_input_channel_ids.values() {
        push_unique_channel_id(&mut ids, *channel_id);
    }
    for channel_id in running.anonymous_role_input_channel_ids.values() {
        push_unique_channel_id(&mut ids, *channel_id);
    }
    for channel_id in running.private_channel_ids.values() {
        push_unique_channel_id(&mut ids, *channel_id);
    }
    if let Some(channel_id) = running.shaman_channel_id {
        push_unique_channel_id(&mut ids, channel_id);
    }
    if let Some(channel_id) = running.frog_channel_id {
        push_unique_channel_id(&mut ids, channel_id);
    }
    ids
}

async fn set_one_channel_slowmode(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    seconds: u64,
) {
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let slowmode = seconds.min(21600) as u16;
    {
        let mut running_write = running.write().await;
        running_write
            .original_slowmode_delays
            .entry(channel_id)
            .or_insert_with(|| channel.rate_limit_per_user.unwrap_or(0));
    }
    if channel.rate_limit_per_user.unwrap_or(0) == slowmode {
        return;
    }
    if let Err(error) = channel_id
        .edit(
            &ctx.http,
            serenity::EditChannel::new().rate_limit_per_user(slowmode),
        )
        .await
    {
        eprintln!("failed to set slowmode for {}: {error:?}", channel_id.get());
    }
}

async fn set_channel_slowmode(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    seconds: u64,
) {
    let channel_ids = {
        let running_read = running.read().await;
        slowmode_channel_ids(&running_read)
    };
    for channel_id in channel_ids {
        set_one_channel_slowmode(ctx, running, channel_id, seconds).await;
    }
}

async fn restore_channel_slowmode(ctx: &serenity::Context, running: &Arc<RwLock<RunningGame>>) {
    let originals = {
        let mut running_write = running.write().await;
        std::mem::take(&mut running_write.original_slowmode_delays)
    };
    for (channel_id, delay) in originals {
        if let Err(error) = channel_id
            .edit(
                &ctx.http,
                serenity::EditChannel::new().rate_limit_per_user(delay),
            )
            .await
        {
            eprintln!(
                "failed to restore slowmode for {}: {error:?}",
                channel_id.get()
            );
        }
    }
}

async fn apply_death_side_effects(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    dead_players: &[Player],
) {
    if dead_players.is_empty() {
        return;
    }
    let config = data.config.read().await.clone();
    let (guild_id, channel_id) = {
        let running_read = running.read().await;
        (running_read.guild_id, running_read.channel_id)
    };
    let Ok(roles) = channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await else {
        return;
    };
    for player in dead_players {
        if let Ok(member) = guild_id
            .member(ctx, serenity::UserId::new(player.user_id))
            .await
        {
            if let Some(participant_role_id) = roles.participant {
                let _ = member.remove_role(ctx, participant_role_id).await;
            }
            if let Some(dead_role_id) = roles.dead {
                let _ = member.add_role(ctx, dead_role_id).await;
            }
        }
        let can_dead_chat = {
            let running_read = running.read().await;
            !running_read
                .game
                .purified_dead_ids
                .contains(&player.user_id)
        };
        set_shaman_channel_member_access(ctx, running, player, true, can_dead_chat).await;
        set_frog_channel_member_access(ctx, running, player, false, false).await;
        restore_frog_game_channel_permission(ctx, running, player).await;
        disable_private_role_channels_for_player(ctx, running, player).await;
    }
    if running.read().await.anonymous_enabled {
        let category = source_category(ctx, channel_id).await;
        for player in dead_players {
            let can_chat = {
                let running_read = running.read().await;
                running_read
                    .game
                    .get_player(player.user_id)
                    .is_some_and(|player| can_use_anonymous_dead_chat(&running_read, player))
            };
            let _ = ensure_anonymous_dead_input_channel(
                ctx, running, player, roles, category, can_chat,
            )
            .await;
            if running.read().await.shaman_channel_id.is_some() {
                let can_shaman_chat = {
                    let running_read = running.read().await;
                    running_read
                        .game
                        .get_player(player.user_id)
                        .is_some_and(|player| can_use_anonymous_shaman_chat(&running_read, player))
                };
                let _ = ensure_anonymous_shaman_input_channel(
                    ctx,
                    running,
                    player,
                    roles,
                    category,
                    can_shaman_chat,
                )
                .await;
            }
        }
        sync_anonymous_general_chat_permissions(ctx, running).await;
    }
}

async fn game_loop(
    ctx: serenity::Context,
    data: Data,
    running: Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let config = data.config.read().await.clone();
    setup_game_channels(&ctx, &data, &running).await?;
    {
        let running_read = running.read().await;
        let game = &running_read.game;
        send_channel_embed(
            &ctx.http,
            running_read.channel_id,
            public_game_settings_text(game, &config, "게임 방 설정입니다."),
            "방 설정",
            serenity::Colour::GOLD,
            vec![],
        )
        .await?;
        send_channel_embed(
            &ctx.http,
            running_read.channel_id,
            game_rule_text(game, &config, running_read.reveal_death_roles),
            "게임 설명",
            serenity::Colour::GOLD,
            vec![],
        )
        .await?;
    }
    send_roles(&ctx, &running, &config).await;
    upsert_game_status(&ctx, &running).await;
    loop {
        {
            let running_read = running.read().await;
            if running_read.game.phase == Phase::Ended {
                break;
            }
        }
        run_night(&ctx, &data, &running).await?;
        if running.read().await.game.phase == Phase::Ended {
            break;
        }
        if announce_winner(&ctx, &data, &running).await? {
            break;
        }
        run_day(&ctx, &data, &running).await?;
        if running.read().await.game.phase == Phase::Ended {
            break;
        }
        run_vote(&ctx, &data, &running).await?;
        if running.read().await.game.phase == Phase::Ended {
            break;
        }
        if announce_winner(&ctx, &data, &running).await? {
            break;
        }
    }
    cleanup_game(&ctx, &data, &running).await;
    let guild_id = running.read().await.guild_id;
    data.games.remove(&guild_id);
    Ok(())
}

async fn send_roles(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    config: &config::BotConfig,
) {
    let (channel_id, payloads) = {
        let running_read = running.read().await;
        let payloads = running_read
            .game
            .players
            .iter()
            .map(|player| {
                let anonymous_notice = if running_read.anonymous_enabled {
                    let alias = running_read
                        .anonymous_aliases
                        .get(&player.user_id)
                        .cloned()
                        .unwrap_or_else(|| "익명".to_string());
                    format!(
                        "\n\n익명 이름: **{alias}**\n채팅은 서버에 생성된 본인 익명 입력 채널에서만 진행하세요."
                    )
                } else {
                    String::new()
                };
                (
                    player.clone(),
                    format!(
                        "{}\n\n방 설정\n{}\n\n게임 설명\n{}\n\n본인 역할 설명은 `/마피아능력`, 전체 역할 설명은 `/역할설명`으로 다시 확인할 수 있습니다.{}",
                        role_message(&running_read.game, player),
                        public_game_settings_text(
                            &running_read.game,
                            config,
                            "현재 게임 설정입니다."
                        ),
                        game_rule_text(
                            &running_read.game,
                            config,
                            running_read.reveal_death_roles
                        ),
                        anonymous_notice
                    ),
                )
            })
            .collect::<Vec<_>>();
        (running_read.channel_id, payloads)
    };
    let mut failed_names = Vec::new();
    for (player, message) in payloads {
        if !send_player_secret(ctx, running, &player, message, vec![]).await {
            failed_names.push(player.name);
        }
    }
    if !failed_names.is_empty() {
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "비밀 메시지를 보낼 수 없는 참가자: {}",
                failed_names.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    let _ = send_channel_embed(
        &ctx.http,
        channel_id,
        "역할 배정이 끝났습니다. 각자 비밀 메시지와 역할별 비공개 채널을 확인하세요.",
        "역할 배정 완료",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
}

fn role_message(game: &MafiaGame, player: &Player) -> String {
    let team = if game.is_cult_team(player) {
        "교주팀"
    } else if game.is_mafia_team(player) {
        "마피아팀"
    } else if player.role == Role::Joker {
        "중립"
    } else {
        "시민팀"
    };
    format!(
        "당신의 역할은 **{}** 입니다.\n진영: **{}**\n\n{}",
        player.role.value(),
        team,
        role_short_guide(player.role)
    )
}

fn role_short_guide(role: Role) -> &'static str {
    match role {
        Role::Mafia => "밤마다 제거할 대상을 선택합니다.",
        Role::Doctor => "밤마다 보호할 대상을 선택합니다.",
        Role::Police => "밤마다 한 명을 조사합니다.",
        Role::Agent => "밤마다 시민팀 지령 정보를 받습니다.",
        Role::Vigilante => "낮에 조사하고 밤에 숙청할 수 있습니다.",
        Role::Detective => "밤 행동의 이동 경로를 추적합니다.",
        Role::Shaman => "사망자를 성불하고 직업을 확인합니다.",
        Role::Priest => "사망자를 한 번 소생시킬 수 있습니다.",
        Role::Reporter => "두 번째 밤부터 특종으로 직업을 공개합니다.",
        Role::Hacker => "낮에 해킹해 직업을 확인하고 능력을 우회합니다.",
        Role::Terrorist => "지목한 위험 대상을 함께 데려갈 수 있습니다.",
        Role::Lover => "연인과 정보를 공유하고 서로를 지킵니다.",
        Role::Soldier => "마피아 공격을 한 번 버팁니다.",
        Role::Spy => "밤마다 직업을 확인하고 마피아와 접선합니다.",
        Role::Contractor => "두 명의 직업을 맞히면 암살합니다.",
        Role::Thief => "투표 시간에 능력을 훔칩니다.",
        Role::Witch => "밤에 대상을 개구리로 저주합니다.",
        Role::Scientist => "사망 후 다음 밤 부활합니다.",
        Role::Madam => "투표로 대상을 유혹합니다.",
        Role::Godfather => "세 번째 밤부터 확정 처치합니다.",
        Role::CultLeader => "홀수날 밤마다 포교합니다.",
        Role::Fanatic => "교주팀 여부를 확인하고 교주를 찾습니다.",
        Role::Joker => "낮 처형으로 단독 승리합니다.",
        Role::Politician => "투표가 2표이며 처형 면역이 있습니다.",
        Role::Judge => "찬반투표 결과를 뒤집을 수 있습니다.",
        Role::Gangster => "밤에 한 명의 다음 낮 투표권을 빼앗습니다.",
        Role::Prophet => "4번째 낮까지 생존하면 소속팀이 승리합니다.",
        Role::Psychologist => "낮에 두 명이 같은 팀인지 봅니다.",
        Role::Graverobber => "첫날 사망자의 직업을 이어받습니다.",
        _ => "낮 토론과 투표로 승리를 노리세요.",
    }
}

fn death_role_text(running: &RunningGame, player: &Player) -> String {
    if running.reveal_death_roles {
        format!("직업은 **{}** 입니다.", player.role.value())
    } else {
        "직업은 공개되지 않습니다.".to_string()
    }
}

async fn trigger_timed_night_events(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let (guild_id, cursed_players, witch_contacts, cult_bells, revived_players) = {
        let mut running_write = running.write().await;
        if running_write.game.phase != Phase::Night {
            return Ok(());
        }
        let (cursed_players, witch_contacts) = running_write.game.apply_witch_curses();
        let cult_bells = running_write.game.consume_cult_bells();
        let revived_players = running_write.game.revive_pending_scientists();
        (
            running_write.guild_id,
            cursed_players,
            witch_contacts,
            cult_bells,
            revived_players,
        )
    };

    if cursed_players.is_empty()
        && witch_contacts.is_empty()
        && cult_bells == 0
        && revived_players.is_empty()
    {
        return Ok(());
    }

    for player in &cursed_players {
        set_frog_channel_member_access(ctx, running, player, true, true).await;
        set_frog_game_channel_permission(ctx, running, player, false).await;
        disable_private_role_channels_for_player(ctx, running, player).await;
    }
    for user_id in &witch_contacts {
        let player = running.read().await.game.get_player(*user_id).cloned();
        if let Some(player) = player {
            grant_private_role_member_access(ctx, data, running, Role::Mafia, &player).await;
            let _ = send_player_secret(
                ctx,
                running,
                &player,
                "저주 대상이 마피아라 마피아와 접선했습니다.",
                vec![],
            )
            .await;
        }
    }
    if !cursed_players.is_empty() {
        send_game_embed(
            ctx,
            running,
            "마녀의 저주가 발동했습니다.\n누군가 다음 밤까지 개구리가 되었습니다.",
            "마녀 저주",
            serenity::Colour::ORANGE,
            vec![],
            false,
            true,
        )
        .await?;
    }
    if cult_bells > 0 {
        send_game_embed(
            ctx,
            running,
            std::iter::repeat_n("교주의 종소리가 울렸습니다.", cult_bells as usize)
                .collect::<Vec<_>>()
                .join("\n"),
            "교주 포교",
            serenity::Colour::ORANGE,
            vec![],
            false,
            true,
        )
        .await?;
    }
    if !revived_players.is_empty() {
        let config = data.config.read().await.clone();
        let roles = channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await?;
        for player in &revived_players {
            restore_revived_player_roles(ctx, running, roles, player).await;
        }
        send_game_embed(
            ctx,
            running,
            revived_players
                .iter()
                .map(|player| format!("[과학자 {}님이 부활했습니다.]", player.name))
                .collect::<Vec<_>>()
                .join("\n"),
            "과학자 부활",
            serenity::Colour::DARK_GREEN,
            vec![],
            false,
            true,
        )
        .await?;
    }
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    Ok(())
}

async fn run_night(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let (
        actors,
        restored_frogs,
        hacker_results,
        vigilante_results,
        godfather_contacts,
        seconds,
        notify,
    ) = {
        let config = data.config.read().await.clone();
        let mut running_write = running.write().await;
        running_write.game.phase = Phase::Night;
        running_write.day_chat_open = false;
        running_write.final_defense_user_id = None;
        running_write.night_timed_events_due = config.night_seconds <= 10;
        running_write.contractor_contract_drafts.clear();
        let restored_frogs = running_write.game.restore_frogs();
        let hacker_results = running_write.game.consume_hacker_results();
        let vigilante_results = running_write.game.consume_vigilante_results();
        let godfather_contacts = running_write.game.ensure_godfather_auto_contact();
        let actors = running_write.game.night_action_actors();
        (
            actors,
            restored_frogs,
            hacker_results,
            vigilante_results,
            godfather_contacts,
            config.night_seconds,
            running_write.night_notify.clone(),
        )
    };
    upsert_game_status(ctx, running).await;
    set_game_channel_chat(ctx, data, running, false).await;
    sync_lover_chat_access(ctx, data, running).await;
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_scientist_mafia_permissions(ctx, data, running).await;
    sync_madam_seduction_permissions(ctx, running).await;
    sync_anonymous_general_chat_permissions(ctx, running).await;
    sync_shaman_chat_access(ctx, data, running).await;
    for player in &restored_frogs {
        set_frog_channel_member_access(ctx, running, player, false, false).await;
        restore_frog_game_channel_permission(ctx, running, player).await;
    }
    for (user_id, message) in hacker_results.into_iter().chain(vigilante_results) {
        let player = running.read().await.game.get_player(user_id).cloned();
        if let Some(player) = player {
            let _ = send_player_secret(ctx, running, &player, message, vec![]).await;
        }
    }
    for user_id in godfather_contacts {
        let player = running.read().await.game.get_player(user_id).cloned();
        if let Some(player) = player {
            grant_private_role_member_access(ctx, data, running, Role::Mafia, &player).await;
            let _ = send_player_secret(
                ctx,
                running,
                &player,
                "세 번째 밤이 되어 마피아 팀과 자동 접선했습니다. 이제 마피아 비밀방을 볼 수 있고 밤마다 확정 처치 대상을 지목합니다.",
                vec![],
            )
            .await;
        }
    }
    send_game_embed(
        ctx,
        running,
        format!(
            "밤이 되었습니다. {seconds}초 동안 게임 채널 채팅이 비활성화됩니다.\n밤 행동이 있는 역할은 본인 익명 채널 또는 DM에서 선택합니다.\n행동 가능한 역할이 모두 선택하면 남은 시간을 기다리지 않고 바로 아침으로 넘어갑니다."
        ),
        "밤",
        serenity::Colour::GOLD,
        vec![],
        false,
        true,
    )
    .await?;
    let police_can_act = actors.iter().any(|actor| actor.role == Role::Police);
    let mut failed_names = Vec::new();
    for actor in actors {
        if !send_night_action_dm(ctx, running, &actor).await {
            failed_names.push(actor.name);
        }
    }
    if !failed_names.is_empty() {
        send_game_embed(
            ctx,
            running,
            format!(
                "밤 행동 선택지를 보낼 수 없는 참가자: {}",
                failed_names.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
            false,
            true,
        )
        .await?;
    }
    let has_changeable_mafia_action = { running.write().await.game.has_changeable_mafia_action() };
    if has_changeable_mafia_action {
        upsert_private_role_status_message(ctx, running, Role::Mafia).await;
    }
    if seconds <= 10 {
        {
            let mut running_write = running.write().await;
            running_write.night_timed_events_due = true;
        }
        trigger_timed_night_events(ctx, data, running).await?;
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(seconds)) => {}
            _ = notify.notified() => {}
        }
    } else {
        let reached_ten_seconds = tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(seconds - 10)) => true,
            _ = notify.notified() => false,
        };
        if running.read().await.game.phase == Phase::Ended {
            return Ok(());
        }
        {
            let mut running_write = running.write().await;
            running_write.night_timed_events_due = true;
        }
        if reached_ten_seconds {
            send_game_embed(
                ctx,
                running,
                "밤 시간이 10초 남았습니다. 아직 행동하지 않았다면 지금 선택하세요.",
                "밤 10초 전",
                serenity::Colour::GOLD,
                vec![],
                false,
                true,
            )
            .await?;
            trigger_timed_night_events(ctx, data, running).await?;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(10)) => {}
                _ = notify.notified() => {}
            }
        } else {
            trigger_timed_night_events(ctx, data, running).await?;
        }
    }
    if running.read().await.game.phase == Phase::Ended {
        return Ok(());
    }
    {
        let mut running_write = running.write().await;
        running_write.night_timed_events_due = true;
    }
    trigger_timed_night_events(ctx, data, running).await?;
    let result = {
        let mut running_write = running.write().await;
        running_write.game.resolve_night()?
    };
    let doctor_saved = result
        .mafia_target
        .as_ref()
        .zip(result.protected.as_ref())
        .is_some_and(|(mafia_target, protected)| mafia_target.user_id == protected.user_id)
        && result.mafia_target.as_ref().is_none_or(|mafia_target| {
            !result
                .killed_players
                .iter()
                .any(|player| player.user_id == mafia_target.user_id)
        })
        && result.lover_sacrifices.is_empty();
    apply_death_side_effects(ctx, data, running, &result.killed_players).await;
    if result.killed_players.is_empty() {
        if doctor_saved {
            if let Some(saved_player) = &result.protected {
                send_game_embed(
                    ctx,
                    running,
                    format!(
                        "아침이 밝았습니다. **{}**님이 의사의 치료로 살아났습니다.",
                        saved_player.name
                    ),
                    "밤 결과",
                    serenity::Colour::DARK_GREEN,
                    vec![],
                    true,
                    true,
                )
                .await?;
            }
        } else {
            send_game_embed(
                ctx,
                running,
                "아침이 밝았습니다. 아무도 사망하지 않았습니다.",
                "밤 결과",
                serenity::Colour::GOLD,
                vec![],
                true,
                true,
            )
            .await?;
        }
    } else {
        let mut lines = Vec::new();
        {
            let running_read = running.read().await;
            for killed in &result.killed_players {
                if result
                    .contractor_kills
                    .iter()
                    .any(|player| player.user_id == killed.user_id)
                {
                    lines.push(format!(
                        "- {} 님이 청부업자에게 정체를 들켜 암살 당했습니다. {}",
                        killed.name,
                        death_role_text(&running_read, killed)
                    ));
                } else if result
                    .vigilante_kills
                    .iter()
                    .any(|player| player.user_id == killed.user_id)
                {
                    lines.push(format!(
                        "- {} 님이 자경단원에게 숙청당했습니다. {}",
                        killed.name,
                        death_role_text(&running_read, killed)
                    ));
                } else {
                    lines.push(format!(
                        "- {}: {}",
                        killed.name,
                        death_role_text(&running_read, killed)
                    ));
                }
            }
        }
        let mut message = format!(
            "아침이 밝았습니다. 밤 사이 사망자가 발생했습니다.\n{}",
            lines.join("\n")
        );
        if !result.lover_sacrifices.is_empty() {
            let lover_lines = result
                .lover_sacrifices
                .iter()
                .map(|(savior, saved)| {
                    format!(
                        "- {}님이 연인 {}님을 살리고 대신 마피아에게 살해 당했습니다!",
                        savior.name, saved.name
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            message.push_str("\n\n연인 희생\n");
            message.push_str(&lover_lines);
        }
        if !result.terrorist_retaliations.is_empty() {
            let retaliation_lines = result
                .terrorist_retaliations
                .iter()
                .map(|(terrorist, target)| {
                    format!(
                        "- {} 님이 지목 중이던 {} 님도 함께 사망했습니다.",
                        terrorist.name, target.name
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            message.push_str("\n\n지목 반격\n");
            message.push_str(&retaliation_lines);
        }
        send_game_embed(
            ctx,
            running,
            message,
            "밤 결과",
            serenity::Colour::GOLD,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if !result.killed_players.is_empty()
        && doctor_saved
        && let Some(saved_player) = &result.protected
    {
        send_game_embed(
            ctx,
            running,
            format!("**{}**님이 의사의 치료로 살아났습니다.", saved_player.name),
            "의사 치료",
            serenity::Colour::DARK_GREEN,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if !result.soldier_blocks.is_empty() {
        send_game_embed(
            ctx,
            running,
            result
                .soldier_blocks
                .iter()
                .map(|soldier| {
                    format!(
                        "군인 **{}**님이 마피아의 공격을 버텨냈습니다!",
                        soldier.name
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            "군인 방탄",
            serenity::Colour::ORANGE,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if !result.priest_revives.is_empty() {
        send_game_embed(
            ctx,
            running,
            result
                .priest_revives
                .iter()
                .map(|player| format!("[{}님이 부활하셨습니다]", player.name))
                .collect::<Vec<_>>()
                .join("\n"),
            "성직자 소생",
            serenity::Colour::DARK_GREEN,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if !result.reporter_results.is_empty() {
        send_game_embed(
            ctx,
            running,
            result
                .reporter_results
                .values()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
            "기자 특종",
            serenity::Colour::DARK_GREEN,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if result.cult_bells > 0 {
        send_game_embed(
            ctx,
            running,
            std::iter::repeat_n("교주의 종소리가 울렸습니다.", result.cult_bells as usize)
                .collect::<Vec<_>>()
                .join("\n"),
            "교주 포교",
            serenity::Colour::ORANGE,
            vec![],
            true,
            true,
        )
        .await?;
    }
    send_private_result_maps(ctx, running, &result).await;
    apply_purification_side_effects(ctx, data, running, &result.shaman_purifications).await;
    if !result.priest_revives.is_empty() {
        let config = data.config.read().await.clone();
        let guild_id = running.read().await.guild_id;
        if let Ok(roles) = channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await {
            for player in &result.priest_revives {
                restore_revived_player_roles(ctx, running, roles, player).await;
            }
        }
    }
    for user_id in result
        .spy_contacts
        .iter()
        .chain(&result.contractor_contacts)
        .chain(&result.witch_contacts)
    {
        let player = running.read().await.game.get_player(*user_id).cloned();
        if let Some(player) = player.filter(|player| player.alive) {
            grant_private_role_member_access(ctx, data, running, Role::Mafia, &player).await;
        }
    }
    for user_id in &result.nurse_contacts {
        let player = running.read().await.game.get_player(*user_id).cloned();
        if let Some(player) = player.filter(|player| player.alive) {
            grant_private_role_member_access(ctx, data, running, Role::Doctor, &player).await;
        }
    }
    for (user_id, inherited_role) in &result.graverobber_results {
        let player = running.read().await.game.get_player(*user_id).cloned();
        if let Some(player) = player {
            if PRIVATE_CHAT_ROLES.contains(inherited_role) {
                grant_private_role_member_access(ctx, data, running, *inherited_role, &player)
                    .await;
            }
            let _ = send_player_secret(
                ctx,
                running,
                &player,
                format!(
                    "도굴꾼 능력으로 **{}** 직업을 이어받았습니다.",
                    inherited_role.value()
                ),
                vec![],
            )
            .await;
        }
    }
    for user_id in &result.fanatic_inherits {
        let player = running.read().await.game.get_player(*user_id).cloned();
        if let Some(player) = player {
            let _ = send_player_secret(
                ctx,
                running,
                &player,
                "교주가 사망해 광신도가 교주의 능력을 물려받았습니다.",
                vec![],
            )
            .await;
        }
    }
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    announce_police_result(ctx, running, &result).await;
    let config = data.config.read().await.clone();
    announce_public_police_status(ctx, running, &config, police_can_act, &result).await?;
    announce_morning_mafia_count(ctx, running, &config).await?;
    upsert_game_status(ctx, running).await;
    Ok(())
}

async fn send_night_action_dm(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    actor: &Player,
) -> bool {
    let (guild_id, role, targets) = {
        let running_read = running.read().await;
        let role = effective_night_role(&running_read.game, actor);
        let targets = if role == Role::Contractor {
            running_read.game.contractor_contract_targets(actor)
        } else {
            night_targets(&running_read.game, actor)
        };
        (running_read.guild_id, role, targets)
    };
    if targets.is_empty() && role != Role::Reporter {
        return true;
    };
    if role == Role::Contractor {
        return send_player_secret(
            ctx,
            running,
            actor,
            "청부업자 밤 행동을 선택하세요.\n두 명과 각 직업을 추측합니다. 둘 중 한 명이라도 마피아를 정확히 맞히면 접선합니다.\n첫날 밤에는 사용할 수 없고, 수사직과 직업이 공개된 사람은 대상에서 제외됩니다.",
            contractor_contract_components(guild_id, actor.user_id, &targets),
        )
        .await;
    }
    send_player_secret(
        ctx,
        running,
        actor,
        format!("{} 밤 행동을 선택하세요", role.value()),
        night_action_components(guild_id, actor.user_id, role, &targets),
    )
    .await
}

fn night_action_components(
    guild_id: serenity::GuildId,
    actor_id: u64,
    role: Role,
    targets: &[Player],
) -> Vec<serenity::CreateActionRow> {
    let mut options = targets
        .iter()
        .take(if role == Role::Reporter { 24 } else { 25 })
        .map(|target| {
            serenity::CreateSelectMenuOption::new(
                target.name.chars().take(100).collect::<String>(),
                target.user_id.to_string(),
            )
        })
        .collect::<Vec<_>>();
    if role == Role::Reporter {
        options.push(serenity::CreateSelectMenuOption::new("사용 안함", "skip"));
    }
    let select = serenity::CreateSelectMenu::new(
        format!("night:{}:{}:{}", guild_id.get(), actor_id, role.value()),
        serenity::CreateSelectMenuKind::String { options },
    )
    .placeholder(night_placeholder(role))
    .min_values(1)
    .max_values(1);
    vec![serenity::CreateActionRow::SelectMenu(select)]
}

fn contractor_contract_components(
    guild_id: serenity::GuildId,
    actor_id: u64,
    targets: &[Player],
) -> Vec<serenity::CreateActionRow> {
    (0..2)
        .flat_map(|slot| {
            let target_options = targets
                .iter()
                .take(25)
                .map(|target| {
                    serenity::CreateSelectMenuOption::new(
                        target.name.chars().take(100).collect::<String>(),
                        target.user_id.to_string(),
                    )
                })
                .collect::<Vec<_>>();
            let role_options = CONTRACTOR_GUESS_ROLES
                .iter()
                .map(|role| serenity::CreateSelectMenuOption::new(role.value(), role.value()))
                .collect::<Vec<_>>();
            [
                serenity::CreateActionRow::SelectMenu(
                    serenity::CreateSelectMenu::new(
                        format!("contractor_target:{}:{}:{}", guild_id.get(), actor_id, slot),
                        serenity::CreateSelectMenuKind::String {
                            options: target_options,
                        },
                    )
                    .placeholder(format!("{}번째 청부 대상", slot + 1))
                    .min_values(1)
                    .max_values(1),
                ),
                serenity::CreateActionRow::SelectMenu(
                    serenity::CreateSelectMenu::new(
                        format!("contractor_role:{}:{}:{}", guild_id.get(), actor_id, slot),
                        serenity::CreateSelectMenuKind::String {
                            options: role_options,
                        },
                    )
                    .placeholder(format!("{}번째 대상 직업 추측", slot + 1))
                    .min_values(1)
                    .max_values(1),
                ),
            ]
        })
        .chain([serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(format!(
                "contractor_submit:{}:{}",
                guild_id.get(),
                actor_id
            ))
            .label("청부 확정")
            .style(serenity::ButtonStyle::Danger),
        ])])
        .collect()
}

fn night_placeholder(role: Role) -> &'static str {
    match role {
        Role::Mafia => "공격할 대상을 선택하세요",
        Role::Doctor => "보호할 대상을 선택하세요",
        Role::Nurse => "처방/치료 대상을 선택하세요",
        Role::Police => "조사할 대상을 선택하세요",
        Role::Vigilante => "숙청할 대상을 선택하세요",
        Role::Reporter => "특종 대상 또는 사용 안함을 선택하세요",
        Role::Detective => "추적할 대상을 선택하세요",
        Role::Shaman => "성불할 사망자를 선택하세요",
        Role::Priest => "소생할 사망자를 선택하세요",
        Role::Spy => "첩보할 대상을 선택하세요",
        Role::Witch => "저주할 대상을 선택하세요",
        Role::Godfather => "확정 처치할 대상을 선택하세요",
        Role::Terrorist => "지목할 대상을 선택하세요",
        Role::Gangster => "공갈할 대상을 선택하세요",
        Role::Thief => "도벽으로 훔친 능력의 대상을 선택하세요",
        Role::CultLeader => "포교할 대상을 선택하세요",
        Role::Fanatic => "추종할 대상을 선택하세요",
        _ => "대상을 선택하세요",
    }
}

fn effective_night_role(game: &MafiaGame, actor: &Player) -> Role {
    if actor.role == Role::Thief {
        game.thief_night_role(actor).unwrap_or(actor.role)
    } else {
        actor.role
    }
}

fn night_targets(game: &MafiaGame, actor: &Player) -> Vec<Player> {
    let role = effective_night_role(game, actor);
    let mut alive = game
        .alive_players()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    alive.sort_by_key(|player| player.name.to_lowercase());
    let mut targets = match role {
        Role::Mafia => alive
            .into_iter()
            .filter(|player| game.can_mafia_attack(player, Some(actor.user_id)))
            .collect(),
        Role::Doctor => alive,
        Role::Nurse => {
            if game.nurse_contacted.contains(&actor.user_id) {
                if game.alive_role_count(Role::Doctor) == 0 {
                    alive
                } else {
                    Vec::new()
                }
            } else {
                alive
                    .into_iter()
                    .filter(|player| player.user_id != actor.user_id)
                    .collect()
            }
        }
        Role::Shaman | Role::Priest => game
            .unpurified_dead_players()
            .into_iter()
            .cloned()
            .collect(),
        Role::CultLeader => alive
            .into_iter()
            .filter(|player| player.user_id != actor.user_id && !game.is_cult_team(player))
            .collect(),
        Role::Vigilante => game.vigilante_execution_targets(actor),
        Role::Contractor => game.contractor_contract_targets(actor),
        _ => alive
            .into_iter()
            .filter(|player| player.user_id != actor.user_id)
            .collect(),
    };
    targets.sort_by_key(|player| player.name.to_lowercase());
    targets
}

async fn send_private_result_maps(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    result: &NightResult,
) {
    let mut maps = vec![
        result.detective_results.clone(),
        result.spy_results.clone(),
        result.contractor_results.clone(),
        result.witch_results.clone(),
        result.godfather_results.clone(),
        result.shaman_results.clone(),
        result.priest_results.clone(),
        result.agent_results.clone(),
        result.reporter_results.clone(),
        result.vigilante_results.clone(),
        result.nurse_results.clone(),
        result.gangster_results.clone(),
        result.cult_results.clone(),
        result.fanatic_results.clone(),
    ];
    maps.push(result.hacker_results.clone());
    for map in maps {
        for (user_id, text) in map {
            let player = running.read().await.game.get_player(user_id).cloned();
            if let Some(player) = player {
                let _ = send_player_secret(ctx, running, &player, text, vec![]).await;
            }
        }
    }
    let _ = running;
}

async fn announce_police_result(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    result: &NightResult,
) {
    let (police_players, message) = {
        let running_read = running.read().await;
        if running_read.game.police_result_announced {
            return;
        }
        let police_players = running_read
            .game
            .alive_players()
            .into_iter()
            .filter(|player| player.role == Role::Police)
            .cloned()
            .collect::<Vec<_>>();
        if police_players.is_empty() {
            return;
        }
        let message = if let Some(target) = &result.police_target {
            let result_text = if result.police_target_is_mafia.unwrap_or(false) {
                "마피아입니다"
            } else {
                "마피아가 아닙니다"
            };
            format!("조사 결과: {} 님은 **{}**.", target.name, result_text)
        } else {
            "경찰 조사 대상이 과반을 넘지 못해 이번 밤 조사 결과가 없습니다.".to_string()
        };
        (police_players, message)
    };
    {
        let mut running_write = running.write().await;
        running_write.game.mark_police_result_announced();
    }
    for player in police_players {
        let _ = send_player_secret(ctx, running, &player, message.clone(), vec![]).await;
    }
}

async fn send_police_result_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    message: &str,
    exclude_user_id: Option<u64>,
) {
    let police_players = {
        let running_read = running.read().await;
        running_read
            .game
            .alive_players()
            .into_iter()
            .filter(|player| player.role == Role::Police && Some(player.user_id) != exclude_user_id)
            .cloned()
            .collect::<Vec<_>>()
    };
    for player in police_players {
        let _ = send_player_secret(ctx, running, &player, message, vec![]).await;
    }
}

async fn announce_public_police_status(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    config: &config::BotConfig,
    police_can_act: bool,
    result: &NightResult,
) -> Result<()> {
    if !config.reveal_public_police_status || !police_can_act {
        return Ok(());
    }
    let (message, color) = if result.police_target.is_none() {
        (
            "경찰 조사는 성공하지 못했습니다. 대상이 과반을 넘지 못했거나 선택이 완료되지 않았습니다.",
            serenity::Colour::ORANGE,
        )
    } else if result.police_target_is_mafia.unwrap_or(false) {
        (
            "경찰이 마피아를 발견했습니다. 자세한 조사 결과는 경찰 비공개 채널로 전달됩니다.",
            serenity::Colour::DARK_GREEN,
        )
    } else {
        (
            "경찰이 마피아를 발견하지 못했습니다. 자세한 조사 결과는 경찰 비공개 채널로 전달됩니다.",
            serenity::Colour::ORANGE,
        )
    };
    send_game_embed(
        ctx,
        running,
        message,
        "경찰 조사 결과 공개",
        color,
        vec![],
        true,
        true,
    )
    .await?;
    Ok(())
}

async fn announce_morning_mafia_count(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    config: &config::BotConfig,
) -> Result<()> {
    if !config.reveal_morning_mafia_count {
        return Ok(());
    }
    let mafia_count = {
        let running_read = running.read().await;
        running_read
            .game
            .alive_players()
            .into_iter()
            .filter(|player| running_read.game.is_known_mafia_team(player))
            .count()
    };
    send_game_embed(
        ctx,
        running,
        format!("현재 생존 마피아: **{mafia_count}명**"),
        "아침 마피아 현황",
        serenity::Colour::GOLD,
        vec![],
        true,
        true,
    )
    .await?;
    Ok(())
}

async fn run_day(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let config = data.config.read().await.clone();
    let (guild_id, day_notify, discussion_seconds, hackers, vigilantes, psychologists) = {
        let mut running_write = running.write().await;
        running_write.game.phase = Phase::Day;
        running_write.day_chat_open = true;
        running_write.final_defense_user_id = None;
        running_write.day_skip_voter_ids.clear();
        running_write.day_skip_confirmed = false;
        running_write.day_extension_voter_ids.clear();
        running_write.day_extension_active = false;
        running_write.day_extension_confirmed = false;
        (
            running_write.guild_id,
            running_write.day_notify.clone(),
            config.discussion_seconds,
            running_write.game.hacker_day_actors(),
            running_write.game.vigilante_day_actors(),
            running_write.game.psychologist_day_actors(),
        )
    };
    upsert_game_status(ctx, running).await;
    set_game_channel_chat(ctx, data, running, true).await;
    set_channel_slowmode(ctx, running, config.chat_slowmode_seconds).await;
    sync_lover_chat_access(ctx, data, running).await;
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_madam_seduction_permissions(ctx, running).await;
    sync_anonymous_general_chat_permissions(ctx, running).await;
    sync_shaman_chat_access(ctx, data, running).await;
    let discussion_time = duration_text(discussion_seconds);
    let public_status = running.read().await.game.public_status();
    let mut day_message = send_game_embed(
        ctx,
        running,
        format!(
            "{}일차 낮입니다. {discussion_time} 동안 자유롭게 토론하세요.\n생존자 과반이 `바로 투표`를 누르면 토론과 연장을 끝내고 바로 지목 투표로 넘어갑니다.\n시간이 지나면 {DAY_EXTENSION_VOTE_SECONDS}초 동안 1분 연장 투표가 열립니다. 생존자 과반수가 연장을 누르면 1분 연장되고, 연장은 낮마다 1번만 가능합니다. 과반수가 모이지 않으면 바로 투표로 넘어갑니다.\n{public_status}",
            running.read().await.game.day_number
        ),
        "낮 토론",
        serenity::Colour::GOLD,
        day_skip_components(guild_id, false, false),
        false,
        true,
    )
    .await?;
    let mut failed_hackers = Vec::new();
    for actor in hackers {
        if !send_day_single_select(ctx, running, &actor, "hacker", "해킹 대상을 선택하세요").await
        {
            failed_hackers.push(actor.name);
        }
    }
    if !failed_hackers.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "해커 낮 행동 DM을 보낼 수 없는 참가자: {}",
                failed_hackers.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    let mut failed_vigilantes = Vec::new();
    for actor in vigilantes {
        if !send_day_single_select(
            ctx,
            running,
            &actor,
            "vigilante",
            "숙청 조사 대상을 선택하세요",
        )
        .await
        {
            failed_vigilantes.push(actor.name);
        }
    }
    if !failed_vigilantes.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "자경단원 낮 행동 DM을 보낼 수 없는 참가자: {}",
                failed_vigilantes.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    let mut failed_psychologists = Vec::new();
    for actor in psychologists {
        if !send_day_multi_select(
            ctx,
            running,
            &actor,
            "psychologist",
            "관찰할 두 명을 선택하세요",
            2,
        )
        .await
        {
            failed_psychologists.push(actor.name);
        }
    }
    if !failed_psychologists.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "심리학자 낮 행동 선택지를 보낼 수 없는 참가자: {}",
                failed_psychologists.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    let mut extension_used = false;
    let mut current_discussion_seconds = discussion_seconds;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(current_discussion_seconds)) => {}
            _ = day_notify.notified() => {}
        }
        {
            let running_read = running.read().await;
            if running_read.game.phase == Phase::Ended || running_read.day_skip_confirmed {
                let _ = day_message
                    .edit(
                        &ctx.http,
                        serenity::EditMessage::new()
                            .components(day_skip_components(guild_id, true, true)),
                    )
                    .await;
                return Ok(());
            }
        }
        if extension_used {
            send_game_embed(
                ctx,
                running,
                "연장된 토론 시간이 종료되었습니다.\n토론 연장은 낮마다 1번만 가능하므로 바로 지목 투표로 넘어갑니다.",
                "낮 토론 종료",
                serenity::Colour::GOLD,
                vec![],
                false,
                true,
            )
            .await?;
            let _ = day_message
                .edit(
                    &ctx.http,
                    serenity::EditMessage::new()
                        .components(day_skip_components(guild_id, true, false)),
                )
                .await;
            return Ok(());
        }

        let (alive_count, required_votes) = {
            let mut running_write = running.write().await;
            let alive_count = running_write.game.alive_players().len();
            running_write.day_extension_voter_ids.clear();
            running_write.day_extension_active = true;
            running_write.day_extension_confirmed = false;
            (alive_count, alive_count / 2 + 1)
        };
        let mut extension_message = send_game_embed(
            ctx,
            running,
            format!(
                "{} 토론 시간이 지났습니다.\n{DAY_EXTENSION_VOTE_SECONDS}초 안에 생존자 과반수({required_votes}/{alive_count}명)가 `1분 연장`을 누르면 낮 토론을 1분 연장합니다.\n과반수가 모이지 않으면 바로 투표로 넘어갑니다.",
                duration_text(current_discussion_seconds)
            ),
            "낮 토론 연장 투표",
            serenity::Colour::GOLD,
            day_extension_components(guild_id, false, false),
            false,
            true,
        )
        .await?;
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(DAY_EXTENSION_VOTE_SECONDS)) => {}
            _ = day_notify.notified() => {}
        }
        let (skip_confirmed, extension_confirmed, extension_votes, phase_ended) = {
            let mut running_write = running.write().await;
            running_write.day_extension_active = false;
            (
                running_write.day_skip_confirmed,
                running_write.day_extension_confirmed,
                running_write.day_extension_voter_ids.len(),
                running_write.game.phase == Phase::Ended,
            )
        };
        if skip_confirmed {
            let _ = extension_message
                .edit(
                    &ctx.http,
                    serenity::EditMessage::new()
                        .embed(make_embed(
                            "생존자 과반수가 바로 투표를 선택해 연장 투표를 종료합니다.\n바로 지목 투표로 넘어갑니다.",
                            "바로 투표",
                            serenity::Colour::DARK_GREEN,
                        ))
                        .components(day_extension_components(guild_id, true, false)),
                )
                .await;
            let _ = day_message
                .edit(
                    &ctx.http,
                    serenity::EditMessage::new()
                        .components(day_skip_components(guild_id, true, true)),
                )
                .await;
            return Ok(());
        }
        if phase_ended {
            return Ok(());
        }
        if extension_confirmed {
            extension_used = true;
            current_discussion_seconds = DISCUSSION_EXTENSION_SECONDS;
            continue;
        }
        let _ = extension_message
            .edit(
                &ctx.http,
                serenity::EditMessage::new()
                    .embed(make_embed(
                        format!(
                            "{DAY_EXTENSION_VOTE_SECONDS}초 동안 1분 연장 투표가 과반수에 도달하지 못했습니다. ({extension_votes}/{required_votes}명)\n바로 투표로 넘어갑니다."
                        ),
                        "낮 토론 종료",
                        serenity::Colour::GOLD,
                    ))
                    .components(day_extension_components(guild_id, true, false)),
            )
            .await;
        let _ = day_message
            .edit(
                &ctx.http,
                serenity::EditMessage::new().components(day_skip_components(guild_id, true, false)),
            )
            .await;
        return Ok(());
    }
}

async fn send_day_single_select(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    actor: &Player,
    kind: &str,
    placeholder: &str,
) -> bool {
    send_day_multi_select(ctx, running, actor, kind, placeholder, 1).await
}

fn day_action_secret_text(kind: &str) -> &'static str {
    match kind {
        "hacker" => {
            "해커 낮 행동을 선택하세요.\n해킹은 1회용입니다. 선택한 대상의 직업은 밤이 시작될 때 비밀 메시지로 전달됩니다.\n해킹 사용 후 자신에게 쓰이는 능력은 해킹 대상에게 우회됩니다."
        }
        "vigilante" => {
            "자경단원 낮 행동을 선택하세요.\n숙청 조사는 1회용입니다. 밤이 시작될 때 대상이 마피아팀인지 비밀 메시지로 전달됩니다.\n숙청 처형은 조사와 별개로 밤에 한 번 시도할 수 있고, 마피아팀이 아니어도 기회가 소진됩니다."
        }
        "psychologist" => {
            "심리학자 낮 행동을 선택하세요.\n자신을 제외한 생존자 2명을 선택하면 두 사람이 같은 팀인지 즉시 확인합니다."
        }
        "thief" => {
            "도둑 투표 시간 행동을 선택하세요.\n하루에 한 번 플레이어 한 명의 직업 능력을 훔쳐 다음 밤까지 사용할 수 있습니다."
        }
        _ => "낮 능력을 선택하세요.",
    }
}

async fn send_day_multi_select(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    actor: &Player,
    kind: &str,
    placeholder: &str,
    count: u8,
) -> bool {
    let (guild_id, mut targets) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read
                .game
                .players
                .iter()
                .filter(|player| player.alive && player.user_id != actor.user_id)
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    targets.sort_by_key(|player| player.name.to_lowercase());
    let options = targets
        .iter()
        .take(25)
        .map(|target| {
            serenity::CreateSelectMenuOption::new(
                target.name.chars().take(100).collect::<String>(),
                target.user_id.to_string(),
            )
        })
        .collect::<Vec<_>>();
    let select = serenity::CreateSelectMenu::new(
        format!("{kind}:{}:{}", guild_id.get(), actor.user_id),
        serenity::CreateSelectMenuKind::String { options },
    )
    .placeholder(placeholder)
    .min_values(count)
    .max_values(count);
    send_player_secret(
        ctx,
        running,
        actor,
        day_action_secret_text(kind),
        vec![serenity::CreateActionRow::SelectMenu(select)],
    )
    .await
}

async fn run_vote(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let config = data.config.read().await.clone();
    let (guild_id, vote_notify, seconds, alive) = {
        let mut running_write = running.write().await;
        running_write.game.start_vote()?;
        running_write.day_chat_open = false;
        running_write.final_defense_user_id = None;
        (
            running_write.guild_id,
            running_write.vote_notify.clone(),
            config.vote_seconds,
            running_write
                .game
                .alive_players()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    upsert_game_status(ctx, running).await;
    set_game_channel_chat(ctx, data, running, false).await;
    sync_anonymous_general_chat_permissions(ctx, running).await;
    let mut options = alive
        .iter()
        .take(24)
        .map(|target| {
            serenity::CreateSelectMenuOption::new(
                target.name.chars().take(100).collect::<String>(),
                target.user_id.to_string(),
            )
        })
        .collect::<Vec<_>>();
    options.push(serenity::CreateSelectMenuOption::new("스킵", "skip"));
    let select = serenity::CreateSelectMenu::new(
        format!("vote:{}", guild_id.get()),
        serenity::CreateSelectMenuKind::String { options },
    )
    .placeholder("처형할 대상 또는 스킵을 선택하세요")
    .min_values(1)
    .max_values(1);
    send_game_embed(
        ctx,
        running,
        format!(
            "지목 투표를 시작합니다. {seconds}초 안에 최후변론에 세울 사람을 선택하세요.\n투표 중에는 게임 채널 채팅이 비활성화됩니다.\n생존자가 모두 투표하면 남은 시간을 기다리지 않고 바로 정산합니다."
        ),
        "지목 투표 시작",
        serenity::Colour::GOLD,
        vec![serenity::CreateActionRow::SelectMenu(select)],
        false,
        true,
    )
    .await?;
    send_thief_vote_actions(ctx, running).await;
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(seconds)) => {}
        _ = vote_notify.notified() => {}
    }
    if running.read().await.game.phase == Phase::Ended {
        return Ok(());
    }
    let vote_result = {
        let mut running_write = running.write().await;
        running_write.game.resolve_nomination_vote()?
    };
    handle_madam_seduction_result(ctx, data, running, &vote_result).await;
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    let vote_summary = {
        let running_read = running.read().await;
        anonymous_vote_summary(&running_read.game, &vote_result)
    };
    let blocked_notice = if vote_result.blocked_voters.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n공갈로 투표권을 잃은 참가자: {}",
            vote_result
                .blocked_voters
                .iter()
                .map(|player| player.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    if vote_result.executed.is_none() {
        let message = if vote_result.tied {
            "투표가 동률이라 최후변론 대상이 없습니다."
        } else if vote_result.skipped {
            "스킵이 최다 득표하여 최후변론 대상이 없습니다."
        } else {
            "투표가 없어 최후변론 대상이 없습니다."
        };
        send_game_embed(
            ctx,
            running,
            format!("{message}{blocked_notice}\n\n익명 투표 집계\n{vote_summary}"),
            "지목 투표 결과",
            serenity::Colour::GOLD,
            vec![],
            false,
            true,
        )
        .await?;
        return Ok(());
    }
    let nominee = vote_result.executed.unwrap();
    {
        let mut running_write = running.write().await;
        running_write.final_defense_user_id = Some(nominee.user_id);
    }
    sync_anonymous_general_chat_permissions(ctx, running).await;
    set_channel_slowmode(ctx, running, 0).await;
    if !running.read().await.game.is_frog(&nominee)
        && !running.read().await.game.is_madam_seduced(&nominee)
    {
        set_member_game_channel_chat(ctx, running, &nominee, true).await;
    }
    send_game_embed(
        ctx,
        running,
        format!(
            "지목 투표 결과, {} 님이 최후변론 대상이 되었습니다.{blocked_notice}\n\n익명 투표 집계\n{vote_summary}",
            nominee.name
        ),
        "지목 투표 결과",
        serenity::Colour::GOLD,
        vec![],
        false,
        true,
    )
    .await?;
    send_game_embed(
        ctx,
        running,
        format!(
            "{} 님의 최후변론 시간입니다. 20초 동안 지목된 사람만 말할 수 있습니다.\n이 시간 동안 슬로우모드는 해제됩니다.",
            nominee.name
        ),
        "최후변론",
        serenity::Colour::GOLD,
        vec![],
        false,
        true,
    )
    .await?;
    tokio::time::sleep(Duration::from_secs(20)).await;
    if running.read().await.game.phase == Phase::Ended {
        return Ok(());
    }
    {
        let mut running_write = running.write().await;
        running_write.game.start_confirmation_vote()?;
        running_write.final_defense_user_id = None;
    }
    restore_member_game_channel_chat(ctx, running).await;
    upsert_game_status(ctx, running).await;
    set_game_channel_chat(ctx, data, running, false).await;
    sync_anonymous_general_chat_permissions(ctx, running).await;
    let confirm_notify = running.read().await.confirm_notify.clone();
    send_game_embed(
        ctx,
        running,
        format!(
            "{} 님 처형 여부를 찬반투표합니다. {CONFIRM_VOTE_SECONDS}초 안에 선택하세요.\n찬성이 반대보다 많으면 처형합니다.",
            nominee.name
        ),
        "찬반투표",
        serenity::Colour::GOLD,
        vec![serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(format!("confirm:{}:1", guild_id.get()))
                .label("찬성")
                .style(serenity::ButtonStyle::Success),
            serenity::CreateButton::new(format!("confirm:{}:0", guild_id.get()))
                .label("반대")
                .style(serenity::ButtonStyle::Danger),
        ])],
        false,
        true,
    )
    .await?;
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(CONFIRM_VOTE_SECONDS)) => {}
        _ = confirm_notify.notified() => {}
    }
    if running.read().await.game.phase == Phase::Ended {
        return Ok(());
    }
    let confirm_result = {
        let mut running_write = running.write().await;
        running_write
            .game
            .resolve_confirmation_vote(nominee.user_id)?
    };
    set_channel_slowmode(ctx, running, config.chat_slowmode_seconds).await;
    let counts = &confirm_result.vote_counts;
    let summary = format!(
        "찬성 {}표 / 반대 {}표",
        counts.get(&true).copied().unwrap_or(0),
        counts.get(&false).copied().unwrap_or(0)
    );
    let judge_notice = if confirm_result.decided_by_judge {
        if let Some(judge) = &confirm_result.judge {
            let judge_choice = match confirm_result.judge_choice {
                None => "미투표(처형 없음)",
                Some(true) => "찬성",
                Some(false) => "반대",
            };
            format!(
                "\n\n[판사 {}님이 투표 결과를 정했습니다]\n판사의 선택: {judge_choice}",
                judge.name
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let mut dead_players = Vec::new();
    if let Some(executed) = &confirm_result.executed {
        dead_players.push(executed.clone());
    }
    dead_players.extend(confirm_result.extra_killed.iter().cloned());
    apply_death_side_effects(ctx, data, running, &dead_players).await;
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    upsert_game_status(ctx, running).await;
    let (message, color, include_dead) = if confirm_result.blocked_by_politician {
        (
            format!(
                "찬반투표 결과, {} 님은 **정치인** 입니다.\n[정치인은 투표로 죽지 않습니다]\n\n{} 님은 처형되지 않고 밤으로 넘어갑니다.{judge_notice}\n\n찬반투표 집계\n{summary}",
                nominee.name, nominee.name
            ),
            serenity::Colour::ORANGE,
            false,
        )
    } else if let Some(executed) = &confirm_result.executed {
        let killed_lines = {
            let running_read = running.read().await;
            dead_players
                .iter()
                .map(|killed| {
                    format!(
                        "- {}: {}",
                        killed.name,
                        death_role_text(&running_read, killed)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut result_message = format!("찬반투표 결과, {} 님이 처형되었습니다.", executed.name);
        if !confirm_result.extra_killed.is_empty() {
            if executed.role == Role::Terrorist {
                result_message.push_str(
                    "\n테러리스트의 [산화]가 발동해 지목 중이던 적 팀도 함께 사망했습니다.",
                );
            } else {
                result_message.push_str(
                    "\n처형 대상이 지목하고 있던 시민팀이 아닌 대상도 함께 사망했습니다.",
                );
            }
        }
        (
            format!(
                "{result_message}\n\n사망자\n{killed_lines}{judge_notice}\n\n찬반투표 집계\n{summary}"
            ),
            serenity::Colour::GOLD,
            true,
        )
    } else if confirm_result.tied {
        (
            format!(
                "찬반투표가 동률이라 처형하지 않습니다.{judge_notice}\n\n찬반투표 집계\n{summary}"
            ),
            serenity::Colour::GOLD,
            false,
        )
    } else {
        let reject_message = if confirm_result.decided_by_judge {
            "판사의 선택으로 처형하지 않습니다."
        } else {
            "반대가 많아 처형하지 않습니다."
        };
        (
            format!("{reject_message}{judge_notice}\n\n찬반투표 집계\n{summary}"),
            serenity::Colour::GOLD,
            false,
        )
    };
    send_game_embed(
        ctx,
        running,
        message,
        "찬반투표 결과",
        color,
        vec![],
        include_dead,
        true,
    )
    .await?;
    Ok(())
}

async fn send_thief_vote_actions(ctx: &serenity::Context, running: &Arc<RwLock<RunningGame>>) {
    let actors = running.read().await.game.thief_vote_actors();
    let mut failed_names = Vec::new();
    for actor in actors {
        if !send_day_single_select(ctx, running, &actor, "thief", "도벽 대상을 선택하세요").await
        {
            failed_names.push(actor.name);
        }
    }
    if !failed_names.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "도둑 도벽 선택지를 보낼 수 없는 참가자: {}",
                failed_names.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
}

async fn announce_winner(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<bool> {
    let winner = running.read().await.game.winner();
    let Some(winner) = winner else {
        return Ok(false);
    };
    let (roles_text, elapsed_seconds, record_payload) = {
        let mut running_write = running.write().await;
        running_write.game.phase = Phase::Ended;
        let elapsed_seconds = running_write.started_at.elapsed().as_secs() as i64;
        let record_payload = if running_write.stats_recorded {
            None
        } else {
            running_write.stats_recorded = true;
            Some((
                running_write.game.clone(),
                running_write.initial_roles.clone(),
                elapsed_seconds,
            ))
        };
        (
            final_role_reveal_text(&running_write),
            elapsed_seconds,
            record_payload,
        )
    };
    upsert_game_status(ctx, running).await;
    if let Some((game_snapshot, initial_roles, elapsed_seconds)) = record_payload {
        let mut stats_file = data.stats.write().await;
        stats::record_game_stats(
            &mut stats_file,
            &game_snapshot,
            &initial_roles,
            elapsed_seconds,
            winner,
        );
        stats::save_stats(&*data.stats_path, &stats_file)?;
    }
    send_game_embed(
        ctx,
        running,
        format!(
            "{}\n플레이 시간: **{}**\n\n최종 역할 공개\n{}",
            match winner {
                Winner::Mafia => "마피아 승리!",
                Winner::Joker => "조커 승리!",
                Winner::Cult => "교주팀 승리!",
                Winner::Citizen => "시민 승리!",
            },
            stats::play_duration_text(elapsed_seconds),
            roles_text
        ),
        "게임 종료",
        serenity::Colour::DARK_GREEN,
        vec![],
        true,
        true,
    )
    .await?;
    Ok(true)
}

async fn handle_component(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
) -> Result<()> {
    let custom_id = component.data.custom_id.as_str();
    let parts = custom_id.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        ["join", guild] => handle_join(ctx, data, component, parse_guild(guild)?).await?,
        ["spectate", guild] => handle_spectate(ctx, data, component, parse_guild(guild)?).await?,
        ["startnow", guild] => {
            handle_recruitment_finish(ctx, data, component, parse_guild(guild)?, false).await?
        }
        ["cancelrec", guild] => {
            handle_recruitment_finish(ctx, data, component, parse_guild(guild)?, true).await?
        }
        ["night", guild, actor_id, _role] => {
            handle_night_action(ctx, data, component, parse_guild(guild)?, actor_id.parse()?)
                .await?
        }
        ["contractor_target", guild, actor_id, slot] => {
            handle_contractor_target(
                ctx,
                data,
                component,
                parse_guild(guild)?,
                actor_id.parse()?,
                slot.parse()?,
            )
            .await?
        }
        ["contractor_role", guild, actor_id, slot] => {
            handle_contractor_role(
                ctx,
                data,
                component,
                parse_guild(guild)?,
                actor_id.parse()?,
                slot.parse()?,
            )
            .await?
        }
        ["contractor_submit", guild, actor_id] => {
            handle_contractor_submit(ctx, data, component, parse_guild(guild)?, actor_id.parse()?)
                .await?
        }
        ["vote", guild] => handle_day_vote(ctx, data, component, parse_guild(guild)?).await?,
        ["confirm", guild, approve] => {
            handle_confirm_vote(ctx, data, component, parse_guild(guild)?, *approve == "1").await?
        }
        ["skipday", guild] => handle_skip_day(ctx, data, component, parse_guild(guild)?).await?,
        ["extendday", guild] => {
            handle_day_extension(ctx, data, component, parse_guild(guild)?).await?
        }
        ["hacker", guild, actor_id] => {
            handle_hacker(ctx, data, component, parse_guild(guild)?, actor_id.parse()?).await?
        }
        ["vigilante", guild, actor_id] => {
            handle_vigilante(ctx, data, component, parse_guild(guild)?, actor_id.parse()?).await?
        }
        ["psychologist", guild, actor_id] => {
            handle_psychologist(ctx, data, component, parse_guild(guild)?, actor_id.parse()?)
                .await?
        }
        ["thief", guild, actor_id] => {
            handle_thief(ctx, data, component, parse_guild(guild)?, actor_id.parse()?).await?
        }
        _ => ack_component(ctx, component).await,
    }
    Ok(())
}

fn parse_guild(value: &str) -> Result<serenity::GuildId> {
    Ok(serenity::GuildId::new(value.parse()?))
}

fn selected_values(component: &serenity::ComponentInteraction) -> Vec<String> {
    match &component.data.kind {
        serenity::ComponentInteractionDataKind::StringSelect { values } => values.clone(),
        _ => Vec::new(),
    }
}

async fn handle_contractor_target(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
    slot: usize,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    if slot >= 2 {
        send_component_private(ctx, component, "잘못된 청부 선택입니다.").await?;
        return Ok(());
    }
    let Some(target_id) = selected_values(component)
        .first()
        .and_then(|value| value.parse().ok())
    else {
        send_component_private(ctx, component, "청부 대상을 선택해야 합니다.").await?;
        return Ok(());
    };
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    running
        .write()
        .await
        .contractor_contract_drafts
        .entry(actor_id)
        .or_default()
        .target_ids[slot] = Some(target_id);
    ack_component(ctx, component).await;
    Ok(())
}

async fn handle_contractor_role(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
    slot: usize,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    if slot >= 2 {
        send_component_private(ctx, component, "잘못된 청부 선택입니다.").await?;
        return Ok(());
    }
    let Some(role) = selected_values(component)
        .first()
        .and_then(|value| find_role_by_name(value))
    else {
        send_component_private(ctx, component, "청부 대상 직업을 선택해야 합니다.").await?;
        return Ok(());
    };
    if !CONTRACTOR_GUESS_ROLES.contains(&role) {
        send_component_private(ctx, component, "청부로 추측할 수 없는 직업입니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    running
        .write()
        .await
        .contractor_contract_drafts
        .entry(actor_id)
        .or_default()
        .guessed_roles[slot] = Some(role);
    ack_component(ctx, component).await;
    Ok(())
}

async fn handle_contractor_submit(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let (message, done, newly_contacted_mafia) = {
        let mut running_write = running.write().await;
        let was_known_mafia_team = running_write
            .game
            .get_player(actor_id)
            .is_some_and(|actor| running_write.game.is_known_mafia_team(actor));
        let Some(draft) = running_write
            .contractor_contract_drafts
            .get(&actor_id)
            .cloned()
        else {
            send_component_private(
                ctx,
                component,
                "청부 대상 2명과 각 대상의 직업을 모두 선택하세요.",
            )
            .await?;
            return Ok(());
        };
        let (Some(first_target_id), Some(second_target_id), Some(first_role), Some(second_role)) = (
            draft.target_ids[0],
            draft.target_ids[1],
            draft.guessed_roles[0],
            draft.guessed_roles[1],
        ) else {
            send_component_private(
                ctx,
                component,
                "청부 대상 2명과 각 대상의 직업을 모두 선택하세요.",
            )
            .await?;
            return Ok(());
        };
        let message = match running_write.game.submit_contractor_contract(
            actor_id,
            first_target_id,
            first_role,
            second_target_id,
            second_role,
        ) {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        running_write.contractor_contract_drafts.remove(&actor_id);
        let newly_contacted_mafia = running_write
            .game
            .get_player(actor_id)
            .filter(|actor| {
                actor.alive
                    && !was_known_mafia_team
                    && running_write.game.is_known_mafia_team(actor)
            })
            .cloned();
        let done = running_write.game.should_finish_night_early();
        (message, done, newly_contacted_mafia)
    };
    if let Some(player) = &newly_contacted_mafia {
        grant_private_role_member_access(ctx, data, &running, Role::Mafia, player).await;
    }
    if done {
        running.read().await.night_notify.notify_waiters();
    }
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        message,
                        "밤 행동 완료",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(vec![]),
            ),
        )
        .await?;
    if running.read().await.night_timed_events_due {
        trigger_timed_night_events(ctx, data, &running).await?;
    }
    Ok(())
}

async fn handle_skip_day(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let user_id = component.user.id.get();
    let outcome = {
        let mut running_write = running.write().await;
        if running_write.game.phase != Phase::Day {
            return send_component_private(ctx, component, "지금 진행 중인 낮 토론이 없습니다.")
                .await
                .map_err(Into::into);
        }
        let alive_ids = running_write
            .game
            .alive_players()
            .into_iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        if !alive_ids.contains(&user_id) {
            return send_component_private(
                ctx,
                component,
                "생존 중인 참가자만 바로 투표를 선택할 수 있습니다.",
            )
            .await
            .map_err(Into::into);
        }
        let required_votes = alive_ids.len() / 2 + 1;
        if running_write.day_skip_voter_ids.contains(&user_id) {
            return send_component_private(
                ctx,
                component,
                format!(
                    "이미 바로 투표에 동의했습니다. 현재 {}/{}명",
                    running_write.day_skip_voter_ids.len(),
                    required_votes
                ),
            )
            .await
            .map_err(Into::into);
        }
        running_write.day_skip_voter_ids.insert(user_id);
        let vote_count = running_write.day_skip_voter_ids.len();
        if vote_count < required_votes {
            return send_component_private(
                ctx,
                component,
                format!("바로 투표에 동의했습니다. 현재 {vote_count}/{required_votes}명"),
            )
            .await
            .map_err(Into::into);
        }
        running_write.day_skip_confirmed = true;
        running_write.day_extension_active = false;
        (
            vote_count,
            alive_ids.len(),
            running_write.day_notify.clone(),
            running_write.guild_id,
        )
    };
    let (vote_count, alive_count, notify, guild_id) = outcome;
    notify.notify_waiters();
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        format!(
                            "생존자 과반수가 바로 투표를 선택했습니다. ({vote_count}/{alive_count}명)\n토론을 끝내고 바로 지목 투표로 넘어갑니다."
                        ),
                        "바로 투표",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(day_skip_components(guild_id, true, true)),
            ),
        )
        .await?;
    if running.read().await.night_timed_events_due {
        trigger_timed_night_events(ctx, data, &running).await?;
    }
    Ok(())
}

async fn handle_day_extension(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let user_id = component.user.id.get();
    let outcome = {
        let mut running_write = running.write().await;
        if !running_write.day_extension_active {
            return send_component_private(ctx, component, "연장 투표가 종료되었습니다.")
                .await
                .map_err(Into::into);
        }
        if running_write.game.phase != Phase::Day {
            return send_component_private(ctx, component, "지금 진행 중인 낮 토론이 없습니다.")
                .await
                .map_err(Into::into);
        }
        let alive_ids = running_write
            .game
            .alive_players()
            .into_iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        if !alive_ids.contains(&user_id) {
            return send_component_private(
                ctx,
                component,
                "생존 중인 참가자만 연장 투표를 할 수 있습니다.",
            )
            .await
            .map_err(Into::into);
        }
        let required_votes = alive_ids.len() / 2 + 1;
        if running_write.day_extension_voter_ids.contains(&user_id) {
            return send_component_private(
                ctx,
                component,
                format!(
                    "이미 1분 연장에 투표했습니다. 현재 {}/{}명",
                    running_write.day_extension_voter_ids.len(),
                    required_votes
                ),
            )
            .await
            .map_err(Into::into);
        }
        running_write.day_extension_voter_ids.insert(user_id);
        let vote_count = running_write.day_extension_voter_ids.len();
        if vote_count < required_votes {
            return send_component_private(
                ctx,
                component,
                format!("1분 연장에 투표했습니다. 현재 {vote_count}/{required_votes}명"),
            )
            .await
            .map_err(Into::into);
        }
        running_write.day_extension_confirmed = true;
        running_write.day_extension_active = false;
        (
            vote_count,
            alive_ids.len(),
            running_write.day_notify.clone(),
            running_write.guild_id,
        )
    };
    let (vote_count, alive_count, notify, guild_id) = outcome;
    notify.notify_waiters();
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        format!(
                            "생존자 과반수가 1분 연장을 선택했습니다. ({vote_count}/{alive_count}명)\n낮 토론을 1분 연장합니다."
                        ),
                        "낮 토론 연장",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(day_extension_components(guild_id, true, true)),
            ),
        )
        .await?;
    if running.read().await.night_timed_events_due {
        trigger_timed_night_events(ctx, data, &running).await?;
    }
    Ok(())
}

async fn handle_join(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(recruitment) = data.recruitments.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "참가자 모집이 종료되었습니다.").await?;
        return Ok(());
    };
    let mut rec = recruitment.write().await;
    if !rec.accepting {
        send_component_private(ctx, component, "참가자 모집이 종료되었습니다.").await?;
        return Ok(());
    }
    let user_id = component.user.id.get();
    let config_snapshot = data.config.read().await;
    if is_blacklisted(&config_snapshot, user_id) {
        send_component_private(
            ctx,
            component,
            "블랙리스트에 등록된 유저는 참가할 수 없습니다.",
        )
        .await?;
        return Ok(());
    }
    drop(config_snapshot);
    if rec.joined_ids.contains(&user_id) {
        send_component_private(ctx, component, "이미 참가했습니다.").await?;
        return Ok(());
    }
    if rec.spectator_ids.contains(&user_id) {
        send_component_private(ctx, component, "이미 관전자로 등록되어 있습니다.").await?;
        return Ok(());
    }
    if rec.joined_ids.len() >= rec.max_players {
        send_component_private(
            ctx,
            component,
            format!(
                "최대 참가 인원 {}명에 도달해 더 이상 참가할 수 없습니다.",
                rec.max_players
            ),
        )
        .await?;
        return Ok(());
    }
    if let Some(member) = component.member.clone() {
        let _ = member.add_role(ctx, rec.participant_role_id).await;
        rec.joined_names.insert(user_id, display_name(&member));
    } else {
        rec.joined_names
            .insert(user_id, component.user.name.clone());
    }
    rec.joined_ids.insert(user_id);
    let updated = rec.clone();
    drop(rec);
    send_component_private(ctx, component, "참가 완료!").await?;
    update_recruitment_message(
        ctx,
        data,
        component,
        guild_id,
        &updated,
        RECRUITMENT_STATUS_OPEN,
        false,
    )
    .await;
    Ok(())
}

async fn handle_spectate(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(recruitment) = data.recruitments.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "참가자 모집이 종료되었습니다.").await?;
        return Ok(());
    };
    let mut rec = recruitment.write().await;
    if !rec.accepting {
        send_component_private(ctx, component, "참가자 모집이 종료되었습니다.").await?;
        return Ok(());
    }
    let user_id = component.user.id.get();
    if rec.joined_ids.contains(&user_id) {
        send_component_private(ctx, component, "이미 참가자로 등록되어 있습니다.").await?;
        return Ok(());
    }
    if rec.spectator_ids.contains(&user_id) {
        send_component_private(ctx, component, "이미 관전자로 등록되어 있습니다.").await?;
        return Ok(());
    }
    rec.spectator_ids.insert(user_id);
    if let Some(member) = component.member.clone() {
        rec.spectator_names.insert(user_id, display_name(&member));
        if let Some(role) = role_by_name(ctx, guild_id, SPECTATOR_ROLE).await? {
            let _ = member.add_role(ctx, role.id).await;
        }
    } else {
        rec.spectator_names
            .insert(user_id, component.user.name.clone());
    }
    let updated = rec.clone();
    drop(rec);
    send_component_private(ctx, component, "관전 등록 완료!").await?;
    update_recruitment_message(
        ctx,
        data,
        component,
        guild_id,
        &updated,
        RECRUITMENT_STATUS_OPEN,
        false,
    )
    .await;
    Ok(())
}

async fn handle_recruitment_finish(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    cancelled: bool,
) -> Result<()> {
    let Some(recruitment) = data.recruitments.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "참가자 모집이 이미 종료되었습니다.").await?;
        return Ok(());
    };
    let mut rec = recruitment.write().await;
    if component.user.id != rec.host_user_id {
        send_component_private(ctx, component, "게임을 모집한 주최자만 사용할 수 있습니다.")
            .await?;
        return Ok(());
    }
    if !cancelled && rec.joined_ids.len() < rec.minimum_players {
        send_component_private(
            ctx,
            component,
            format!(
                "아직 시작할 수 없습니다. 최소 {}명이 필요합니다. 현재 {}명입니다.",
                rec.minimum_players,
                rec.joined_ids.len()
            ),
        )
        .await?;
        return Ok(());
    }
    rec.cancelled = cancelled;
    rec.accepting = false;
    let updated = rec.clone();
    rec.done.notify_waiters();
    drop(rec);
    if cancelled {
        ack_component(ctx, component).await;
        update_recruitment_message(
            ctx,
            data,
            component,
            guild_id,
            &updated,
            RECRUITMENT_STATUS_CANCELLED,
            true,
        )
        .await;
    } else {
        ack_component(ctx, component).await;
    }
    Ok(())
}

async fn handle_night_action(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let values = selected_values(component);
    let target_id = values.first().and_then(|value| {
        if value == "skip" {
            None
        } else {
            value.parse().ok()
        }
    });
    let (
        message,
        done,
        mafia_action_view,
        spy_bonus_targets,
        newly_contacted_mafia,
        cult_bells,
        immediate_police_result,
        broadcast_police_result,
    ) = {
        let mut running_write = running.write().await;
        let was_known_mafia_team = running_write
            .game
            .get_player(actor_id)
            .is_some_and(|actor| running_write.game.is_known_mafia_team(actor));
        let message = match running_write.game.submit_night_action(actor_id, target_id) {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        let cult_bells = running_write.game.consume_cult_bells();
        let actor = running_write.game.get_player(actor_id).cloned();
        let is_police_action = actor.as_ref().is_some_and(|actor| {
            actor.role == Role::Police
                || (actor.role == Role::Thief
                    && running_write.game.thief_night_role(actor) == Some(Role::Police))
        });
        let (immediate_police_result, broadcast_police_result) = if is_police_action {
            if let Some(result) = running_write.game.consume_ready_police_result() {
                (Some(result.clone()), Some(result))
            } else {
                (
                    Some(
                        "다른 경찰의 선택이 남아 있어 조사 결과는 아직 확정되지 않았습니다."
                            .to_string(),
                    ),
                    None,
                )
            }
        } else {
            (None, None)
        };
        let newly_contacted_mafia = actor
            .as_ref()
            .filter(|actor| {
                actor.alive
                    && !was_known_mafia_team
                    && running_write.game.is_known_mafia_team(actor)
            })
            .cloned();
        let mafia_action_view = actor.as_ref().and_then(|actor| {
            let role = effective_night_role(&running_write.game, actor);
            if actor.role == Role::Mafia || (actor.role == Role::Thief && role == Role::Mafia) {
                Some((
                    night_targets(&running_write.game, actor),
                    mafia_night_target_status_text(&running_write),
                ))
            } else {
                None
            }
        });
        let spy_bonus_targets = actor.and_then(|actor| {
            if actor.role == Role::Spy && running_write.game.spy_can_use_bonus_action(actor_id) {
                Some(night_targets(&running_write.game, &actor))
            } else {
                None
            }
        });
        let done = running_write.game.should_finish_night_early();
        (
            message,
            done,
            mafia_action_view,
            spy_bonus_targets,
            newly_contacted_mafia,
            cult_bells,
            immediate_police_result,
            broadcast_police_result,
        )
    };
    if let Some(player) = &newly_contacted_mafia {
        grant_private_role_member_access(ctx, data, &running, Role::Mafia, player).await;
    }
    if let Some(result) = &broadcast_police_result {
        send_police_result_message(ctx, &running, result, Some(actor_id)).await;
    }
    let response_message = if let Some(result) = immediate_police_result {
        format!("{message}\n\n{result}")
    } else {
        message
    };
    if let Some((targets, status_text)) = mafia_action_view {
        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::UpdateMessage(
                    serenity::CreateInteractionResponseMessage::new()
                        .embed(make_embed(
                            format!("{response_message}\n\n{status_text}"),
                            "마피아 처치 선택",
                            serenity::Colour::DARK_GREEN,
                        ))
                        .components(night_action_components(
                            guild_id,
                            actor_id,
                            Role::Mafia,
                            &targets,
                        )),
                ),
            )
            .await?;
        upsert_private_role_status_message(ctx, &running, Role::Mafia).await;
        if running.read().await.night_timed_events_due {
            trigger_timed_night_events(ctx, data, &running).await?;
        }
        return Ok(());
    }
    if let Some(targets) = spy_bonus_targets {
        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::UpdateMessage(
                    serenity::CreateInteractionResponseMessage::new()
                        .embed(make_embed(
                            format!(
                                "{response_message}\n\n추가 첩보를 한 번 더 사용할 수 있습니다."
                            ),
                            "접선 성공",
                            serenity::Colour::DARK_GREEN,
                        ))
                        .components(night_action_components(
                            guild_id,
                            actor_id,
                            Role::Spy,
                            &targets,
                        )),
                ),
            )
            .await?;
        if running.read().await.night_timed_events_due {
            trigger_timed_night_events(ctx, data, &running).await?;
        }
        return Ok(());
    }
    if done {
        running.read().await.night_notify.notify_waiters();
    }
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        response_message,
                        "밤 행동 완료",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(vec![]),
            ),
        )
        .await?;
    if running.read().await.night_timed_events_due {
        trigger_timed_night_events(ctx, data, &running).await?;
    }
    if cult_bells > 0 {
        send_game_embed(
            ctx,
            &running,
            std::iter::repeat_n("교주의 종소리가 울렸습니다.", cult_bells as usize)
                .collect::<Vec<_>>()
                .join("\n"),
            "교주 포교",
            serenity::Colour::ORANGE,
            vec![],
            true,
            true,
        )
        .await?;
        sync_cult_team_channel_access(ctx, data, &running).await;
    }
    Ok(())
}

async fn handle_day_vote(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let values = selected_values(component);
    let target_id = values.first().and_then(|value| {
        if value == "skip" {
            None
        } else {
            value.parse().ok()
        }
    });
    let (message, done) = {
        let mut running_write = running.write().await;
        let message = match running_write
            .game
            .submit_day_vote(component.user.id.get(), target_id)
        {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        (message, running_write.game.all_day_votes_submitted())
    };
    if done {
        running.read().await.vote_notify.notify_waiters();
    }
    send_component_private(ctx, component, message).await?;
    Ok(())
}

async fn handle_confirm_vote(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    approve: bool,
) -> Result<()> {
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let (message, done) = {
        let mut running_write = running.write().await;
        let message = match running_write
            .game
            .submit_confirmation_vote(component.user.id.get(), approve)
        {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        (message, running_write.game.all_confirm_votes_submitted())
    };
    if done {
        running.read().await.confirm_notify.notify_waiters();
    }
    send_component_private(ctx, component, message).await?;
    Ok(())
}

async fn handle_hacker(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    let value = selected_values(component)
        .first()
        .and_then(|v| v.parse().ok());
    handle_day_action(
        ctx,
        data,
        component,
        guild_id,
        actor_id,
        value,
        "해킹 완료",
        |game, actor, target| game.submit_hacker_action(actor, target),
        |_, _, message| format!("{message}\n밤이 시작될 때 대상의 직업을 확인합니다."),
    )
    .await
}

async fn handle_vigilante(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    let value = selected_values(component)
        .first()
        .and_then(|v| v.parse().ok());
    handle_day_action(
        ctx,
        data,
        component,
        guild_id,
        actor_id,
        value,
        "숙청 조사 완료",
        |game, actor, target| game.submit_vigilante_investigation(actor, target),
        |game, actor, message| {
            let investigation = game
                .consume_vigilante_results()
                .remove(&actor)
                .unwrap_or_else(|| "조사 결과를 확인하지 못했습니다.".to_string());
            format!("{message}\n\n{investigation}")
        },
    )
    .await
}

async fn handle_thief(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    let value = selected_values(component)
        .first()
        .and_then(|v| v.parse().ok());
    handle_day_action(
        ctx,
        data,
        component,
        guild_id,
        actor_id,
        value,
        "도벽 완료",
        |game, actor, target| game.submit_thief_steal(actor, target),
        |_, _, message| message,
    )
    .await
}

async fn handle_psychologist(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let values = selected_values(component);
    if values.len() < 2 {
        send_component_private(ctx, component, "서로 다른 두 명을 선택해야 합니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let (Some(first), Some(second)) = (
        values.first().and_then(|value| value.parse().ok()),
        values.get(1).and_then(|value| value.parse().ok()),
    ) else {
        ack_component(ctx, component).await;
        return Ok(());
    };
    let message = {
        let mut running_write = running.write().await;
        match running_write
            .game
            .submit_psychologist_observation(actor_id, first, second)
        {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        }
    };
    ack_component(ctx, component).await;
    component
        .channel_id
        .edit_message(
            &ctx.http,
            component.message.id,
            serenity::EditMessage::new()
                .embed(make_embed(
                    message,
                    "심리학자 관찰 완료",
                    serenity::Colour::DARK_GREEN,
                ))
                .components(vec![]),
        )
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_day_action<F, G>(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
    target_id: Option<u64>,
    title: &'static str,
    apply: F,
    finish_message: G,
) -> Result<()>
where
    F: FnOnce(&mut MafiaGame, u64, u64) -> Result<String>,
    G: FnOnce(&mut MafiaGame, u64, String) -> String,
{
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let Some(target_id) = target_id else {
        send_component_private(ctx, component, "대상을 선택해야 합니다.").await?;
        return Ok(());
    };
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let (message, newly_contacted_mafia) = {
        let mut running_write = running.write().await;
        let was_known_mafia_team = running_write
            .game
            .get_player(actor_id)
            .is_some_and(|actor| running_write.game.is_known_mafia_team(actor));
        let message = match apply(&mut running_write.game, actor_id, target_id) {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        let message = finish_message(&mut running_write.game, actor_id, message);
        let newly_contacted_mafia = running_write
            .game
            .get_player(actor_id)
            .filter(|actor| {
                actor.alive
                    && !was_known_mafia_team
                    && running_write.game.is_known_mafia_team(actor)
            })
            .cloned();
        (message, newly_contacted_mafia)
    };
    if let Some(player) = &newly_contacted_mafia {
        grant_private_role_member_access(ctx, data, &running, Role::Mafia, player).await;
    }
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(message, title, serenity::Colour::DARK_GREEN))
                    .components(vec![]),
            ),
        )
        .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아중지",
    description_localized("ko", "진행 중인 마피아 게임을 중지합니다.")
)]
async fn stop_game(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if let Some((_id, running)) = ctx.data().games.remove(&guild_id) {
        let (roles, notifies) = {
            let mut running_write = running.write().await;
            running_write.game.phase = Phase::Ended;
            (
                running_write.game.reveal_roles(),
                [
                    running_write.night_notify.clone(),
                    running_write.vote_notify.clone(),
                    running_write.confirm_notify.clone(),
                    running_write.day_notify.clone(),
                ],
            )
        };
        for notify in notifies {
            notify.notify_waiters();
        }
        send_game_embed(
            ctx.serenity_context(),
            &running,
            format!("관리자가 게임을 중지했습니다.\n\n최종 역할\n{roles}"),
            "게임 중지",
            serenity::Colour::RED,
            vec![],
            true,
            true,
        )
        .await?;
        cleanup_game(ctx.serenity_context(), ctx.data(), &running).await;
        reply_embed(
            ctx,
            "게임을 중지했습니다.",
            "게임 중지",
            serenity::Colour::DARK_GREEN,
            false,
        )
        .await?;
    } else {
        reply_embed(
            ctx,
            "진행 중인 게임이 없습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
    }
    Ok(())
}

async fn show_public_status_impl(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let Some(running) = ctx.data().games.get(&guild_id).map(|entry| entry.clone()) else {
        reply_embed(
            ctx,
            "진행 중인 게임이 없습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let (text, ephemeral) = {
        let running_read = running.read().await;
        (
            command_status_text(&running_read, ctx.author().id.get()),
            running_read.anonymous_enabled
                && running_read
                    .game
                    .get_player(ctx.author().id.get())
                    .is_some(),
        )
    };
    reply_embed(ctx, text, "게임 현황", serenity::Colour::GOLD, ephemeral).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "상태",
    description_localized("ko", "현재 마피아 게임 생존자와 사망자를 확인합니다.")
)]
async fn show_public_status(ctx: Context<'_>) -> Result<(), Error> {
    show_public_status_impl(ctx).await
}

#[poise::command(
    slash_command,
    rename = "마피아상태",
    description_localized("ko", "진행 중인 마피아 게임 상태를 확인합니다.")
)]
async fn show_manager_status(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let Some(running) = ctx.data().games.get(&guild_id).map(|entry| entry.clone()) else {
        reply_embed(
            ctx,
            "진행 중인 게임이 없습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let text = running.read().await.game.public_status();
    reply_embed(ctx, text, "게임 상태", serenity::Colour::GOLD, true).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "메모",
    description_localized("ko", "개인 메모 채널에 참가자별 메모를 저장하거나 조회합니다.")
)]
async fn memo(
    ctx: Context<'_>,
    #[description = "메모 대상 참가자"] 참가자: serenity::User,
    #[description = "저장할 메모 내용. 비워두면 조회합니다."] 메모내용: Option<String>,
) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "메모",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let Some(running) = ctx.data().games.get(&guild_id).map(|entry| entry.clone()) else {
        reply_embed(
            ctx,
            "진행 중인 게임이 없습니다.",
            "메모",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let author_id = ctx.author().id.get();
    let (author, target, channel_id) = {
        let running_read = running.read().await;
        let Some(author) = running_read.game.get_player(author_id).cloned() else {
            reply_embed(
                ctx,
                "현재 게임 참가자만 메모를 사용할 수 있습니다.",
                "메모",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        };
        let Some(target) = running_read.game.get_player(참가자.id.get()).cloned() else {
            reply_embed(
                ctx,
                "메모 대상은 현재 게임 참가자여야 합니다.",
                "메모",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        };
        (author, target, running_read.channel_id)
    };

    let config = ctx.data().config.read().await.clone();
    let roles = channel_role_ids(
        ctx.serenity_context(),
        guild_id,
        &config,
        ctx.data().bot_user_id,
    )
    .await?;
    let category = source_category(ctx.serenity_context(), channel_id).await;
    let Some(memo_channel_id) =
        ensure_memo_channel(ctx.serenity_context(), &running, &author, roles, category).await
    else {
        reply_embed(
            ctx,
            "개인 메모 채널을 만들 수 없습니다.",
            "메모",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };

    let content = 메모내용.unwrap_or_default().trim().to_string();
    if !content.is_empty() {
        let (memo_number, target_name) = {
            let mut running_write = running.write().await;
            let target_name = running_write
                .game
                .get_player(target.user_id)
                .map(|target| status_display_name(&running_write, target))
                .unwrap_or_else(|| target.name.clone());
            let memos = running_write
                .memos
                .entry(author_id)
                .or_default()
                .entry(target.user_id)
                .or_default();
            memos.push(content.clone());
            (memos.len(), target_name)
        };
        let _ = send_channel_embed(
            ctx.http(),
            memo_channel_id,
            format!("대상: {target_name}\n{memo_number}. {content}"),
            "메모 등록",
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await;
        reply_embed(
            ctx,
            format!("{target_name} 님에 대한 메모를 저장했습니다."),
            "메모 등록",
            serenity::Colour::DARK_GREEN,
            true,
        )
        .await?;
    } else {
        let chunks = {
            let running_read = running.read().await;
            let target_name = running_read
                .game
                .get_player(target.user_id)
                .map(|target| status_display_name(&running_read, target))
                .unwrap_or_else(|| target.name.clone());
            let memos = running_read
                .memos
                .get(&author_id)
                .and_then(|target_memos| target_memos.get(&target.user_id))
                .cloned()
                .unwrap_or_default();
            let header = format!("{target_name} 님에 대한 메모");
            if memos.is_empty() {
                vec![format!("{header}\n저장된 메모가 없습니다.")]
            } else {
                let mut chunks = Vec::new();
                let mut current = header.clone();
                for (index, memo) in memos.iter().enumerate() {
                    let line = format!("{}. {memo}", index + 1);
                    if current.len() + line.len() + 1 > 3500 {
                        chunks.push(current);
                        current = format!("{header} (계속)\n{line}");
                    } else {
                        current.push('\n');
                        current.push_str(&line);
                    }
                }
                chunks.push(current);
                chunks
            }
        };
        for chunk in chunks {
            ctx.send(
                poise::CreateReply::default()
                    .embed(make_embed(chunk, "메모 조회", serenity::Colour::GOLD))
                    .ephemeral(true),
            )
            .await?;
        }
    }
    Ok(())
}

fn personal_stats_text(stats_file: &stats::StatsFile, user_id: u64, fallback_name: &str) -> String {
    let Some(entry) = stats_file.users.get(&user_id.to_string()) else {
        return "아직 기록된 게임 전적이 없습니다.".to_string();
    };
    let name = if entry.name.is_empty() {
        fallback_name
    } else {
        &entry.name
    };
    format!(
        "{name}님의 전적\n전체 게임: **{}판**\n승리/패배: **{}승 {}패**\n승률: **{}**\n마피아팀 플레이: **{}회**\n게임시간: **{}**\n레이팅: **{}점** (최고 {}점, 반영 {}판)\n\n역할별 플레이\n{}",
        entry.games,
        entry.wins,
        entry.losses,
        stats::win_rate_text(entry.wins, entry.games),
        entry.mafia_team_games,
        stats::play_duration_text(entry.play_seconds),
        entry.rating,
        entry.rating_peak,
        entry.rating_games,
        stats::role_stats_text(entry)
    )
}

#[poise::command(
    slash_command,
    rename = "내정보",
    description_localized("ko", "내 마피아 게임 전적을 확인합니다.")
)]
async fn show_my_info(ctx: Context<'_>) -> Result<(), Error> {
    let stats_file = ctx.data().stats.read().await;
    let user = ctx.author();
    let text = personal_stats_text(&stats_file, user.id.get(), &user.name);
    reply_embed(ctx, text, "내정보", serenity::Colour::GOLD, true).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "레이팅로그",
    description_localized("ko", "내 최근 레이팅 변화 기록을 확인합니다.")
)]
async fn rating_log(ctx: Context<'_>) -> Result<(), Error> {
    let stats_file = ctx.data().stats.read().await;
    let user = ctx.author();
    let text = stats::rating_log_text(&stats_file, user.id.get(), &user.name, 10);
    reply_embed(ctx, text, "레이팅 로그", serenity::Colour::GOLD, true).await?;
    Ok(())
}

fn image_color(hex: &str) -> Rgb<u8> {
    let value = hex.trim_start_matches('#');
    let red = u8::from_str_radix(&value[0..2], 16).unwrap_or(255);
    let green = u8::from_str_radix(&value[2..4], 16).unwrap_or(255);
    let blue = u8::from_str_radix(&value[4..6], 16).unwrap_or(255);
    Rgb([red, green, blue])
}

fn fill_rect(image: &mut RgbImage, x: i32, y: i32, width: u32, height: u32, color: Rgb<u8>) {
    let left = x.max(0) as u32;
    let top = y.max(0) as u32;
    let right = (x as i64 + width as i64)
        .clamp(0, image.width() as i64)
        .max(left as i64) as u32;
    let bottom = (y as i64 + height as i64)
        .clamp(0, image.height() as i64)
        .max(top as i64) as u32;

    for pixel_y in top..bottom {
        for pixel_x in left..right {
            image.put_pixel(pixel_x, pixel_y, color);
        }
    }
}

fn fill_horizontal_line(image: &mut RgbImage, x0: i32, x1: i32, y: i32, color: Rgb<u8>) {
    if y < 0 || y >= image.height() as i32 {
        return;
    }
    let left = x0.min(x1).max(0) as u32;
    let right = x0.max(x1).min(image.width() as i32 - 1);
    if right < 0 || left > right as u32 {
        return;
    }
    for pixel_x in left..=right as u32 {
        image.put_pixel(pixel_x, y as u32, color);
    }
}

fn fill_circle(image: &mut RgbImage, center: (i32, i32), radius: i32, color: Rgb<u8>) {
    let mut x = 0;
    let mut y = radius;
    let mut p = 1 - radius;
    let (x0, y0) = center;

    while x <= y {
        fill_horizontal_line(image, x0 - x, x0 + x, y0 + y, color);
        fill_horizontal_line(image, x0 - y, x0 + y, y0 + x, color);
        fill_horizontal_line(image, x0 - x, x0 + x, y0 - y, color);
        fill_horizontal_line(image, x0 - y, x0 + y, y0 - x, color);

        x += 1;
        if p < 0 {
            p += 2 * x + 1;
        } else {
            y -= 1;
            p += 2 * (x - y) + 1;
        }
    }
}

fn blend_channel(left: u8, right: u8, left_weight: f32, right_weight: f32) -> u8 {
    let value = left as f32 * left_weight + right as f32 * right_weight;
    if value < u8::MAX as f32 {
        if value > u8::MIN as f32 {
            value as u8
        } else {
            u8::MIN
        }
    } else {
        u8::MAX
    }
}

fn blend_rgb(left: Rgb<u8>, right: Rgb<u8>, left_weight: f32, right_weight: f32) -> Rgb<u8> {
    Rgb([
        blend_channel(left[0], right[0], left_weight, right_weight),
        blend_channel(left[1], right[1], left_weight, right_weight),
        blend_channel(left[2], right[2], left_weight, right_weight),
    ])
}

fn layout_lb_glyphs(
    scale: PxScale,
    font: &impl Font,
    text: &str,
    mut visit: impl FnMut(OutlinedGlyph, GlyphRect),
) {
    let font = font.as_scaled(scale);
    let mut last: Option<GlyphId> = None;
    let mut width = 0.0;

    for character in text.chars() {
        let glyph_id = font.glyph_id(character);
        let glyph = glyph_id.with_scale_and_position(scale, point(width, font.ascent()));
        width += font.h_advance(glyph_id);
        if let Some(outlined) = font.outline_glyph(glyph) {
            if let Some(last) = last {
                width += font.kern(glyph_id, last);
            }
            last = Some(glyph_id);
            let bounds = outlined.px_bounds();
            visit(outlined, bounds);
        }
    }
}

fn draw_lb_text(
    image: &mut RgbImage,
    font: &FontArc,
    size: f32,
    x: i32,
    y: i32,
    text: impl AsRef<str>,
    color: Rgb<u8>,
) {
    let image_width = image.width() as i32;
    let image_height = image.height() as i32;

    layout_lb_glyphs(PxScale::from(size), font, text.as_ref(), |glyph, bounds| {
        glyph.draw(|glyph_x, glyph_y, value| {
            let image_x = glyph_x as i32 + x + bounds.min.x.round() as i32;
            let image_y = glyph_y as i32 + y + bounds.min.y.round() as i32;
            let value = value.clamp(0.0, 1.0);

            if (0..image_width).contains(&image_x) && (0..image_height).contains(&image_y) {
                let pixel = *image.get_pixel(image_x as u32, image_y as u32);
                image.put_pixel(
                    image_x as u32,
                    image_y as u32,
                    blend_rgb(pixel, color, 1.0 - value, value),
                );
            }
        });
    });
}

fn truncate_for_board(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut text = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    text.push_str("...");
    text
}

fn leaderboard_metric_column(metric: &str) -> &'static str {
    match metric {
        "winrate" => "winrate",
        "games" => "games",
        "mafia" => "mafia",
        "playtime" => "time",
        "rating" => "rating",
        _ => "record",
    }
}

fn render_leaderboard_image(stats_file: &stats::StatsFile, metric: &str) -> Option<Vec<u8>> {
    let entries = stats::leaderboard_entries(stats_file, metric, 10);
    if entries.is_empty() {
        return None;
    }

    const IMAGE_WIDTH: u32 = 1280;
    const TOP_PADDING: i32 = 40;
    const SIDE_PADDING: i32 = 48;
    const HEADER_HEIGHT: i32 = 150;
    const ROW_HEIGHT: i32 = 78;
    const BOTTOM_PADDING: i32 = 44;

    let height =
        (TOP_PADDING + HEADER_HEIGHT + ROW_HEIGHT * entries.len() as i32 + BOTTOM_PADDING) as u32;
    let mut image = RgbImage::from_pixel(IMAGE_WIDTH, height, image_color("#111318"));
    let font = FontArc::try_from_slice(include_bytes!("../MalangmalangR.ttf")).ok()?;

    let text = image_color("#f5f7fb");
    let muted = image_color("#aeb6c8");
    let accent = image_color("#ffd166");
    let panel = image_color("#1d2028");
    let row_dark = image_color("#242832");
    let row_light = image_color("#292e3a");

    draw_lb_text(
        &mut image,
        &font,
        44.0,
        SIDE_PADDING,
        TOP_PADDING,
        "마피아 리더보드",
        text,
    );
    draw_lb_text(
        &mut image,
        &font,
        24.0,
        SIDE_PADDING,
        TOP_PADDING + 58,
        "게임 종료 후 기록된 전적 기준",
        muted,
    );
    fill_rect(
        &mut image,
        IMAGE_WIDTH as i32 - SIDE_PADDING - 230,
        TOP_PADDING + 10,
        210,
        38,
        image_color("#374151"),
    );
    draw_lb_text(
        &mut image,
        &font,
        24.0,
        IMAGE_WIDTH as i32 - SIDE_PADDING - 214,
        TOP_PADDING + 16,
        format!("기준: {}", stats::leaderboard_metric_name(metric)),
        text,
    );

    let panel_top = TOP_PADDING + 116;
    let panel_bottom = height as i32 - BOTTOM_PADDING + 8;
    fill_rect(
        &mut image,
        SIDE_PADDING,
        panel_top,
        IMAGE_WIDTH - (SIDE_PADDING as u32 * 2),
        (panel_bottom - panel_top) as u32,
        panel,
    );

    let columns = HashMap::from([
        ("rank", SIDE_PADDING + 32),
        ("name", SIDE_PADDING + 110),
        ("record", SIDE_PADDING + 410),
        ("games", SIDE_PADDING + 555),
        ("winrate", SIDE_PADDING + 665),
        ("mafia", SIDE_PADDING + 800),
        ("time", SIDE_PADDING + 930),
        ("rating", SIDE_PADDING + 1085),
    ]);
    let selected_column = leaderboard_metric_column(metric);
    let header_y = panel_top + 24;
    for (key, label) in [
        ("rank", "#"),
        ("name", "이름"),
        ("record", "승패"),
        ("games", "판수"),
        ("winrate", "승률"),
        ("mafia", "마피아"),
        ("time", "시간"),
        ("rating", "레이팅"),
    ] {
        draw_lb_text(
            &mut image,
            &font,
            21.0,
            columns[key],
            header_y,
            label,
            if key == selected_column {
                accent
            } else {
                muted
            },
        );
    }

    let row_start_y = panel_top + 62;
    for (index, (_user_id, entry)) in entries.iter().enumerate() {
        let rank = index + 1;
        let y = row_start_y + index as i32 * ROW_HEIGHT;
        let row_fill = if rank % 2 == 1 { row_dark } else { row_light };
        fill_rect(
            &mut image,
            SIDE_PADDING + 18,
            y,
            IMAGE_WIDTH - ((SIDE_PADDING + 18) as u32 * 2),
            (ROW_HEIGHT - 10) as u32,
            row_fill,
        );
        let medal = match rank {
            1 => image_color("#f6c945"),
            2 => image_color("#c4ccd8"),
            3 => image_color("#c58b5b"),
            _ => image_color("#3b4252"),
        };
        fill_circle(&mut image, (columns["rank"] + 17, y + 36), 20, medal);
        draw_lb_text(
            &mut image,
            &font,
            24.0,
            columns["rank"] + if rank < 10 { 9 } else { 3 },
            y + 22,
            rank.to_string(),
            if rank <= 3 {
                image_color("#111318")
            } else {
                text
            },
        );

        let name = if entry.name.is_empty() {
            "알 수 없음".to_string()
        } else {
            truncate_for_board(&entry.name, 13)
        };
        let values = [
            ("name", name),
            ("record", format!("{}승 {}패", entry.wins, entry.losses)),
            ("games", format!("{}판", entry.games)),
            ("winrate", stats::win_rate_text(entry.wins, entry.games)),
            ("mafia", format!("{}회", entry.mafia_team_games)),
            ("time", stats::play_duration_text(entry.play_seconds)),
            ("rating", format!("{}점", entry.rating)),
        ];
        for (key, value) in values {
            draw_lb_text(
                &mut image,
                &font,
                if key == "name" { 27.0 } else { 23.0 },
                columns[key],
                y + if key == "name" { 18 } else { 21 },
                value,
                if key == selected_column { accent } else { text },
            );
        }
    }
    draw_lb_text(
        &mut image,
        &font,
        18.0,
        SIDE_PADDING + 18,
        height as i32 - 30,
        "마피아 게임 진행 메시지",
        muted,
    );

    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .ok()?;
    Some(bytes.into_inner())
}

#[poise::command(
    slash_command,
    rename = "리더보드",
    description_localized("ko", "마피아 게임 전적 순위를 확인합니다.")
)]
async fn show_leaderboard(
    ctx: Context<'_>,
    #[description = "정렬 기준"] 기준: Option<LeaderboardMetric>,
) -> Result<(), Error> {
    let metric = 기준.map_or("wins", LeaderboardMetric::value);
    let stats_file = ctx.data().stats.read().await;
    if let Some(image) = render_leaderboard_image(&stats_file, metric) {
        ctx.send(
            poise::CreateReply::default().attachment(serenity::CreateAttachment::bytes(
                image,
                format!("mafia_leaderboard_{metric}.png"),
            )),
        )
        .await?;
        return Ok(());
    }
    let text = stats::leaderboard_text(&stats_file, metric);
    reply_embed(ctx, text, "리더보드", serenity::Colour::GOLD, false).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "리더보드초기화",
    description_localized("ko", "마피아 게임 전적과 리더보드를 초기화합니다.")
)]
async fn reset_leaderboard(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut stats_file = ctx.data().stats.write().await;
    *stats_file = stats::StatsFile::default();
    stats::save_stats(&*ctx.data().stats_path, &stats_file)?;
    reply_embed(
        ctx,
        "리더보드와 개인 전적을 초기화했습니다.",
        "리더보드",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아설정",
    description_localized("ko", "마피아 게임 기본 설정을 변경합니다.")
)]
#[allow(clippy::too_many_arguments)]
async fn configure_game(
    ctx: Context<'_>,
    #[description = "마피아 수"] mafia: Option<u32>,
    #[description = "의사 수"] doctor: Option<u32>,
    #[description = "경찰 수"] police: Option<u32>,
    #[description = "시민 특수룰 수"] citizen_special: Option<u32>,
    #[description = "마피아 특수룰 수"] mafia_special: Option<u32>,
    #[description = "중립 특수룰 수"] neutral_special: Option<u32>,
    #[description = "낮 채팅 슬로우모드 초. 기본 3초"] slowmode: Option<u64>,
    #[description = "사망 시 직업 공개 여부"] death_role_reveal: Option<bool>,
    #[description = "낮에 경찰 조사 성공 여부 공개 여부"] police_status_reveal: Option<bool>,
    #[description = "아침 생존 마피아 수 공개 여부"] mafia_count_reveal: Option<bool>,
    #[description = "사립탐정 활성화 여부"] detective: Option<bool>,
    #[description = "영매 활성화 여부"] shaman: Option<bool>,
    #[description = "도굴꾼 활성화 여부"] graverobber: Option<bool>,
    #[description = "스파이 활성화 여부"] spy: Option<bool>,
    #[description = "청부업자 활성화 여부"] contractor: Option<bool>,
    #[description = "마녀 활성화 여부"] witch: Option<bool>,
    #[description = "과학자 활성화 여부"] scientist: Option<bool>,
    #[description = "대부 활성화 여부"] godfather: Option<bool>,
    #[description = "조커 활성화 여부"] joker: Option<bool>,
    #[description = "정치인 활성화 여부"] politician: Option<bool>,
    #[description = "판사 활성화 여부"] judge: Option<bool>,
    #[description = "기자 활성화 여부"] reporter: Option<bool>,
    #[description = "해커 활성화 여부"] hacker: Option<bool>,
    #[description = "테러리스트 활성화 여부"] terrorist: Option<bool>,
    #[description = "군인 활성화 여부"] soldier: Option<bool>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    let previous = config_write.clone();
    if let Some(value) = mafia {
        if value < 1 {
            reply_embed(
                ctx,
                "마피아는 최소 1명이어야 합니다.",
                "설정 오류",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        }
        config_write.default_mafia_count = value;
    }
    if let Some(value) = doctor {
        config_write.default_doctor_count = value;
    }
    if let Some(value) = police {
        config_write.default_police_count = value;
    }
    if let Some(value) = citizen_special {
        config_write.citizen_special_count = value;
    }
    if let Some(value) = mafia_special {
        config_write.mafia_special_count = value;
    }
    if let Some(value) = neutral_special {
        config_write.neutral_special_count = value;
    }
    if let Some(value) = slowmode {
        config_write.chat_slowmode_seconds = value;
    }
    if let Some(value) = death_role_reveal {
        config_write.reveal_death_roles = value;
    }
    if let Some(value) = police_status_reveal {
        config_write.reveal_public_police_status = value;
    }
    if let Some(value) = mafia_count_reveal {
        config_write.reveal_morning_mafia_count = value;
    }
    if let Some(value) = detective {
        config_write.enable_detective = value;
    }
    if let Some(value) = shaman {
        config_write.enable_shaman = value;
    }
    if let Some(value) = graverobber {
        config_write.enable_graverobber = value;
    }
    if let Some(value) = spy {
        config_write.enable_spy = value;
    }
    if let Some(value) = contractor {
        config_write.enable_contractor = value;
    }
    if let Some(value) = witch {
        config_write.enable_witch = value;
    }
    if let Some(value) = scientist {
        config_write.enable_scientist = value;
    }
    if let Some(value) = godfather {
        config_write.enable_godfather = value;
    }
    if let Some(value) = joker {
        config_write.enable_joker = value;
    }
    if let Some(value) = politician {
        config_write.enable_politician = value;
    }
    if let Some(value) = judge {
        config_write.enable_judge = value;
    }
    if let Some(value) = reporter {
        config_write.enable_reporter = value;
    }
    if let Some(value) = hacker {
        config_write.enable_hacker = value;
    }
    if let Some(value) = terrorist {
        config_write.enable_terrorist = value;
    }
    if let Some(value) = soldier {
        config_write.enable_soldier = value;
    }
    let validation = choose_special_roles(&config_write)
        .and_then(|special_roles| selected_role_counts(&config_write, &special_roles))
        .map(|role_counts| {
            let minimum_players = minimum_player_count(&role_counts);
            let max_players = effective_max_player_count(&config_write);
            (minimum_players, max_players)
        });
    match validation {
        Ok((minimum_players, max_players)) if max_players < minimum_players => {
            *config_write = previous;
            reply_embed(
                ctx,
                format!("현재 설정의 최소 시작 인원은 {minimum_players}명이라 최대 인원 {max_players}명으로 시작할 수 없습니다."),
                "설정 오류",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        }
        Err(error) => {
            *config_write = previous;
            reply_embed(
                ctx,
                error.to_string(),
                "설정 오류",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        }
        _ => {}
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아인원설정",
    description_localized("ko", "마피아 게임 모집 최대 인원을 설정합니다.")
)]
async fn configure_player_limit(
    ctx: Context<'_>,
    #[description = "최대 참가 인원. 0은 제한 없음(봇 최대 24명)"] max_players: u32,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    if max_players as usize > MAX_GAME_PLAYERS {
        reply_embed(
            ctx,
            format!("최대 인원은 {MAX_GAME_PLAYERS}명 이하로 설정해야 합니다."),
            "설정 오류",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    config_write.max_player_count = max_players;
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 인원 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아익명설정",
    description_localized("ko", "마피아 게임 익명 채팅 사용 여부를 설정합니다.")
)]
async fn configure_anonymous_mode(
    ctx: Context<'_>,
    #[description = "익명 채팅 사용 여부"] enabled: bool,
    #[description = "익명 이름을 동물로 할지 숫자로 할지 선택합니다."] 이름방식: Option<
        AnonymousNameMode,
    >,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    config_write.anonymous_mode = enabled;
    if let Some(name_mode) = 이름방식 {
        config_write.anonymous_name_mode = name_mode.value().to_string();
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 익명 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아웹설정",
    description_localized(
        "ko",
        "브라우저에서 게임 설정을 편집할 수 있는 1회용 링크를 발급합니다. (관리자 전용)"
    )
)]
async fn web_configure_game(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "웹 설정",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let user = ctx.author();
    let token = web_settings::issue_session(
        &ctx.data().web_sessions,
        guild_id.get(),
        user.id.get(),
        user.name.clone(),
    );
    let url = format!(
        "{}{}/{}",
        ctx.data().web_base_url.trim_end_matches('/'),
        web_settings::settings_path(),
        token
    );
    let minutes = web_settings::session_ttl_minutes();
    reply_embed(
        ctx,
        format!(
            "아래 링크에서 마피아 게임 설정을 편집할 수 있습니다.\n{url}\n\n⚠️ 이 링크는 **{}** 님만 사용할 수 있고, **{minutes}분 동안 1회**만 유효합니다. 다른 사람과 공유하지 마세요.",
            user.name
        ),
        "웹 설정 링크 발급",
        serenity::Colour::DARK_GREEN,
        true,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아추가설정",
    description_localized("ko", "추가 역할 묶음을 설정합니다.")
)]
#[allow(clippy::too_many_arguments)]
async fn configure_extra_roles(
    ctx: Context<'_>,
    nurse: Option<bool>,
    lover: Option<bool>,
    priest: Option<bool>,
    madam: Option<bool>,
    gangster: Option<bool>,
    prophet: Option<bool>,
    psychologist: Option<bool>,
    thief: Option<bool>,
    cult_team: Option<bool>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    if let Some(v) = nurse {
        config_write.enable_nurse = v;
    }
    if let Some(v) = lover {
        config_write.enable_lover = v;
    }
    if let Some(v) = priest {
        config_write.enable_priest = v;
    }
    if let Some(v) = madam {
        config_write.enable_madam = v;
    }
    if let Some(v) = gangster {
        config_write.enable_gangster = v;
    }
    if let Some(v) = prophet {
        config_write.enable_prophet = v;
    }
    if let Some(v) = psychologist {
        config_write.enable_psychologist = v;
    }
    if let Some(v) = thief {
        config_write.enable_thief = v;
    }
    if let Some(v) = cult_team {
        config_write.enable_cult_team = v;
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 추가 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아수사설정",
    description_localized("ko", "수사직 후보를 설정합니다.")
)]
async fn configure_investigation_role(
    ctx: Context<'_>,
    agent: Option<bool>,
    vigilante: Option<bool>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    if let Some(v) = agent {
        config_write.use_agent = v;
    }
    if let Some(v) = vigilante {
        config_write.use_vigilante = v;
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 수사 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아비활성화",
    description_localized("ko", "마피아 게임 시작을 비활성화합니다.")
)]
async fn disable_mafia_game(ctx: Context<'_>) -> Result<(), Error> {
    set_game_enabled(ctx, false).await
}

#[poise::command(
    slash_command,
    rename = "마피아활성화",
    description_localized("ko", "마피아 게임 시작을 활성화합니다.")
)]
async fn enable_mafia_game(ctx: Context<'_>) -> Result<(), Error> {
    set_game_enabled(ctx, true).await
}

async fn set_game_enabled(ctx: Context<'_>, enabled: bool) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    config_write.game_enabled = enabled;
    config::save_config(&*ctx.data().config_path, &config_write)?;
    drop(config_write);
    reply_embed(
        ctx,
        if enabled {
            "마피아 게임을 활성화했습니다. 이제 새 게임을 시작할 수 있습니다."
        } else {
            "마피아 게임을 비활성화했습니다. 새 게임을 시작할 수 없습니다."
        },
        "마피아 게임",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "블랙리스트추가",
    description_localized("ko", "마피아 게임 참가 블랙리스트에 유저를 추가합니다.")
)]
async fn add_to_blacklist(
    ctx: Context<'_>,
    #[description = "블랙리스트에 추가할 유저"] 유저: serenity::User,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    let id = 유저.id.get();
    let changed = !config_write.blacklist_user_ids.contains(&id);
    if changed {
        config_write.blacklist_user_ids.push(id);
        config_write.blacklist_user_ids.sort_unstable();
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    drop(config_write);
    reply_embed(
        ctx,
        if changed {
            format!(
                "{} 님을 블랙리스트에 추가했습니다. 이제 게임에 참가할 수 없습니다.",
                유저.name
            )
        } else {
            format!("{} 님은 이미 블랙리스트에 있습니다.", 유저.name)
        },
        "블랙리스트",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "블랙리스트해제",
    description_localized("ko", "마피아 게임 참가 블랙리스트에서 유저를 제거합니다.")
)]
async fn remove_from_blacklist(
    ctx: Context<'_>,
    #[description = "블랙리스트에서 해제할 유저"] 유저: serenity::User,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    let id = 유저.id.get();
    let before = config_write.blacklist_user_ids.len();
    config_write
        .blacklist_user_ids
        .retain(|user_id| *user_id != id);
    let changed = config_write.blacklist_user_ids.len() != before;
    config::save_config(&*ctx.data().config_path, &config_write)?;
    drop(config_write);
    reply_embed(
        ctx,
        if changed {
            format!(
                "{} 님을 블랙리스트에서 해제했습니다. 이제 게임에 참가할 수 있습니다.",
                유저.name
            )
        } else {
            format!("{} 님은 블랙리스트에 없습니다.", 유저.name)
        },
        "블랙리스트",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "블랙리스트목록",
    description_localized("ko", "마피아 게임 참가 블랙리스트 목록을 확인합니다.")
)]
async fn show_blacklist(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let config_read = ctx.data().config.read().await;
    let text = if config_read.blacklist_user_ids.is_empty() {
        "블랙리스트가 비어 있습니다.".to_string()
    } else {
        config_read
            .blacklist_user_ids
            .iter()
            .take(50)
            .enumerate()
            .map(|(i, id)| format!("{}. `{id}`", i + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    drop(config_read);
    reply_embed(ctx, text, "블랙리스트", serenity::Colour::GOLD, true).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "직업정보",
    description_localized("ko", "특정 직업의 설명을 확인합니다.")
)]
async fn show_role_info(
    ctx: Context<'_>,
    #[description = "설명을 볼 직업 이름"] 직업명: String,
) -> Result<(), Error> {
    let role = find_role_by_name(&직업명);
    if let Some(role) = role {
        reply_embed(
            ctx,
            format!("{}\n{}", role.value(), role_short_guide(role)),
            "직업정보",
            serenity::Colour::DARK_GREEN,
            false,
        )
        .await?;
    } else {
        reply_embed(
            ctx,
            "직업을 찾을 수 없습니다. 정확한 직업명을 입력하세요.",
            "직업정보",
            serenity::Colour::RED,
            true,
        )
        .await?;
    }
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "역할설명",
    description_localized("ko", "마피아 게임 전체 역할 설명을 공지용 임베드로 보냅니다.")
)]
async fn show_role_descriptions(ctx: Context<'_>) -> Result<(), Error> {
    let mut lines = Vec::new();
    for role in [
        Role::Mafia,
        Role::Police,
        Role::Agent,
        Role::Vigilante,
        Role::Doctor,
        Role::Nurse,
        Role::Gangster,
        Role::Prophet,
        Role::Psychologist,
        Role::Detective,
        Role::Shaman,
        Role::Priest,
        Role::Graverobber,
        Role::Politician,
        Role::Judge,
        Role::Reporter,
        Role::Hacker,
        Role::Terrorist,
        Role::Lover,
        Role::Soldier,
        Role::Spy,
        Role::Contractor,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Madam,
        Role::Godfather,
        Role::CultLeader,
        Role::Fanatic,
        Role::Joker,
        Role::Citizen,
    ] {
        lines.push(format!("**{}** - {}", role.value(), role_short_guide(role)));
    }
    reply_embed(
        ctx,
        lines.join("\n"),
        "역할 설명",
        serenity::Colour::GOLD,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아능력",
    description_localized("ko", "배정받은 역할과 능력 설명을 다시 확인합니다.")
)]
async fn show_abilities(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "능력 설명",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let Some(running) = ctx.data().games.get(&guild_id).map(|entry| entry.clone()) else {
        reply_embed(
            ctx,
            "진행 중인 게임이 없습니다.",
            "능력 설명",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let running_read = running.read().await;
    let Some(player) = running_read.game.get_player(ctx.author().id.get()) else {
        reply_embed(
            ctx,
            "현재 게임 참가자만 능력 설명을 확인할 수 있습니다.",
            "능력 설명",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    reply_embed(
        ctx,
        role_message(&running_read.game, player),
        "능력 설명",
        serenity::Colour::GOLD,
        true,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "용어정보",
    description_localized("ko", "마피아 게임 용어 하나를 확인합니다.")
)]
async fn show_term_info(
    ctx: Context<'_>,
    #[description = "설명을 볼 용어"] 용어: String,
) -> Result<(), Error> {
    let Some(term) = find_term_by_name(&용어) else {
        reply_embed(
            ctx,
            "용어를 찾을 수 없습니다. 정확한 용어를 입력하세요.",
            "용어정보",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    reply_embed(
        ctx,
        format!("분류: {}\n\n{}", term.category, term_field_value(&term)),
        &format!("용어정보 - {}", term.names[0]),
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "용어설명",
    description_localized("ko", "마피아 게임 용어 설명을 공지용 임베드로 보냅니다.")
)]
async fn show_term_descriptions(ctx: Context<'_>) -> Result<(), Error> {
    for (index, (title, body)) in term_guide_pages().into_iter().enumerate() {
        if index == 0 {
            reply_embed(ctx, body, &title, serenity::Colour::GOLD, false).await?;
        } else {
            send_channel_embed(
                &ctx.serenity_context().http,
                ctx.channel_id(),
                body,
                &title,
                serenity::Colour::GOLD,
                vec![],
            )
            .await?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct TermEntry {
    category: String,
    names: Vec<String>,
    meaning: String,
    example: String,
}

fn find_term_by_name(name: &str) -> Option<TermEntry> {
    let query = name.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let terms = mafia_term_entries();
    for term in &terms {
        if term.names.iter().any(|alias| alias.to_lowercase() == query) {
            return Some(term.clone());
        }
    }
    let matches = terms
        .into_iter()
        .filter(|term| {
            term.names
                .iter()
                .any(|alias| alias.to_lowercase().contains(&query))
        })
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

fn term_field_value(term: &TermEntry) -> String {
    let mut lines = vec![term.meaning.clone()];
    if term.names.len() > 1 {
        lines.push(format!("같은 말: {}", term.names[1..].join(", ")));
    }
    if !term.example.is_empty() {
        lines.push(format!("예시: {}", term.example));
    }
    lines.join("\n")
}

fn term_guide_pages() -> Vec<(String, String)> {
    let mut pages = Vec::new();
    let mut grouped: Vec<(String, Vec<TermEntry>)> = Vec::new();
    for term in mafia_term_entries() {
        if let Some((_category, terms)) = grouped
            .iter_mut()
            .find(|(category, _terms)| *category == term.category)
        {
            terms.push(term);
        } else {
            grouped.push((term.category.clone(), vec![term]));
        }
    }
    for (category, terms) in grouped {
        let mut body =
            "마피아42 용어 문서를 참고해 이 봇 진행에 맞게 짧게 정리한 용어집입니다.".to_string();
        let mut page_index = 1;
        for term in terms {
            let entry = format!("\n\n**{}**\n{}", term.names[0], term_field_value(&term));
            if body.len() + entry.len() > 3600 {
                let title = if page_index == 1 {
                    format!("용어 설명 - {category}")
                } else {
                    format!("용어 설명 - {category} {page_index}")
                };
                pages.push((title, body));
                page_index += 1;
                body = "마피아42 용어 문서를 참고해 이 봇 진행에 맞게 짧게 정리한 용어집입니다."
                    .to_string();
            }
            body.push_str(&entry);
        }
        let title = if page_index == 1 {
            format!("용어 설명 - {category}")
        } else {
            format!("용어 설명 - {category} {page_index}")
        };
        pages.push((title, body));
    }
    pages
}

fn mafia_term_entries() -> Vec<TermEntry> {
    let mut terms = Vec::new();
    let mut in_section = false;
    for line in include_str!("../role_data.py").lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("MAFIA_TERM_ENTRIES") {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        if trimmed == ")" {
            break;
        }
        if !trimmed.starts_with("(\"") {
            continue;
        }
        let strings = extract_python_strings(trimmed);
        if strings.len() < 4 {
            continue;
        }
        let category = strings[0].clone();
        let meaning = strings[strings.len() - 2].clone();
        let example = strings[strings.len() - 1].clone();
        let names = strings[1..strings.len() - 2].to_vec();
        if !names.is_empty() {
            terms.push(TermEntry {
                category,
                names,
                meaning,
                example,
            });
        }
    }
    terms
}

fn extract_python_strings(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in line.chars() {
        if !in_string {
            if ch == '"' {
                in_string = true;
                current.clear();
            }
            continue;
        }
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            in_string = false;
            values.push(current.clone());
        } else {
            current.push(ch);
        }
    }
    values
}

fn find_role_by_name(name: &str) -> Option<Role> {
    let query = name.trim();
    [
        Role::Mafia,
        Role::Doctor,
        Role::Nurse,
        Role::Police,
        Role::Agent,
        Role::Vigilante,
        Role::Reporter,
        Role::Hacker,
        Role::Detective,
        Role::Shaman,
        Role::Priest,
        Role::Soldier,
        Role::Gangster,
        Role::Prophet,
        Role::Psychologist,
        Role::Spy,
        Role::Contractor,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Madam,
        Role::Graverobber,
        Role::Godfather,
        Role::Joker,
        Role::Politician,
        Role::Judge,
        Role::Terrorist,
        Role::Lover,
        Role::CultLeader,
        Role::Fanatic,
        Role::Citizen,
    ]
    .into_iter()
    .find(|role| role.value() == query)
}

#[derive(Clone, Copy)]
enum AnonymousMessageKind {
    General { owner_id: u64 },
    Dead { owner_id: u64 },
    Shaman { owner_id: u64 },
    Role { owner_id: u64, role: Role },
}

fn anonymous_message_body(message: &serenity::Message) -> String {
    let mut parts = Vec::new();
    let content = message.content.trim();
    if !content.is_empty() {
        parts.push(content.to_string());
    }
    parts.extend(
        message
            .attachments
            .iter()
            .map(|attachment| attachment.url.clone()),
    );
    if parts.is_empty() {
        "(내용 없음)".to_string()
    } else {
        parts.join("\n")
    }
}

fn anonymous_avatar_url(author_label: &str) -> Option<String> {
    if let Some(number) = author_label
        .strip_suffix("번")
        .and_then(|value| value.parse::<usize>().ok())
    {
        let color = NUMBER_AVATAR_COLORS[(number.saturating_sub(1)) % NUMBER_AVATAR_COLORS.len()];
        return Some(format!(
            "https://dummyimage.com/128x128/{color}/ffffff.png&text={number}"
        ));
    }
    animal_emoji_code(author_label).map(|code| {
        format!("https://cdn.jsdelivr.net/gh/twitter/twemoji@14.0.2/assets/72x72/{code}.png")
    })
}

fn no_mentions() -> serenity::CreateAllowedMentions {
    serenity::CreateAllowedMentions::new()
        .all_users(false)
        .all_roles(false)
        .everyone(false)
        .replied_user(false)
}

async fn anonymous_webhook(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
) -> Option<serenity::Webhook> {
    let cached_url = running
        .read()
        .await
        .anonymous_webhook_urls
        .get(&channel_id)
        .cloned();
    if let Some(url) = cached_url
        && let Ok(webhook) = serenity::Webhook::from_url(&ctx.http, &url).await
    {
        return Some(webhook);
    }

    let webhook = channel_id
        .create_webhook(
            &ctx.http,
            serenity::CreateWebhook::new("Mafia Anonymous")
                .audit_log_reason("마피아 게임 익명 채팅 웹훅 생성"),
        )
        .await
        .ok()?;
    if let Some(url) = webhook.url.as_ref() {
        running
            .write()
            .await
            .anonymous_webhook_urls
            .insert(channel_id, url.expose_secret().to_string());
    }
    Some(webhook)
}

async fn send_anonymous_text(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    author_label: &str,
    body: &str,
) {
    if let Some(webhook) = anonymous_webhook(ctx, running, channel_id).await {
        let username = author_label.chars().take(80).collect::<String>();
        let mut builder = serenity::ExecuteWebhook::new()
            .content(body)
            .username(username)
            .allowed_mentions(no_mentions());
        if let Some(avatar_url) = anonymous_avatar_url(author_label) {
            builder = builder.avatar_url(avatar_url);
        }
        if webhook.execute(&ctx.http, false, builder).await.is_ok() {
            return;
        }
    }
    let _ = channel_id
        .send_message(
            &ctx.http,
            serenity::CreateMessage::new()
                .content(format!("{author_label}: {body}"))
                .allowed_mentions(no_mentions()),
        )
        .await;
}

fn can_use_anonymous_dead_chat(running: &RunningGame, player: &Player) -> bool {
    !player.alive && !running.game.purified_dead_ids.contains(&player.user_id)
}

fn can_use_anonymous_shaman_chat(running: &RunningGame, player: &Player) -> bool {
    if !player.alive {
        return !running.game.purified_dead_ids.contains(&player.user_id);
    }
    player.role == Role::Shaman
        && running.game.phase == Phase::Night
        && !running.game.is_frog(player)
        && !running.game.is_madam_seduced(player)
}

fn anonymous_dead_sender_label(running: &RunningGame, sender: &Player) -> String {
    if sender.alive && sender.role == Role::Shaman {
        "익명의 목소리".to_string()
    } else if running.anonymous_enabled {
        running
            .anonymous_aliases
            .get(&sender.user_id)
            .cloned()
            .unwrap_or_else(|| "익명".to_string())
    } else {
        sender.name.clone()
    }
}

async fn relay_anonymous_general_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    sender_id: u64,
    body: &str,
) {
    let (deliveries, log_channel, sender_alias) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(sender_id) else {
            return;
        };
        let sender_alias = running_read
            .anonymous_aliases
            .get(&sender.user_id)
            .cloned()
            .unwrap_or_else(|| "익명".to_string());
        let deliveries = running_read
            .game
            .alive_players()
            .into_iter()
            .filter(|viewer| viewer.user_id != sender.user_id && !running_read.game.is_frog(viewer))
            .filter_map(|viewer| {
                running_read
                    .anonymous_input_channel_ids
                    .get(&viewer.user_id)
                    .copied()
            })
            .collect::<Vec<_>>();
        (deliveries, running_read.channel_id, sender_alias)
    };
    for channel_id in deliveries {
        send_anonymous_text(ctx, running, channel_id, &sender_alias, body).await;
    }
    send_anonymous_text(
        ctx,
        running,
        log_channel,
        "[익명 로그/일반]",
        &format!("{sender_alias} - {body}"),
    )
    .await;
}

async fn relay_anonymous_role_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    sender_id: u64,
    role: Role,
    body: &str,
) {
    let (deliveries, log_channel, sender_alias) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(sender_id) else {
            return;
        };
        let sender_alias = running_read
            .anonymous_aliases
            .get(&sender.user_id)
            .cloned()
            .unwrap_or_else(|| "익명".to_string());
        let deliveries = anonymous_role_status_player_ids(&running_read, role)
            .into_iter()
            .filter(|viewer_id| *viewer_id != sender.user_id)
            .filter_map(|viewer_id| {
                let viewer = running_read.game.get_player(viewer_id)?;
                if !can_use_anonymous_role_chat(&running_read, viewer, role) {
                    return None;
                }
                running_read
                    .anonymous_role_input_channel_ids
                    .get(&(viewer_id, role))
                    .copied()
            })
            .collect::<Vec<_>>();
        (deliveries, running_read.channel_id, sender_alias)
    };
    for channel_id in deliveries {
        send_anonymous_text(ctx, running, channel_id, &sender_alias, body).await;
    }
    send_anonymous_text(
        ctx,
        running,
        log_channel,
        &format!("[익명 로그/{}]", role.value()),
        &format!("{sender_alias} - {body}"),
    )
    .await;
}

async fn relay_anonymous_dead_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    sender_id: u64,
    body: &str,
) {
    let (deliveries, sender_label) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(sender_id) else {
            return;
        };
        let deliveries = running_read
            .game
            .players
            .iter()
            .filter(|viewer| {
                viewer.user_id != sender.user_id
                    && !viewer.alive
                    && !running_read
                        .game
                        .purified_dead_ids
                        .contains(&viewer.user_id)
            })
            .filter_map(|viewer| {
                running_read
                    .anonymous_dead_input_channel_ids
                    .get(&viewer.user_id)
                    .copied()
            })
            .collect::<Vec<_>>();
        (
            deliveries,
            anonymous_dead_sender_label(&running_read, sender),
        )
    };
    for channel_id in deliveries {
        send_anonymous_text(ctx, running, channel_id, &sender_label, body).await;
    }
}

async fn relay_anonymous_shaman_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    sender_id: u64,
    body: &str,
) {
    let (deliveries, log_channel, sender_label) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(sender_id) else {
            return;
        };
        let deliveries = running_read
            .game
            .players
            .iter()
            .filter(|viewer| {
                viewer.user_id != sender.user_id
                    && ((!viewer.alive
                        && !running_read
                            .game
                            .purified_dead_ids
                            .contains(&viewer.user_id))
                        || (viewer.alive
                            && viewer.role == Role::Shaman
                            && !running_read.game.is_frog(viewer)))
            })
            .filter_map(|viewer| {
                running_read
                    .anonymous_shaman_input_channel_ids
                    .get(&viewer.user_id)
                    .copied()
            })
            .collect::<Vec<_>>();
        (
            deliveries,
            running_read.shaman_channel_id,
            anonymous_dead_sender_label(&running_read, sender),
        )
    };
    for channel_id in deliveries {
        send_anonymous_text(ctx, running, channel_id, &sender_label, body).await;
    }
    if let Some(channel_id) = log_channel {
        send_anonymous_text(
            ctx,
            running,
            channel_id,
            "[익명 로그/영매]",
            &format!("{sender_label} - {body}"),
        )
        .await;
    }
}

async fn handle_anonymous_message(
    ctx: &serenity::Context,
    running: Arc<RwLock<RunningGame>>,
    message: &serenity::Message,
    kind: AnonymousMessageKind,
) -> Result<()> {
    let owner_id = match kind {
        AnonymousMessageKind::General { owner_id }
        | AnonymousMessageKind::Dead { owner_id }
        | AnonymousMessageKind::Shaman { owner_id }
        | AnonymousMessageKind::Role { owner_id, .. } => owner_id,
    };
    if message.author.id.get() != owner_id {
        let _ = message.delete(&ctx.http).await;
        return Ok(());
    }

    let body = anonymous_message_body(message);
    let can_relay = {
        let running_read = running.read().await;
        let Some(player) = running_read.game.get_player(owner_id) else {
            return Ok(());
        };
        match kind {
            AnonymousMessageKind::General { .. } => {
                if running_read.game.is_madam_seduced(player) {
                    false
                } else {
                    can_use_anonymous_general_chat(&running_read, player)
                }
            }
            AnonymousMessageKind::Dead { .. } => can_use_anonymous_dead_chat(&running_read, player),
            AnonymousMessageKind::Shaman { .. } => {
                can_use_anonymous_shaman_chat(&running_read, player)
            }
            AnonymousMessageKind::Role { role, .. } => {
                if running_read.game.is_madam_seduced(player) {
                    false
                } else {
                    can_use_anonymous_role_chat(&running_read, player, role)
                }
            }
        }
    };
    if !can_relay {
        return Ok(());
    }

    match kind {
        AnonymousMessageKind::General { .. } => {
            relay_anonymous_general_message(ctx, &running, owner_id, &body).await;
        }
        AnonymousMessageKind::Dead { .. } => {
            relay_anonymous_dead_message(ctx, &running, owner_id, &body).await;
        }
        AnonymousMessageKind::Shaman { .. } => {
            relay_anonymous_shaman_message(ctx, &running, owner_id, &body).await;
        }
        AnonymousMessageKind::Role { role, .. } => {
            relay_anonymous_role_message(ctx, &running, owner_id, role, &body).await;
        }
    }
    Ok(())
}

async fn handle_message_event(
    ctx: &serenity::Context,
    data: &Data,
    message: &serenity::Message,
) -> Result<()> {
    if message.author.bot {
        return Ok(());
    }
    let Some(guild_id) = message.guild_id else {
        return Ok(());
    };
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        return Ok(());
    };
    let kind = {
        let running_read = running.read().await;
        if let Some(owner_id) = running_read
            .anonymous_dead_input_channel_owners
            .get(&message.channel_id)
            .copied()
        {
            Some(AnonymousMessageKind::Dead { owner_id })
        } else if let Some(owner_id) = running_read
            .anonymous_shaman_input_channel_owners
            .get(&message.channel_id)
            .copied()
        {
            Some(AnonymousMessageKind::Shaman { owner_id })
        } else if let Some(owner_id) = running_read
            .anonymous_input_channel_owners
            .get(&message.channel_id)
            .copied()
        {
            Some(AnonymousMessageKind::General { owner_id })
        } else {
            running_read
                .anonymous_role_input_channels
                .get(&message.channel_id)
                .copied()
                .map(|(owner_id, role)| AnonymousMessageKind::Role { owner_id, role })
        }
    };
    if let Some(kind) = kind {
        handle_anonymous_message(ctx, running, message, kind).await?;
    }
    Ok(())
}

#[allow(clippy::single_match, clippy::collapsible_if)]
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
            if let Err(error) = handle_component(ctx, data, component).await {
                eprintln!("component error: {error:?}");
            }
        }
        serenity::FullEvent::Message { new_message } => {
            if let Err(error) = handle_message_event(ctx, data, new_message).await {
                eprintln!("message event error: {error:?}");
            }
        }
        _ => {}
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let token =
        std::env::var("DISCORD_TOKEN").context(".env 파일에 DISCORD_TOKEN을 설정하세요.")?;
    let config_path = workspace_path("config.json")?;
    let stats_path = workspace_path("stats.json")?;
    let config = config::load_config(&config_path)?;
    let stats = stats::load_stats(&stats_path).unwrap_or_default();
    let web_host = std::env::var("WEB_SETTINGS_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let web_port = std::env::var("WEB_SETTINGS_PORT")
        .unwrap_or_else(|_| "8800".to_string())
        .parse::<u16>()
        .context("WEB_SETTINGS_PORT는 1~65535 사이 숫자여야 합니다.")?;
    let web_base_url = web_settings::base_url(&web_host, web_port);
    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::GUILD_MEMBERS
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_PRESENCES;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                start_game(),
                stop_game(),
                disable_mafia_game(),
                enable_mafia_game(),
                add_to_blacklist(),
                remove_from_blacklist(),
                show_blacklist(),
                configure_game(),
                web_configure_game(),
                configure_player_limit(),
                configure_anonymous_mode(),
                configure_extra_roles(),
                configure_investigation_role(),
                show_manager_status(),
                show_public_status(),
                memo(),
                show_my_info(),
                rating_log(),
                show_leaderboard(),
                reset_leaderboard(),
                show_term_info(),
                show_term_descriptions(),
                show_role_info(),
                show_abilities(),
                show_role_descriptions(),
            ],
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                println!("Rust Mafia bot ready: {}", ready.user.name);
                let config = Arc::new(RwLock::new(config));
                let config_path = Arc::new(config_path);
                let stats = Arc::new(RwLock::new(stats));
                let stats_path = Arc::new(stats_path);
                let web_sessions = Arc::new(DashMap::new());
                let games = Arc::new(DashMap::new());
                let recruitments = Arc::new(DashMap::new());
                let data = Data {
                    config: config.clone(),
                    config_path: config_path.clone(),
                    stats: stats.clone(),
                    stats_path,
                    games: games.clone(),
                    recruitments: recruitments.clone(),
                    web_sessions: web_sessions.clone(),
                    web_base_url: Arc::new(web_base_url.clone()),
                    bot_user_id: ready.user.id,
                };
                let web_state = web_settings::WebSettingsState {
                    config,
                    config_path,
                    stats,
                    games,
                    recruitments,
                    sessions: web_sessions,
                    started_at: Instant::now(),
                    bot_name: ready.user.name.clone(),
                    guild_count: ready.guilds.len(),
                };
                let host = web_host.clone();
                tokio::spawn(async move {
                    if let Err(error) = web_settings::run_server(web_state, host, web_port).await {
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
