use anyhow::{Context as AnyhowContext, Result};
use dashmap::DashMap;
use mafia_remake::game::MafiaGame;
use mafia_remake::model::Role;
use mafia_remake::{config, stats};
use poise::serenity_prelude as serenity;
pub(crate) mod web_settings;

use std::collections::{HashMap, HashSet};
use std::{
    path::PathBuf,
    sync::Arc,
    time::Instant,
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
    /// Activity 프론트엔드에 표시할 밤 행동 결과 (user_id → 결과 텍스트)
    activity_night_results: HashMap<u64, String>,
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


mod embed;
mod channel;
mod runner;
mod commands;
mod activity;

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
        serenity::FullEvent::Message { new_message } => {
            if let Err(error) = commands::handle_message_event(ctx, data, new_message).await {
                eprintln!("message event error: {error:?}");
            }
        }
        _ => {}
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    dotenvy::dotenv().ok();
    let token =
        std::env::var("DISCORD_TOKEN").context(".env 파일에 DISCORD_TOKEN을 설정하세요.")?;
    let config_path = embed::workspace_path("config.json")?;
    let stats_path = embed::workspace_path("stats.json")?;
    let config = config::load_config(&config_path)?;
    let stats = stats::load_stats(&stats_path).unwrap_or_default();
    let web_host = std::env::var("WEB_SETTINGS_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let web_port = std::env::var("WEB_SETTINGS_PORT")
        .unwrap_or_else(|_| "8800".to_string())
        .parse::<u16>()
        .context("WEB_SETTINGS_PORT는 1~65535 사이 숫자여야 합니다.")?;
    let web_base_url = web_settings::base_url(&web_host, web_port);

    // 공유 상태를 Discord 연결 전에 먼저 생성
    let games: Arc<DashMap<serenity::GuildId, Arc<RwLock<RunningGame>>>> = Arc::new(DashMap::new());
    let recruitments: Arc<DashMap<serenity::GuildId, Arc<RwLock<Recruitment>>>> = Arc::new(DashMap::new());
    let config_arc = Arc::new(RwLock::new(config));
    let stats_arc = Arc::new(RwLock::new(stats));
    let web_sessions: Arc<DashMap<String, web_settings::WebSettingsSession>> = Arc::new(DashMap::new());
    let config_path_arc = Arc::new(config_path);
    let stats_path_arc = Arc::new(stats_path);

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
        activity_client_id,
        activity_client_secret,
    );
    let activity_host = web_host.clone();
    tokio::spawn(async move {
        activity::run_activity_server(activity_state, activity_host, activity_port, activity_static, activity_tls_cert, activity_tls_key).await;
    });

    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::GUILD_MEMBERS
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_PRESENCES;

    let games_setup = games.clone();
    let recruitments_setup = recruitments.clone();
    let config_setup = config_arc.clone();
    let stats_setup = stats_arc.clone();
    let web_sessions_setup = web_sessions.clone();
    let config_path_setup = config_path_arc.clone();
    let stats_path_setup = stats_path_arc.clone();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::start_game(),
                commands::stop_game(),
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
                // Entry Point 커맨드(Activity용)가 있으면 bulk 등록이 실패하므로 에러 무시
                if let Err(e) = poise::builtins::register_globally(ctx, &framework.options().commands).await {
                    eprintln!("커맨드 등록 경고 (무시됨): {e}");
                }
                println!("Rust Mafia bot ready: {}", ready.user.name);
                let data = Data {
                    config: config_setup.clone(),
                    config_path: config_path_setup.clone(),
                    stats: stats_setup.clone(),
                    stats_path: stats_path_setup,
                    games: games_setup.clone(),
                    recruitments: recruitments_setup.clone(),
                    web_sessions: web_sessions_setup.clone(),
                    web_base_url: Arc::new(web_base_url.clone()),
                    bot_user_id: ready.user.id,
                };
                let web_state = web_settings::WebSettingsState {
                    config: config_setup,
                    config_path: config_path_setup,
                    stats: stats_setup,
                    games: games_setup,
                    recruitments: recruitments_setup,
                    sessions: web_sessions_setup,
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
