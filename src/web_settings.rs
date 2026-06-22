use crate::{Recruitment, RunningGame};
use anyhow::{Context, Result, bail};
use chrono::{SecondsFormat, Utc};
use dashmap::DashMap;
use mafia_remake::config::{self, BotConfig};
use mafia_remake::model::{
    CITIZEN_SPECIAL_ROLES, MAFIA_SPECIAL_ROLES, NEUTRAL_SPECIAL_ROLES, Phase, Role,
};
use mafia_remake::stats::{self, StatsFile};
use poise::serenity_prelude as serenity;
use rand::RngCore;
use rustls::ServerConfig;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

const WEB_SETTINGS_PATH: &str = "/web-settings";
const WEB_SETTINGS_SESSION_TTL_SECONDS: u64 = 600;
const MAX_GAME_PLAYERS: usize = 24;
const WEB_LEADERBOARD_METRICS: &[&str] =
    &["rating", "wins", "winrate", "games", "mafia", "playtime"];

#[derive(Debug, Clone)]
pub struct WebSettingsSession {
    pub guild_id: u64,
    pub user_id: u64,
    pub user_label: String,
    pub expires_at: Instant,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiKeyStore {
    #[serde(default)]
    keys: Vec<ApiKeyRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiKeyRecord {
    id: String,
    label: String,
    guild_id: u64,
    created_by_user_id: u64,
    created_at: String,
    key_hash: String,
    #[serde(default)]
    revoked: bool,
}

#[derive(Clone)]
pub struct WebSettingsState {
    pub config: Arc<RwLock<BotConfig>>,
    pub config_path: Arc<PathBuf>,
    pub api_keys: Arc<RwLock<ApiKeyStore>>,
    pub api_keys_path: Arc<PathBuf>,
    pub stats: Arc<RwLock<StatsFile>>,
    pub games: Arc<DashMap<serenity::GuildId, Arc<RwLock<RunningGame>>>>,
    pub recruitments: Arc<DashMap<serenity::GuildId, Arc<RwLock<Recruitment>>>>,
    pub sessions: Arc<DashMap<String, WebSettingsSession>>,
    pub started_at: Instant,
    pub bot_name: String,
    pub guild_count: usize,
    pub base_url: String,
}

pub fn load_api_key_store(path: impl AsRef<Path>) -> Result<ApiKeyStore> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(ApiKeyStore::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("API 키 파일을 읽지 못했습니다: {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("API 키 JSON을 파싱하지 못했습니다: {}", path.display()))
}

fn save_api_key_store(path: impl AsRef<Path>, store: &ApiKeyStore) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("API 키 디렉터리를 만들지 못했습니다: {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(store).context("API 키 JSON 직렬화 실패")?;
    let temp_path = path.with_file_name(format!(
        "{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("api_keys.json")
    ));
    fs::write(&temp_path, format!("{text}\n"))
        .with_context(|| format!("API 키 임시 파일을 쓰지 못했습니다: {}", temp_path.display()))?;
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("기존 API 키 파일을 교체하지 못했습니다: {}", path.display()))?;
    }
    fs::rename(&temp_path, path)
        .with_context(|| format!("API 키 파일을 교체하지 못했습니다: {}", path.display()))?;
    Ok(())
}

fn api_key_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn issue_api_key(store: &mut ApiKeyStore, guild_id: u64, user_id: u64, label: String) -> String {
    let key = format!(
        "mfr_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    store.keys.push(ApiKeyRecord {
        id: Uuid::new_v4().simple().to_string(),
        label,
        guild_id,
        created_by_user_id: user_id,
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        key_hash: api_key_hash(&key),
        revoked: false,
    });
    key
}

#[derive(Debug, Clone, Copy)]
enum WebFieldKind {
    Bool,
    Int,
    Text,
    IntList,
}

#[derive(Debug, Clone, Copy)]
struct WebConfigField {
    name: &'static str,
    label: &'static str,
    kind: WebFieldKind,
    min_value: Option<u64>,
}

const WEB_CONFIG_FIELDS: &[WebConfigField] = &[
    field(
        "participant_role",
        "참가자 역할 이름",
        WebFieldKind::Text,
        None,
    ),
    field("manager_role", "관리자 역할 이름", WebFieldKind::Text, None),
    field("game_enabled", "게임 시작 활성화", WebFieldKind::Bool, None),
    field(
        "max_player_count",
        "모집 최대 인원 (0 = 제한 없음)",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "night_seconds",
        "밤 진행 시간(초)",
        WebFieldKind::Int,
        Some(1),
    ),
    field(
        "discussion_seconds",
        "낮 토론 시간(초)",
        WebFieldKind::Int,
        Some(1),
    ),
    field("vote_seconds", "투표 시간(초)", WebFieldKind::Int, Some(1)),
    field(
        "chat_slowmode_seconds",
        "낮 채팅 슬로우모드(초)",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "default_mafia_count",
        "기본 마피아 수",
        WebFieldKind::Int,
        Some(1),
    ),
    field(
        "default_doctor_count",
        "기본 의사 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "default_police_count",
        "기본 경찰 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "default_joker_count",
        "기본 조커 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "citizen_special_count",
        "시민 특수룰 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "mafia_special_count",
        "마피아 특수룰 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "neutral_special_count",
        "중립 특수룰 수",
        WebFieldKind::Int,
        Some(0),
    ),
    field(
        "reveal_death_roles",
        "사망 시 직업 공개",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "reveal_public_police_status",
        "경찰 조사 결과 공개",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "reveal_morning_mafia_count",
        "아침마다 생존 마피아 수 공개",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "anonymous_mode",
        "익명 채팅 모드 사용",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "anonymous_name_mode",
        "익명 이름 모드 (animal / number)",
        WebFieldKind::Text,
        None,
    ),
    field("use_agent", "요원 사용", WebFieldKind::Bool, None),
    field("use_vigilante", "자경단원 사용", WebFieldKind::Bool, None),
    field(
        "enable_detective",
        "사립탐정 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "enable_graverobber",
        "도굴꾼 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_spy", "스파이 활성화", WebFieldKind::Bool, None),
    field(
        "enable_contractor",
        "청부업자 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_witch", "마녀 활성화", WebFieldKind::Bool, None),
    field(
        "enable_scientist",
        "과학자 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_madam", "마담 활성화", WebFieldKind::Bool, None),
    field("enable_godfather", "대부 활성화", WebFieldKind::Bool, None),
    field("enable_joker", "조커 활성화", WebFieldKind::Bool, None),
    field(
        "enable_politician",
        "정치인 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_judge", "판사 활성화", WebFieldKind::Bool, None),
    field("enable_reporter", "기자 활성화", WebFieldKind::Bool, None),
    field("enable_hacker", "해커 활성화", WebFieldKind::Bool, None),
    field(
        "enable_terrorist",
        "테러리스트 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_lover", "연인 활성화", WebFieldKind::Bool, None),
    field("enable_shaman", "영매 활성화", WebFieldKind::Bool, None),
    field("enable_priest", "성직자 활성화", WebFieldKind::Bool, None),
    field("enable_soldier", "군인 활성화", WebFieldKind::Bool, None),
    field("enable_nurse", "간호사 활성화", WebFieldKind::Bool, None),
    field("enable_gangster", "건달 활성화", WebFieldKind::Bool, None),
    field("enable_prophet", "예언자 활성화", WebFieldKind::Bool, None),
    field(
        "enable_psychologist",
        "심리학자 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field("enable_thief", "도둑 활성화", WebFieldKind::Bool, None),
    field(
        "enable_cult_team",
        "교주/광신도 팀 활성화",
        WebFieldKind::Bool,
        None,
    ),
    field(
        "blacklist_user_ids",
        "블랙리스트 유저 ID 목록",
        WebFieldKind::IntList,
        None,
    ),
];

const fn field(
    name: &'static str,
    label: &'static str,
    kind: WebFieldKind,
    min_value: Option<u64>,
) -> WebConfigField {
    WebConfigField {
        name,
        label,
        kind,
        min_value,
    }
}

const WEB_PAGE_STYLE: &str = r#"
<style>
  :root { color-scheme: light; --bg: #f4f6f8; --surface: #ffffff; --surface-strong: #f8fafc; --line: #dbe2e8; --text: #1f2933; --muted: #667085; --accent: #2563eb; --accent-strong: #1d4ed8; --warm: #a16207; --danger: #c2413b; }
  * { box-sizing: border-box; }
  html { min-width: 320px; background: var(--bg); }
  body { min-width: 320px; margin: 0; padding: 28px 20px 48px; background: var(--bg); color: var(--text); font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Apple SD Gothic Neo", sans-serif; font-size: 15px; line-height: 1.55; }
  .site-shell { width: min(1120px, 100%); margin: 0 auto; }
  .site-header { display: flex; align-items: center; gap: 12px; padding: 0 0 18px; border-bottom: 1px solid var(--line); }
  .site-mark { display: grid; place-items: center; width: 34px; height: 34px; flex: 0 0 34px; border: 1px solid #bfdbfe; border-radius: 6px; background: #eff6ff; color: var(--accent-strong); text-decoration: none; font-weight: 800; letter-spacing: 0; }
  .eyebrow { margin: 0 0 2px; color: var(--muted); font-size: 0.72rem; font-weight: 700; letter-spacing: 0.06em; }
  h1, h2, h3 { color: var(--text); letter-spacing: 0; }
  h1 { margin: 0; font-size: 1.5rem; line-height: 1.2; }
  h2 { margin: 0 0 12px; font-size: 1.05rem; line-height: 1.3; }
  h3 { margin: 0 0 8px; font-size: 0.95rem; }
  a { color: var(--accent-strong); text-underline-offset: 3px; }
  a:hover { color: #1e40af; }
  main { min-width: 0; }
  .meta { margin: 0 0 20px; color: var(--muted); font-size: 0.92rem; }
  .nav { display: flex; flex-wrap: wrap; gap: 4px; margin: 14px 0 20px; padding: 5px; border: 1px solid var(--line); border-radius: 6px; background: var(--surface); box-shadow: 0 1px 2px rgb(31 41 51 / 0.04); }
  .nav a { padding: 7px 10px; border: 1px solid transparent; color: var(--muted); text-decoration: none; }
  .nav a:hover { border-color: #dbeafe; background: #eff6ff; color: var(--accent-strong); }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 190px), 1fr)); gap: 10px; margin: 16px 0; }
  .split { display: grid; grid-template-columns: minmax(0, 1.1fr) minmax(0, 0.9fr); gap: 14px; }
  .card, .podium-card { min-width: 0; border: 1px solid var(--line); border-radius: 6px; padding: 14px; background: var(--surface); box-shadow: 0 1px 2px rgb(31 41 51 / 0.04); }
  .card span, .podium-card .rank { color: var(--muted); font-size: 0.82rem; }
  .card strong { display: block; margin-top: 5px; color: var(--text); font-size: 1.45rem; line-height: 1.1; overflow-wrap: anywhere; }
  .panel { min-width: 0; overflow-x: auto; border: 1px solid var(--line); border-radius: 6px; padding: 16px; margin: 14px 0; background: var(--surface); box-shadow: 0 1px 2px rgb(31 41 51 / 0.04); }
  .panel > :last-child { margin-bottom: 0; }
  .pill { display: inline-block; padding: 2px 8px; border: 1px solid var(--line); border-radius: 999px; color: var(--muted); font-size: 0.82rem; }
  .metric-tabs { display: flex; flex-wrap: wrap; gap: 6px; margin: 12px 0 18px; }
  .metric-tabs a { padding: 6px 10px; border: 1px solid var(--line); border-radius: 4px; background: var(--surface); color: var(--muted); text-decoration: none; }
  .metric-tabs a:hover, .metric-tabs a.active { border-color: #bfdbfe; background: #eff6ff; color: var(--accent-strong); }
  .podium { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 190px), 1fr)); gap: 10px; margin-bottom: 16px; }
  .podium-card .name { margin: 7px 0; font-size: 1.05rem; font-weight: 800; overflow-wrap: anywhere; }
  .podium-card .rating { color: #854d0e; font-size: 1.35rem; font-weight: 800; }
  .endpoint { display: grid; grid-template-columns: minmax(0, 0.85fr) minmax(0, 1.15fr); gap: 12px; padding: 12px 0; border-bottom: 1px solid var(--line); }
  .endpoint:last-child { border-bottom: 0; padding-bottom: 0; }
  code { display: inline; max-width: 100%; padding: 2px 5px; border: 1px solid #d9e2ec; border-radius: 4px; background: #f6f8fa; color: #334e68; font-family: ui-monospace, SFMono-Regular, Consolas, monospace; font-size: 0.88em; overflow-wrap: anywhere; word-break: break-word; }
  pre { max-width: 100%; margin: 10px 0 0; padding: 12px; overflow-x: auto; border: 1px solid #d9e2ec; border-radius: 4px; background: #f8fafc; color: #334155; font-family: ui-monospace, SFMono-Regular, Consolas, monospace; font-size: 0.82rem; line-height: 1.55; white-space: pre-wrap; overflow-wrap: anywhere; word-break: break-word; }
  table { width: 100%; min-width: 560px; border-collapse: collapse; }
  th, td { padding: 9px 8px; border-bottom: 1px solid var(--line); text-align: left; vertical-align: top; overflow-wrap: anywhere; }
  th { color: var(--muted); font-size: 0.78rem; font-weight: 700; letter-spacing: 0.04em; }
  td.num, th.num { text-align: right; }
  fieldset { min-width: 0; margin: 0 0 16px; padding: 4px 16px; border: 1px solid var(--line); border-radius: 6px; background: var(--surface); }
  legend { padding: 0 6px; color: var(--text); font-weight: 700; }
  .row { display: flex; align-items: center; justify-content: space-between; min-width: 0; gap: 16px; padding: 10px 0; border-bottom: 1px solid #edf0f2; }
  .row:last-child { border-bottom: none; }
  .row span { min-width: 0; flex: 1 1 auto; overflow-wrap: anywhere; }
  input[type="text"], input[type="number"], textarea { width: min(400px, 100%); min-width: 0; padding: 8px 10px; border: 1px solid #cbd5df; border-radius: 4px; background: #fff; color: var(--text); font: inherit; font-size: 0.92rem; }
  input[type="text"]:focus, input[type="number"]:focus, textarea:focus { outline: 2px solid #bfdbfe; outline-offset: 1px; border-color: var(--accent); }
  textarea { min-height: 88px; resize: vertical; }
  input[type="checkbox"] { width: 18px; height: 18px; accent-color: var(--accent); }
  button { margin-top: 14px; padding: 9px 14px; border: 1px solid var(--accent-strong); border-radius: 4px; background: var(--accent-strong); color: #fff; font: inherit; font-weight: 700; cursor: pointer; transition: background 140ms ease, border-color 140ms ease; }
  button:hover { border-color: #1e40af; background: #1e40af; }
  button:focus-visible, a:focus-visible { outline: 2px solid #93c5fd; outline-offset: 2px; }
  .message { margin: 0 0 16px; padding: 11px 12px; border: 1px solid #fde68a; border-left: 3px solid var(--warm); border-radius: 4px; background: #fffbeb; color: #713f12; }
  .message.error { border-color: #fecaca; border-left-color: var(--danger); background: #fef2f2; color: #991b1b; }
  small { color: var(--muted); }
  @media (max-width: 760px) {
    body { padding: 18px 12px 32px; }
    .site-header { align-items: flex-start; }
    .nav { margin-bottom: 14px; }
    .split, .endpoint { grid-template-columns: minmax(0, 1fr); }
    .row { align-items: stretch; flex-direction: column; gap: 8px; }
    input[type="text"], input[type="number"], textarea { width: 100%; }
    table { font-size: 0.88rem; }
  }
</style>
"#;

pub fn settings_path() -> &'static str {
    WEB_SETTINGS_PATH
}

pub fn session_ttl_minutes() -> u64 {
    (WEB_SETTINGS_SESSION_TTL_SECONDS / 60).max(1)
}

pub fn base_url(host: &str, port: u16, use_https: bool) -> String {
    if let Ok(base_url) = std::env::var("WEB_SETTINGS_BASE_URL")
        && !base_url.trim().is_empty()
    {
        return base_url.trim_end_matches('/').to_string();
    }
    let display_host = if matches!(host, "0.0.0.0" | "::") {
        "localhost"
    } else {
        host
    };
    let scheme = if use_https { "https" } else { "http" };
    format!("{scheme}://{display_host}:{port}")
}

pub fn issue_session(
    sessions: &DashMap<String, WebSettingsSession>,
    guild_id: u64,
    user_id: u64,
    user_label: String,
) -> String {
    purge_expired_sessions(sessions);
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut token, "{byte:02x}");
    }
    sessions.insert(
        token.clone(),
        WebSettingsSession {
            guild_id,
            user_id,
            user_label,
            expires_at: Instant::now() + Duration::from_secs(WEB_SETTINGS_SESSION_TTL_SECONDS),
        },
    );
    token
}

pub async fn run_server(
    state: WebSettingsState,
    host: String,
    port: u16,
    tls_cert: Option<String>,
    tls_key: Option<String>,
) -> Result<()> {
    let listener = TcpListener::bind((host.as_str(), port)).await?;
    if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
        let tls_config = Arc::new(load_tls_config(&cert, &key)?);
        let acceptor = TlsAcceptor::from(tls_config);
        println!("Rust web settings server ready (HTTPS): https://{host}:{port}");
        loop {
            let (stream, _addr) = listener.accept().await?;
            let state = state.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(stream) => {
                        if let Err(error) = handle_connection(stream, state).await {
                            eprintln!("web settings error: {error:?}");
                        }
                    }
                    Err(error) => eprintln!("web settings tls error: {error:?}"),
                }
            });
        }
    }

    println!("Rust web settings server ready (HTTP): http://{host}:{port}");
    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, state).await {
                eprintln!("web settings error: {error:?}");
            }
        });
    }
}

fn load_tls_config(cert_path: &str, key_path: &str) -> Result<ServerConfig> {
    let mut cert_reader = BufReader::new(
        File::open(cert_path).with_context(|| format!("failed to open TLS cert: {cert_path}"))?,
    );
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read TLS cert: {cert_path}"))?;
    if certs.is_empty() {
        bail!("TLS cert file has no certificates: {cert_path}");
    }

    let mut key_reader = BufReader::new(
        File::open(key_path).with_context(|| format!("failed to open TLS key: {key_path}"))?,
    );
    let key = rustls_pemfile::private_key(&mut key_reader)
        .with_context(|| format!("failed to read TLS key: {key_path}"))?
        .with_context(|| format!("TLS key file has no private key: {key_path}"))?;

    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("failed to build web settings TLS config")
}

async fn handle_connection<S>(mut stream: S, state: WebSettingsState) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let response = match read_http_request(&mut stream).await {
        Ok(request) => route_request(&state, request).await,
        Err(error) => http_response(
            "400 Bad Request",
            &render_message_page("잘못된 요청", &error.to_string()),
        ),
    };
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}

async fn route_request(state: &WebSettingsState, request: HttpRequest) -> String {
    let (path, query) = request.path.split_once('?').unwrap_or((&request.path, ""));
    if request.method == "OPTIONS" && path.starts_with("/api/") {
        return api_options_response();
    }
    if let Some(response) = route_protected_api_request(state, &request, path, query).await {
        return response;
    }
    if request.method == "GET"
        && let Some(response) = route_public_request(state, path, query).await
    {
        return response;
    }
    let Some(session_path) = path.strip_prefix(&format!("{WEB_SETTINGS_PATH}/")) else {
        return http_response(
            "404 Not Found",
            &render_message_page("404", "요청한 페이지를 찾을 수 없습니다."),
        );
    };
    let (token, subpath) = session_path.split_once('/').unwrap_or((session_path, ""));
    purge_expired_sessions(&state.sessions);
    let Some(session) = state.sessions.get(token).map(|entry| entry.clone()) else {
        return http_response("410 Gone", &expired_page());
    };
    let _session_scope = (session.guild_id, session.user_id);

    if subpath == "api-keys" {
        return route_api_key_management(state, &session, token, &request).await;
    }
    if !subpath.is_empty() {
        return http_response(
            "404 Not Found",
            &render_message_page("404", "요청한 페이지를 찾을 수 없습니다."),
        );
    }

    match request.method.as_str() {
        "GET" => {
            let config = state.config.read().await.clone();
            http_response(
                "200 OK",
                &render_settings_page(
                    &session,
                    &format!("{WEB_SETTINGS_PATH}/{token}"),
                    &config,
                    Some(&web_status_values(state).await),
                    None,
                ),
            )
        }
        "POST" => {
            let updates = match parse_form_updates(&request.body) {
                Ok(updates) => updates,
                Err(error) => {
                    let config = state.config.read().await.clone();
                    return http_response(
                        "400 Bad Request",
                        &render_settings_page(
                            &session,
                            &format!("{WEB_SETTINGS_PATH}/{token}"),
                            &config,
                            Some(&web_status_values(state).await),
                            Some(&error),
                        ),
                    );
                }
            };
            let mut config = state.config.write().await;
            if let Err(error) = apply_updates(&mut config, &updates) {
                let page_config = config.clone();
                drop(config);
                let status = web_status_values(state).await;
                return http_response(
                    "400 Bad Request",
                    &render_settings_page(
                        &session,
                        &format!("{WEB_SETTINGS_PATH}/{token}"),
                        &page_config,
                        Some(&status),
                        Some(&error),
                    ),
                );
            }
            if let Err(error) = config::save_config(&*state.config_path, &config) {
                let page_config = config.clone();
                let error = error.to_string();
                drop(config);
                let status = web_status_values(state).await;
                return http_response(
                    "500 Internal Server Error",
                    &render_settings_page(
                        &session,
                        &format!("{WEB_SETTINGS_PATH}/{token}"),
                        &page_config,
                        Some(&status),
                        Some(&error),
                    ),
                );
            }
            drop(config);
            state.sessions.remove(token);
            http_response("200 OK", &saved_page())
        }
        _ => http_response(
            "405 Method Not Allowed",
            &render_message_page(
                "지원하지 않는 요청",
                "GET 또는 POST 요청만 사용할 수 있습니다.",
            ),
        ),
    }
}

fn purge_expired_sessions(sessions: &DashMap<String, WebSettingsSession>) {
    let now = Instant::now();
    sessions.retain(|_token, session| session.expires_at > now);
}

#[derive(Debug)]
enum ApiAuthError {
    Missing,
    Invalid,
    Forbidden,
}

impl ApiAuthError {
    fn response(&self) -> String {
        match self {
            Self::Missing => json_error("401 Unauthorized", "missing API key"),
            Self::Invalid => json_error("401 Unauthorized", "invalid API key"),
            Self::Forbidden => json_error("403 Forbidden", "API key is not authorized for this guild"),
        }
    }
}

fn request_api_key(request: &HttpRequest) -> Option<&str> {
    request
        .headers
        .get("x-api-key")
        .map(String::as_str)
        .or_else(|| {
            request
                .headers
                .get("authorization")
                .and_then(|value| value.strip_prefix("Bearer "))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

async fn authenticate_api_key(
    state: &WebSettingsState,
    request: &HttpRequest,
) -> std::result::Result<ApiKeyRecord, ApiAuthError> {
    let key = request_api_key(request).ok_or(ApiAuthError::Missing)?;
    let key_hash = api_key_hash(key);
    state
        .api_keys
        .read()
        .await
        .keys
        .iter()
        .find(|record| !record.revoked && record.key_hash == key_hash)
        .cloned()
        .ok_or(ApiAuthError::Invalid)
}

fn require_key_guild(record: &ApiKeyRecord, guild_id: u64) -> std::result::Result<(), ApiAuthError> {
    if record.guild_id == guild_id {
        Ok(())
    } else {
        Err(ApiAuthError::Forbidden)
    }
}

fn api_key_value(record: &ApiKeyRecord) -> Value {
    json!({
        "id": record.id,
        "label": record.label,
        "guild_id": record.guild_id,
        "created_at": record.created_at,
        "revoked": record.revoked,
    })
}

fn parse_api_guild_path<'a>(path: &'a str, prefix: &str) -> Option<(u64, Option<&'a str>)> {
    let rest = path.strip_prefix(prefix)?;
    let (guild_id, suffix) = rest.split_once('/').map_or((rest, None), |(id, suffix)| (id, Some(suffix)));
    Some((guild_id.parse().ok()?, suffix))
}

async fn api_game_value(state: &WebSettingsState, guild_id: u64) -> Option<Value> {
    let running = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())?;
    let running = running.read().await;
    let mut players = running
        .game
        .players
        .iter()
        .map(|player| {
            json!({
                "user_id": player.user_id,
                "name": player.name,
                "alive": player.alive,
                "role": player.role.value(),
            })
        })
        .collect::<Vec<_>>();
    players.sort_by_key(|player| player["name"].as_str().unwrap_or_default().to_lowercase());
    Some(json!({
        "guild_id": guild_id,
        "channel_id": running.channel_id.get(),
        "phase": running.game.phase.value(),
        "day_number": running.game.day_number,
        "participant_count": running.game.players.len(),
        "alive_count": running.game.alive_players().len(),
        "dead_count": running.game.dead_players().len(),
        "spectator_count": running.spectator_user_ids.len(),
        "anonymous_enabled": running.anonymous_enabled,
        "phase_remaining_seconds": running.phase_deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()).as_secs()),
        "day_skip_votes": running.day_skip_voter_ids.len(),
        "day_skip_confirmed": running.day_skip_confirmed,
        "players": players,
    }))
}

async fn api_recruitment_value(state: &WebSettingsState, guild_id: u64) -> Option<Value> {
    let recruitment = state
        .recruitments
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())?;
    let recruitment = recruitment.read().await;
    let mut participants = recruitment
        .joined_ids
        .iter()
        .map(|user_id| {
            json!({
                "user_id": user_id,
                "name": recruitment.joined_names.get(user_id).cloned().unwrap_or_else(|| user_id.to_string()),
            })
        })
        .collect::<Vec<_>>();
    participants.sort_by_key(|player| player["name"].as_str().unwrap_or_default().to_lowercase());
    let mut spectators = recruitment
        .spectator_ids
        .iter()
        .map(|user_id| {
            json!({
                "user_id": user_id,
                "name": recruitment.spectator_names.get(user_id).cloned().unwrap_or_else(|| user_id.to_string()),
            })
        })
        .collect::<Vec<_>>();
    spectators.sort_by_key(|player| player["name"].as_str().unwrap_or_default().to_lowercase());
    let mut role_counts = recruitment
        .role_counts
        .iter()
        .map(|(role, count)| json!({"role": role.value(), "count": count}))
        .collect::<Vec<_>>();
    role_counts.sort_by_key(|item| item["role"].as_str().unwrap_or_default().to_string());
    Some(json!({
        "guild_id": guild_id,
        "host_user_id": recruitment.host_user_id.get(),
        "accepting": recruitment.accepting,
        "cancelled": recruitment.cancelled,
        "minimum_players": recruitment.minimum_players,
        "max_players": recruitment.max_players,
        "participant_count": participants.len(),
        "spectator_count": spectators.len(),
        "participants": participants,
        "spectators": spectators,
        "role_counts": role_counts,
        "special_roles": recruitment.special_roles.iter().map(|role| role.value()).collect::<Vec<_>>(),
    }))
}

async fn control_game(
    state: &WebSettingsState,
    guild_id: u64,
    action: &str,
) -> std::result::Result<Value, String> {
    let Some(running) = state
        .games
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    else {
        return Err("game not found".to_string());
    };
    let notifications = {
        let mut running = running.write().await;
        match action {
            "stop" => {
                if running.game.phase == Phase::Ended {
                    return Err("game is already ending".to_string());
                }
                running.game.phase = Phase::Ended;
                running.phase_deadline = None;
                vec![
                    running.night_notify.clone(),
                    running.vote_notify.clone(),
                    running.confirm_notify.clone(),
                    running.day_notify.clone(),
                ]
            }
            "skip_day" => {
                if running.game.phase != Phase::Day {
                    return Err("skip_day is only available during day discussion".to_string());
                }
                running.day_skip_confirmed = true;
                running.day_extension_active = false;
                vec![running.day_notify.clone()]
            }
            "extend_day" => {
                if running.game.phase != Phase::Day || !running.day_extension_active {
                    return Err("extend_day is only available during the day extension vote".to_string());
                }
                running.day_extension_confirmed = true;
                vec![running.day_notify.clone()]
            }
            _ => return Err("unsupported game action".to_string()),
        }
    };
    for notify in notifications {
        notify.notify_waiters();
    }
    Ok(json!({"ok": true, "guild_id": guild_id, "action": action}))
}

async fn cancel_recruitment(
    state: &WebSettingsState,
    guild_id: u64,
) -> std::result::Result<Value, String> {
    let Some(recruitment) = state
        .recruitments
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    else {
        return Err("recruitment not found".to_string());
    };
    let notify = {
        let mut recruitment = recruitment.write().await;
        if !recruitment.accepting {
            return Err("recruitment is no longer accepting players".to_string());
        }
        recruitment.cancelled = true;
        recruitment.accepting = false;
        recruitment.done.clone()
    };
    notify.notify_waiters();
    Ok(json!({"ok": true, "guild_id": guild_id, "action": "cancel"}))
}

async fn start_recruitment(
    state: &WebSettingsState,
    guild_id: u64,
) -> std::result::Result<Value, String> {
    let Some(recruitment) = state
        .recruitments
        .get(&serenity::GuildId::new(guild_id))
        .map(|entry| entry.value().clone())
    else {
        return Err("recruitment not found".to_string());
    };
    let notify = {
        let mut recruitment = recruitment.write().await;
        if !recruitment.accepting {
            return Err("recruitment is no longer accepting players".to_string());
        }
        if recruitment.joined_ids.len() < recruitment.minimum_players {
            return Err("not enough players to start".to_string());
        }
        recruitment.accepting = false;
        recruitment.done.clone()
    };
    notify.notify_waiters();
    Ok(json!({"ok": true, "guild_id": guild_id, "action": "start"}))
}

async fn route_protected_api_request(
    state: &WebSettingsState,
    request: &HttpRequest,
    path: &str,
    query: &str,
) -> Option<String> {
    if !path.starts_with("/api/v1/") {
        return None;
    }
    let key = match authenticate_api_key(state, request).await {
        Ok(key) => key,
        Err(error) => return Some(error.response()),
    };
    let query = parse_urlencoded(query);
    let response = match (request.method.as_str(), path) {
        ("GET", "/api/v1/me") => json_response(json!({"key": api_key_value(&key)})),
        ("GET", "/api/v1/config") => {
            let status = web_status_values(state).await;
            json_response(json!({"settings": status["settings"].clone()}))
        }
        ("GET", "/api/v1/stats") => json_response(web_stats_summary(state).await),
        ("GET", "/api/v1/games") => {
            let games = api_game_value(state, key.guild_id).await.into_iter().collect::<Vec<_>>();
            json_response(json!({"games": games}))
        }
        ("GET", "/api/v1/leaderboard") => {
            let limit = query
                .get("limit")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(10);
            json_response(web_leaderboard_values(state, "rating", limit).await)
        }
        _ => {
            if let Some(metric) = path.strip_prefix("/api/v1/leaderboard/") {
                if !WEB_LEADERBOARD_METRICS.contains(&metric) {
                    json_error("400 Bad Request", "unsupported leaderboard metric")
                } else {
                    let limit = query
                        .get("limit")
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(10);
                    json_response(web_leaderboard_values(state, metric, limit).await)
                }
            } else if let Some((guild_id, suffix)) = parse_api_guild_path(path, "/api/v1/games/") {
                if let Err(error) = require_key_guild(&key, guild_id) {
                    error.response()
                } else if suffix.is_none() && request.method == "GET" {
                    api_game_value(state, guild_id)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "game not found"))
                } else if suffix == Some("actions") && request.method == "POST" {
                    let action = serde_json::from_str::<Value>(&request.body)
                        .ok()
                        .and_then(|body| body.get("action").and_then(Value::as_str).map(str::to_string));
                    let Some(action) = action else {
                        return Some(json_error("400 Bad Request", "JSON body requires action"));
                    };
                    control_game(state, guild_id, &action)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|message| json_error("409 Conflict", &message))
                } else {
                    json_error("404 Not Found", "API endpoint not found")
                }
            } else if let Some((guild_id, suffix)) = parse_api_guild_path(path, "/api/v1/recruitments/") {
                if let Err(error) = require_key_guild(&key, guild_id) {
                    error.response()
                } else if suffix.is_none() && request.method == "GET" {
                    api_recruitment_value(state, guild_id)
                        .await
                        .map(json_response)
                        .unwrap_or_else(|| json_error("404 Not Found", "recruitment not found"))
                } else if suffix == Some("actions") && request.method == "POST" {
                    let action = serde_json::from_str::<Value>(&request.body)
                        .ok()
                        .and_then(|body| body.get("action").and_then(Value::as_str).map(str::to_string));
                    match action.as_deref() {
                        Some("cancel") => cancel_recruitment(state, guild_id)
                            .await
                            .map(json_response)
                            .unwrap_or_else(|message| json_error("409 Conflict", &message)),
                        Some("start") => start_recruitment(state, guild_id)
                            .await
                            .map(json_response)
                            .unwrap_or_else(|message| json_error("409 Conflict", &message)),
                        _ => json_error("400 Bad Request", "supported recruitment actions: start, cancel"),
                    }
                } else {
                    json_error("404 Not Found", "API endpoint not found")
                }
            } else {
                json_error("404 Not Found", "API endpoint not found")
            }
        }
    };
    Some(response)
}

async fn route_public_request(state: &WebSettingsState, path: &str, query: &str) -> Option<String> {
    let query = parse_urlencoded(query);
    match path {
        "/" => {
            let status = web_status_values(state).await;
            let leaderboard = web_leaderboard_values(state, "rating", 3).await;
            let stats = web_stats_summary(state).await;
            Some(http_response(
                "200 OK",
                &render_home_page(&status, &leaderboard, &stats),
            ))
        }
        "/status" => {
            let status = web_status_values(state).await;
            Some(http_response("200 OK", &render_status_page(&status)))
        }
        "/leaderboard" => {
            let metric = query.get("metric").map(String::as_str).unwrap_or("rating");
            let leaderboard = web_leaderboard_values(state, metric, 20).await;
            let stats = web_stats_summary(state).await;
            Some(http_response(
                "200 OK",
                &render_leaderboard_page(&leaderboard, &stats),
            ))
        }
        "/api" | "/api/docs" => Some(http_response(
            "200 OK",
            &render_api_docs_page(&state.base_url),
        )),
        "/health" => Some(json_response(
            json!({"ok": true, "service": "mafia-discord-bot"}),
        )),
        "/api/status" => Some(json_response(web_status_values(state).await)),
        "/api/games" => {
            let status = web_status_values(state).await;
            Some(json_response(json!({"games": status["games"].clone()})))
        }
        "/api/settings" => {
            let status = web_status_values(state).await;
            Some(json_response(
                json!({"settings": status["settings"].clone()}),
            ))
        }
        "/api/stats" => Some(json_response(web_stats_summary(state).await)),
        "/api/leaderboard" => {
            let limit = query
                .get("limit")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(10);
            Some(json_response(
                web_leaderboard_values(state, "rating", limit).await,
            ))
        }
        _ => {
            if let Some(metric) = path.strip_prefix("/api/leaderboard/") {
                let limit = query
                    .get("limit")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(10);
                Some(json_response(
                    web_leaderboard_values(state, metric, limit).await,
                ))
            } else {
                None
            }
        }
    }
}

fn valid_api_key_label(value: &str) -> std::result::Result<String, String> {
    let label = value.trim();
    if label.is_empty() || label.chars().count() > 64 || label.chars().any(char::is_control) {
        return Err("API 키 이름은 제어 문자 없이 1~64자여야 합니다.".to_string());
    }
    Ok(label.to_string())
}

fn api_key_records_for_guild(store: &ApiKeyStore, guild_id: u64) -> Vec<ApiKeyRecord> {
    let mut records = store
        .keys
        .iter()
        .filter(|record| record.guild_id == guild_id)
        .cloned()
        .collect::<Vec<_>>();
    records.sort_by_key(|record| std::cmp::Reverse(record.created_at.clone()));
    records
}

async fn route_api_key_management(
    state: &WebSettingsState,
    session: &WebSettingsSession,
    token: &str,
    request: &HttpRequest,
) -> String {
    let action = format!("{WEB_SETTINGS_PATH}/{token}/api-keys");
    match request.method.as_str() {
        "GET" => {
            let store = state.api_keys.read().await;
            let records = api_key_records_for_guild(&store, session.guild_id);
            http_response(
                "200 OK",
                &render_api_key_page(session, &action, &records, None, None),
            )
        }
        "POST" => {
            let form = parse_urlencoded(&request.body);
            let result = match form.get("action").map(String::as_str) {
                Some("create") => {
                    let label = form
                        .get("label")
                        .ok_or_else(|| "API 키 이름을 입력하세요.".to_string())
                        .and_then(|value| valid_api_key_label(value));
                    let label = match label {
                        Ok(label) => label,
                        Err(error) => return api_key_management_error(state, session, &action, error).await,
                    };
                    let mut store = state.api_keys.write().await;
                    let previous = store.clone();
                    let key = issue_api_key(&mut store, session.guild_id, session.user_id, label);
                    if let Err(error) = save_api_key_store(&*state.api_keys_path, &store) {
                        *store = previous;
                        let error = error.to_string();
                        drop(store);
                        return api_key_management_error(state, session, &action, error).await;
                    }
                    Ok(Some(key))
                }
                Some("revoke") => {
                    let Some(key_id) = form.get("key_id") else {
                        return api_key_management_error(
                            state,
                            session,
                            &action,
                            "폐기할 API 키를 선택하세요.".to_string(),
                        )
                        .await;
                    };
                    let mut store = state.api_keys.write().await;
                    let previous = store.clone();
                    let Some(record) = store
                        .keys
                        .iter_mut()
                        .find(|record| record.id == *key_id && record.guild_id == session.guild_id)
                    else {
                        drop(store);
                        return api_key_management_error(
                            state,
                            session,
                            &action,
                            "API 키를 찾을 수 없습니다.".to_string(),
                        )
                        .await;
                    };
                    record.revoked = true;
                    if let Err(error) = save_api_key_store(&*state.api_keys_path, &store) {
                        *store = previous;
                        let error = error.to_string();
                        drop(store);
                        return api_key_management_error(state, session, &action, error).await;
                    }
                    Ok(None)
                }
                _ => Err("지원하지 않는 API 키 작업입니다.".to_string()),
            };
            match result {
                Ok(issued_key) => {
                    let store = state.api_keys.read().await;
                    let records = api_key_records_for_guild(&store, session.guild_id);
                    http_response(
                        "200 OK",
                        &render_api_key_page(session, &action, &records, issued_key.as_deref(), None),
                    )
                }
                Err(error) => api_key_management_error(state, session, &action, error).await,
            }
        }
        _ => json_error("405 Method Not Allowed", "GET or POST is required"),
    }
}

async fn api_key_management_error(
    state: &WebSettingsState,
    session: &WebSettingsSession,
    action: &str,
    error: String,
) -> String {
    let store = state.api_keys.read().await;
    let records = api_key_records_for_guild(&store, session.guild_id);
    http_response(
        "400 Bad Request",
        &render_api_key_page(session, action, &records, None, Some(&error)),
    )
}

async fn web_status_values(state: &WebSettingsState) -> Value {
    let now = Instant::now();
    let config = state.config.read().await.clone();
    let mut games = Vec::new();
    for entry in state.games.iter() {
        let guild_id = entry.key().get();
        let running = entry.value().read().await;
        let alive_count = running.game.alive_players().len();
        let dead_count = running.game.dead_players().len();
        games.push(json!({
            "guild_id": guild_id,
            "guild_name": guild_id.to_string(),
            "channel_id": running.channel_id.get(),
            "channel_name": format!("#{}", running.channel_id.get()),
            "phase": running.game.phase.value(),
            "day": format!("{}일차", running.game.day_number),
            "participant_count": running.game.players.len(),
            "alive_count": alive_count,
            "dead_count": dead_count,
            "spectator_count": running.spectator_user_ids.len(),
            "anonymous_enabled": running.anonymous_enabled,
            "elapsed": stats::play_duration_text(running.started_at.elapsed().as_secs() as i64),
        }));
    }
    games.sort_by_key(|item| {
        item.get("guild_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    });
    json!({
        "bot": {
            "ready": true,
            "name": state.bot_name,
            "latency_ms": 0,
            "guild_count": state.guild_count,
            "user_count": 0,
            "uptime": stats::play_duration_text(now.duration_since(state.started_at).as_secs() as i64),
        },
        "api": {
            "base_url": format!("{}/api", state.base_url.trim_end_matches('/')),
        },
        "games": games,
        "recruiting_guild_count": state.recruitments.len(),
        "settings": {
            "game_enabled": config.game_enabled,
            "max_player_count_text": if config.max_player_count == 0 {
                "제한 없음".to_string()
            } else {
                format!("{}명", config.max_player_count)
            },
            "role_summary": format!(
                "마피아 {}명, 의사 {}명, 수사직 {}명",
                config.default_mafia_count, config.default_doctor_count, config.default_police_count
            ),
            "special_summary": format!(
                "시민 {}개, 마피아 {}개, 중립 {}개",
                config.citizen_special_count, config.mafia_special_count, config.neutral_special_count
            ),
            "anonymous_mode_text": if config.anonymous_mode {
                format!("켜짐 ({})", match config.anonymous_name_mode.as_str() {
                    "number" => "숫자",
                    _ => "동물",
                })
            } else {
                "꺼짐".to_string()
            },
            "slowmode_text": format!("{}초", config.chat_slowmode_seconds),
            "cult_team_text": if config.enable_cult_team { "켜짐" } else { "꺼짐" },
        }
    })
}

async fn web_stats_summary(state: &WebSettingsState) -> Value {
    let stats_read = state.stats.read().await;
    let entries = stats_read.users.values().collect::<Vec<_>>();
    let played_entries = entries
        .iter()
        .copied()
        .filter(|entry| entry.games > 0)
        .collect::<Vec<_>>();
    let total_player_games = played_entries.iter().map(|entry| entry.games).sum::<i64>();
    let total_wins = played_entries.iter().map(|entry| entry.wins).sum::<i64>();
    let total_play_seconds = played_entries
        .iter()
        .map(|entry| entry.play_seconds)
        .sum::<i64>();
    let average_rating = if played_entries.is_empty() {
        stats::INITIAL_RATING
    } else {
        (played_entries.iter().map(|entry| entry.rating).sum::<i64>() as f64
            / played_entries.len() as f64)
            .round() as i64
    };
    json!({
        "registered_users": entries.len(),
        "recorded_players": played_entries.len(),
        "total_player_games": total_player_games,
        "total_wins": total_wins,
        "total_playtime": stats::play_duration_text(total_play_seconds),
        "total_play_seconds": total_play_seconds,
        "average_rating": average_rating,
    })
}

async fn web_leaderboard_values(state: &WebSettingsState, metric: &str, limit: usize) -> Value {
    let metric = if WEB_LEADERBOARD_METRICS.contains(&metric) {
        metric
    } else {
        "rating"
    };
    let safe_limit = limit.clamp(1, 50);
    let stats_read = state.stats.read().await;
    let entries = stats::leaderboard_entries(&stats_read, metric, safe_limit)
        .into_iter()
        .enumerate()
        .map(|(index, (user_id, entry))| {
            let winrate = if entry.games > 0 {
                ((entry.wins as f64 / entry.games as f64 * 1000.0).round()) / 10.0
            } else {
                0.0
            };
            json!({
                "rank": index + 1,
                "user_id": user_id,
                "name": if entry.name.is_empty() { "알 수 없음".to_string() } else { entry.name.clone() },
                "games": entry.games,
                "wins": entry.wins,
                "losses": entry.losses,
                "winrate": winrate,
                "winrate_text": stats::win_rate_text(entry.wins, entry.games),
                "mafia_team_games": entry.mafia_team_games,
                "play_seconds": entry.play_seconds,
                "playtime": stats::play_duration_text(entry.play_seconds),
                "rating": entry.rating,
                "rating_peak": entry.rating_peak,
                "rating_games": entry.rating_games,
                "value": stats::leaderboard_value(&entry, metric),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "metric": metric,
        "metric_name": stats::leaderboard_metric_name(metric),
        "metrics": WEB_LEADERBOARD_METRICS
            .iter()
            .map(|key| json!({"key": key, "name": stats::leaderboard_metric_name(key)}))
            .collect::<Vec<_>>(),
        "limit": safe_limit,
        "entries": entries,
    })
}

fn render_settings_page(
    session: &WebSettingsSession,
    action: &str,
    config: &BotConfig,
    status: Option<&Value>,
    error: Option<&str>,
) -> String {
    let message_html = error.map_or_else(String::new, |message| {
        format!(
            r#"<p class="message error">⚠️ {}</p>"#,
            html_escape(message)
        )
    });
    let rows = WEB_CONFIG_FIELDS
        .iter()
        .map(|field| render_field(*field, config))
        .collect::<Vec<_>>()
        .join("\n");
    let status_html = status.map(render_status_summary).unwrap_or_default();
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>마피아 게임 설정</title>
{WEB_PAGE_STYLE}
</head>
<body>
<div class="site-shell">
{}
<p class="meta">{} 님 전용 1회용 링크입니다. 저장하면 이 링크는 더 이상 사용할 수 없습니다.</p>
{}
{message_html}
<form method="post" action="{}">
  <fieldset>
    <legend>설정 항목</legend>
    {rows}
  </fieldset>
  <button type="submit">저장하기</button>
</form>
<p><a href="{}/api-keys">API 키 관리</a></p>
</main>
</div>
</body>
</html>"#,
        render_page_header("🕵️ 마피아 게임 웹 설정", false),
        html_escape(&session.user_label),
        status_html,
        html_escape(action),
        html_escape(action)
    )
}

fn render_api_key_page(
    session: &WebSettingsSession,
    action: &str,
    records: &[ApiKeyRecord],
    issued_key: Option<&str>,
    error: Option<&str>,
) -> String {
    let message_html = error.map_or_else(String::new, |message| {
        format!(r#"<p class="message error">⚠️ {}</p>"#, html_escape(message))
    });
    let issued_html = issued_key.map_or_else(String::new, |key| {
        format!(
            r#"<section class="panel"><h2>새 API 키</h2><p class="message error">이 키는 지금 한 번만 표시됩니다. 안전한 곳에 보관하세요.</p><pre>{}</pre></section>"#,
            html_escape(key)
        )
    });
    let rows = records
        .iter()
        .map(|record| {
            let state = if record.revoked { "폐기됨" } else { "활성" };
            let action = if record.revoked {
                String::new()
            } else {
                format!(
                    r#"<form method="post" action="{action}"><input type="hidden" name="action" value="revoke"><input type="hidden" name="key_id" value="{}"><button type="submit">폐기</button></form>"#,
                    html_escape(&record.id)
                )
            };
            format!(
                r#"<tr><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{action}</td></tr>"#,
                html_escape(&record.label),
                html_escape(&record.id),
                html_escape(&record.created_at),
                state,
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let table = if rows.is_empty() {
        "<p class=\"meta\">발급된 API 키가 없습니다.</p>".to_string()
    } else {
        format!(
            r#"<table><thead><tr><th>이름</th><th>키 ID</th><th>발급 시각</th><th>상태</th><th></th></tr></thead><tbody>{rows}</tbody></table>"#
        )
    };
    let settings_path = action.trim_end_matches("/api-keys");
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>마피아 API 키 관리</title>
{WEB_PAGE_STYLE}
</head>
<body>
<div class="site-shell">
{}
<p class="meta">{} 서버 전용 키입니다. 발급된 키는 이 서버의 보호 API만 사용할 수 있습니다.</p>
{message_html}
{issued_html}
<section class="panel"><h2>키 발급</h2><form method="post" action="{action}"><input type="hidden" name="action" value="create"><label class="row" for="label"><span>키 이름</span><input type="text" id="label" name="label" maxlength="64" required></label><button type="submit">키 발급</button></form></section>
<section class="panel"><h2>발급된 키</h2>{table}</section>
<p><a href="{settings_path}">설정으로 돌아가기</a></p>
</main>
</div>
</body>
</html>"#,
        render_page_header("마피아 API 키 관리", false),
        html_escape(&session.user_label),
        action = html_escape(action),
        settings_path = html_escape(settings_path),
    )
}

fn safe_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => html_escape(text),
        Some(Value::Number(number)) => html_escape(&number.to_string()),
        Some(Value::Bool(value)) => html_escape(&value.to_string()),
        _ => "-".to_string(),
    }
}

fn render_nav() -> &'static str {
    r#"<nav class="nav"><a href="/">홈</a><a href="/status">상태판</a><a href="/leaderboard">리더보드</a><a href="/api/docs">API 문서</a></nav>"#
}

fn render_status_summary(status: &Value) -> String {
    let bot = status.get("bot").unwrap_or(&Value::Null);
    let settings = status.get("settings").unwrap_or(&Value::Null);
    let games_len = status
        .get("games")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let cards = [
        (
            "봇 상태",
            if bot["ready"].as_bool().unwrap_or(false) {
                "온라인".to_string()
            } else {
                "시작 중".to_string()
            },
        ),
        ("서버 수", safe_text(bot.get("guild_count"))),
        ("진행 중 게임", games_len.to_string()),
        (
            "모집 중 서버",
            safe_text(status.get("recruiting_guild_count")),
        ),
        (
            "게임 시작",
            if settings["game_enabled"].as_bool().unwrap_or(false) {
                "활성화".to_string()
            } else {
                "비활성화".to_string()
            },
        ),
        ("업타임", safe_text(bot.get("uptime"))),
    ];
    format!(
        r#"<section class="grid">{}</section>"#,
        cards
            .into_iter()
            .map(|(label, value)| format!(
                r#"<div class="card"><span>{}</span><strong>{}</strong></div>"#,
                html_escape(label),
                value
            ))
            .collect::<Vec<_>>()
            .join("")
    )
}

fn render_games_table(status: &Value) -> String {
    let Some(games) = status.get("games").and_then(Value::as_array) else {
        return r#"<section class="panel"><h2>진행 중 게임</h2><p class="meta">현재 진행 중인 게임이 없습니다.</p></section>"#.to_string();
    };
    if games.is_empty() {
        return r#"<section class="panel"><h2>진행 중 게임</h2><p class="meta">현재 진행 중인 게임이 없습니다.</p></section>"#.to_string();
    }
    let rows = games
        .iter()
        .map(|item| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}/{}</td><td>{}</td><td>{}</td></tr>",
                safe_text(item.get("guild_name")),
                safe_text(item.get("channel_name")),
                safe_text(item.get("phase")),
                safe_text(item.get("day")),
                safe_text(item.get("alive_count")),
                safe_text(item.get("participant_count")),
                safe_text(item.get("dead_count")),
                safe_text(item.get("elapsed")),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"<section class="panel"><h2>진행 중 게임</h2><table><thead><tr><th>서버</th><th>채널</th><th>단계</th><th>일차</th><th>생존/참가</th><th>사망</th><th>진행 시간</th></tr></thead><tbody>{rows}</tbody></table></section>"#
    )
}

fn base_html(title: &str, body: &str, auto_refresh: bool) -> String {
    let refresh = if auto_refresh {
        r#"<meta http-equiv="refresh" content="20">"#
    } else {
        ""
    };
    format!(
        r#"<!DOCTYPE html><html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex">{refresh}<title>{}</title>{WEB_PAGE_STYLE}</head><body><div class="site-shell">{}{body}</main></div></body></html>"#,
        html_escape(title),
        render_page_header(title, true),
    )
}

fn render_page_header(title: &str, with_nav: bool) -> String {
    let nav = if with_nav { render_nav() } else { "" };
    format!(
        r#"<header class="site-header"><a class="site-mark" href="/" aria-label="마피아 봇 홈">M</a><div><p class="eyebrow">MAFIA REMAKE</p><h1>{}</h1></div></header>{nav}<main>"#,
        html_escape(title),
    )
}

fn render_home_page(status: &Value, leaderboard: &Value, stats_summary: &Value) -> String {
    let body = format!(
        r#"<p class="meta">봇 상태와 전적을 한눈에 보는 홈입니다. 상태 정보는 20초마다 자동 새로고침됩니다.</p>{}{}{}"#,
        render_status_summary(status),
        render_games_table(status),
        render_stats_cards(stats_summary),
    );
    let body = format!(
        "{body}<section class=\"panel\"><h2>레이팅 TOP 3</h2>{}</section>",
        render_leaderboard_podium(leaderboard)
    );
    base_html("마피아 봇 홈", &body, true)
}

fn render_status_page(status: &Value) -> String {
    let settings = status.get("settings").unwrap_or(&Value::Null);
    let rows = [
        (
            "최대 인원",
            safe_text(settings.get("max_player_count_text")),
        ),
        ("기본 구성", safe_text(settings.get("role_summary"))),
        ("특수룰 수", safe_text(settings.get("special_summary"))),
        ("익명 채팅", safe_text(settings.get("anonymous_mode_text"))),
        ("채팅 슬로우모드", safe_text(settings.get("slowmode_text"))),
        ("교주팀", safe_text(settings.get("cult_team_text"))),
    ]
    .into_iter()
    .map(|(label, value)| format!("<tr><th>{}</th><td>{value}</td></tr>", html_escape(label)))
    .collect::<Vec<_>>()
    .join("");
    let body = format!(
        r#"<p class="meta">진행 중 게임, 서버 연결 상태, 주요 게임 설정만 보여줍니다. 20초마다 자동 새로고침됩니다.</p>{}<section class="panel"><h2>현재 주요 설정</h2><table><tbody>{rows}</tbody></table></section>{}"#,
        render_status_summary(status),
        render_games_table(status),
    );
    base_html("마피아 봇 상태판", &body, true)
}

fn render_stats_cards(stats_summary: &Value) -> String {
    let cards = [
        (
            "기록된 유저",
            safe_text(stats_summary.get("recorded_players")),
        ),
        (
            "누적 플레이",
            safe_text(stats_summary.get("total_player_games")),
        ),
        ("누적 시간", safe_text(stats_summary.get("total_playtime"))),
        (
            "평균 레이팅",
            safe_text(stats_summary.get("average_rating")),
        ),
    ];
    format!(
        r#"<section class="grid">{}</section>"#,
        cards
            .into_iter()
            .map(|(label, value)| format!(
                r#"<div class="card"><span>{}</span><strong>{value}</strong></div>"#,
                html_escape(label)
            ))
            .collect::<Vec<_>>()
            .join("")
    )
}

fn render_metric_tabs(leaderboard: &Value) -> String {
    let current = leaderboard
        .get("metric")
        .and_then(Value::as_str)
        .unwrap_or("rating");
    let Some(metrics) = leaderboard.get("metrics").and_then(Value::as_array) else {
        return String::new();
    };
    let links = metrics
        .iter()
        .filter_map(|metric| {
            let key = metric.get("key").and_then(Value::as_str)?;
            let name = metric.get("name").and_then(Value::as_str).unwrap_or(key);
            let class_attr = if key == current {
                r#" class="active""#
            } else {
                ""
            };
            Some(format!(
                r#"<a href="/leaderboard?metric={}"{}>{}</a>"#,
                html_escape(key),
                class_attr,
                html_escape(name)
            ))
        })
        .collect::<Vec<_>>()
        .join("");
    format!(r#"<div class="metric-tabs">{links}</div>"#)
}

fn render_leaderboard_podium(leaderboard: &Value) -> String {
    let entries = leaderboard
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return r#"<p class="meta">아직 기록된 게임 전적이 없습니다.</p>"#.to_string();
    }
    let cards = entries
        .iter()
        .take(3)
        .map(|entry| {
            format!(
                r#"<div class="podium-card"><div class="rank">#{}</div><div class="name">{}</div><div class="rating">{}점</div><div class="meta">{}승 {}패 · 승률 {}</div></div>"#,
                safe_text(entry.get("rank")),
                safe_text(entry.get("name")),
                safe_text(entry.get("rating")),
                safe_text(entry.get("wins")),
                safe_text(entry.get("losses")),
                safe_text(entry.get("winrate_text")),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(r#"<div class="podium">{cards}</div>"#)
}

fn render_leaderboard_page(leaderboard: &Value, stats_summary: &Value) -> String {
    let body = format!(
        r#"<p class="meta">현재 기준: <span class="pill">{}</span></p>{}{}{}{}"#,
        safe_text(leaderboard.get("metric_name")),
        render_metric_tabs(leaderboard),
        render_leaderboard_podium(leaderboard),
        render_leaderboard_table(leaderboard, false),
        render_stats_cards(stats_summary),
    );
    base_html("마피아 리더보드", &body, false)
}

fn render_leaderboard_table(leaderboard: &Value, compact: bool) -> String {
    let entries = leaderboard
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return r#"<p class="meta">아직 기록된 게임 전적이 없습니다.</p>"#.to_string();
    }
    let rows = entries
        .iter()
        .map(|entry| {
            format!(
                r#"<tr><td class="num">{}</td><td>{}</td><td class="num">{}</td><td>{}승 {}패</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td>{}</td></tr>"#,
                safe_text(entry.get("rank")),
                safe_text(entry.get("name")),
                safe_text(entry.get("rating")),
                safe_text(entry.get("wins")),
                safe_text(entry.get("losses")),
                safe_text(entry.get("winrate_text")),
                safe_text(entry.get("games")),
                safe_text(entry.get("mafia_team_games")),
                safe_text(entry.get("playtime")),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let title = if compact {
        ""
    } else {
        "<h2>전체 순위</h2>"
    };
    format!(
        r#"<section class="panel">{title}<table><thead><tr><th class="num">순위</th><th>이름</th><th class="num">레이팅</th><th>승패</th><th class="num">승률</th><th class="num">판수</th><th class="num">마피아팀</th><th>게임시간</th></tr></thead><tbody>{rows}</tbody></table></section>"#
    )
}

fn render_api_docs_page(base_url: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    let api_url = format!("{base_url}/api");
    let protected_api_url = format!("{api_url}/v1");
    let public_endpoints = [
        ("GET /health", "봇 웹 서버가 살아 있는지 확인합니다."),
        (
            "GET /api/status",
            "봇 연결 상태, 진행 중 게임, 공개 설정 요약을 반환합니다.",
        ),
        ("GET /api/games", "진행 중 게임 목록만 반환합니다."),
        (
            "GET /api/settings",
            "공개 가능한 게임 설정 요약을 반환합니다.",
        ),
        ("GET /api/stats", "전적 요약 정보를 반환합니다."),
        ("GET /api/leaderboard", "레이팅 기준 리더보드를 반환합니다."),
        (
            "GET /api/leaderboard/{metric}",
            "wins, winrate, games, mafia, playtime, rating 기준 리더보드를 반환합니다.",
        ),
    ];
    let protected_endpoints = [
        ("GET /api/v1/me", "API 키 정보와 서버 범위를 반환합니다. API 키 필요."),
        ("GET /api/v1/config", "게임 설정 요약을 반환합니다. API 키 필요."),
        ("GET /api/v1/stats", "전적 요약을 반환합니다. API 키 필요."),
        (
            "GET /api/v1/leaderboard/{metric}",
            "보호 리더보드를 반환합니다. API 키 필요.",
        ),
        ("GET /api/v1/games", "키 발급 서버의 진행 중 게임을 반환합니다. API 키 필요."),
        (
            "GET /api/v1/games/{guild_id}",
            "참가자, 직업, 단계, 타이머를 포함한 게임 상세를 반환합니다. API 키 필요.",
        ),
        (
            "POST /api/v1/games/{guild_id}/actions",
            "JSON action: skip_day, extend_day 또는 stop. API 키 필요.",
        ),
        (
            "GET /api/v1/recruitments/{guild_id}",
            "모집 인원과 역할 구성을 반환합니다. API 키 필요.",
        ),
        (
            "POST /api/v1/recruitments/{guild_id}/actions",
            "JSON action: start 또는 cancel. API 키 필요.",
        ),
    ];
    let render_rows = |endpoints: &[(&str, &str)]| endpoints
        .iter()
        .map(|(path, desc)| {
            format!(
                r#"<div class="endpoint"><code>{}</code><span>{}</span></div>"#,
                html_escape(path),
                html_escape(desc)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let public_rows = render_rows(&public_endpoints);
    let protected_rows = render_rows(&protected_endpoints);
    let body = format!(
        r#"<p class="meta">기본 API 주소는 <code>{api_url}</code>입니다. 모든 응답은 JSON이며, <code>limit</code>은 1~50 범위입니다.</p>
<section class="panel"><h2>인증</h2><p>관리자는 <code>/마피아웹설정</code>에서 서버 전용 API 키를 발급합니다. 보호 API는 키 발급 서버의 데이터와 작업만 허용합니다.</p><pre>X-API-Key: mfr_...
Authorization: Bearer mfr_...</pre></section>
<section class="panel"><h2>공개 조회 API</h2>{public_rows}</section>
<section class="panel"><h2>보호 관리 API</h2>{protected_rows}</section>
<section class="panel"><h2>관리 작업 본문</h2><pre>POST {protected_api_url}/games/{{guild_id}}/actions
{{"action":"skip_day"}}   # 낮 토론 즉시 종료
{{"action":"extend_day"}} # 연장 투표 중 1분 연장 승인
{{"action":"stop"}}       # 게임 종료

POST {protected_api_url}/recruitments/{{guild_id}}/actions
{{"action":"start"}}      # 최소 인원 충족 시 즉시 시작
{{"action":"cancel"}}     # 모집 취소</pre></section>
<section class="panel"><h2>응답 코드</h2><pre>200 성공 · 400 잘못된 요청 · 401 키 없음/오류 · 403 다른 서버 키 · 404 대상 없음 · 409 현재 상태에서 작업 불가</pre></section>
<section class="panel"><h2>호출 예시</h2><pre>curl -H "X-API-Key: mfr_..." {protected_api_url}/games/123

curl -X POST -H "Authorization: Bearer mfr_..." -H "Content-Type: application/json" \
  -d '{{"action":"skip_day"}}' {protected_api_url}/games/123/actions</pre></section>"#,
        api_url = html_escape(&api_url),
        protected_api_url = html_escape(&protected_api_url),
    );
    base_html("마피아 봇 API 문서", &body, false)
}

fn render_field(field: WebConfigField, config: &BotConfig) -> String {
    let field_id = format!("field_{}", field.name);
    let label = html_escape(field.label);
    match field.kind {
        WebFieldKind::Bool => {
            let checked = if config_value(config, field.name) == "true" {
                " checked"
            } else {
                ""
            };
            format!(
                r#"<label class="row" for="{field_id}"><span>{label}</span><input type="checkbox" id="{field_id}" name="{}"{checked}></label>"#,
                field.name
            )
        }
        WebFieldKind::Int => {
            let min_attr = field
                .min_value
                .map(|value| format!(r#" min="{value}""#))
                .unwrap_or_default();
            format!(
                r#"<label class="row" for="{field_id}"><span>{label}</span><input type="number" id="{field_id}" name="{}" value="{}"{min_attr} required></label>"#,
                field.name,
                html_escape(&config_value(config, field.name))
            )
        }
        WebFieldKind::Text => format!(
            r#"<label class="row" for="{field_id}"><span>{label}</span><input type="text" id="{field_id}" name="{}" value="{}" required></label>"#,
            field.name,
            html_escape(&config_value(config, field.name))
        ),
        WebFieldKind::IntList => {
            let value = config
                .blacklist_user_ids
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                r#"<label class="row" for="{field_id}"><span>{label}<br><small>한 줄에 하나씩, 또는 쉼표/공백으로 구분</small></span><textarea id="{field_id}" name="{}">{}</textarea></label>"#,
                field.name,
                html_escape(&value)
            )
        }
    }
}

fn render_message_page(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>{}</title>
{WEB_PAGE_STYLE}
</head>
<body>
<div class="site-shell">
{}
<p>{}</p>
</main>
</div>
</body>
</html>"#,
        html_escape(title),
        render_page_header(title, false),
        html_escape(message)
    )
}

fn expired_page() -> String {
    render_message_page(
        "🔒 링크가 만료되었습니다",
        "이 링크는 더 이상 유효하지 않습니다. 디스코드에서 /마피아웹설정 명령어를 다시 실행해 새 링크를 발급받으세요.",
    )
}

fn saved_page() -> String {
    render_message_page(
        "✅ 설정을 저장했습니다",
        "마피아 게임 설정이 반영되었습니다. 이 창은 닫으셔도 됩니다.",
    )
}

fn config_value(config: &BotConfig, name: &str) -> String {
    match name {
        "participant_role" => config.participant_role.clone(),
        "manager_role" => config.manager_role.clone(),
        "game_enabled" => config.game_enabled.to_string(),
        "max_player_count" => config.max_player_count.to_string(),
        "night_seconds" => config.night_seconds.to_string(),
        "discussion_seconds" => config.discussion_seconds.to_string(),
        "vote_seconds" => config.vote_seconds.to_string(),
        "chat_slowmode_seconds" => config.chat_slowmode_seconds.to_string(),
        "default_mafia_count" => config.default_mafia_count.to_string(),
        "default_doctor_count" => config.default_doctor_count.to_string(),
        "default_police_count" => config.default_police_count.to_string(),
        "default_joker_count" => config.default_joker_count.to_string(),
        "citizen_special_count" => config.citizen_special_count.to_string(),
        "mafia_special_count" => config.mafia_special_count.to_string(),
        "neutral_special_count" => config.neutral_special_count.to_string(),
        "reveal_death_roles" => config.reveal_death_roles.to_string(),
        "reveal_public_police_status" => config.reveal_public_police_status.to_string(),
        "reveal_morning_mafia_count" => config.reveal_morning_mafia_count.to_string(),
        "anonymous_mode" => config.anonymous_mode.to_string(),
        "anonymous_name_mode" => config.anonymous_name_mode.clone(),
        "use_agent" => config.use_agent.to_string(),
        "use_vigilante" => config.use_vigilante.to_string(),
        "enable_detective" => config.enable_detective.to_string(),
        "enable_graverobber" => config.enable_graverobber.to_string(),
        "enable_spy" => config.enable_spy.to_string(),
        "enable_contractor" => config.enable_contractor.to_string(),
        "enable_witch" => config.enable_witch.to_string(),
        "enable_scientist" => config.enable_scientist.to_string(),
        "enable_madam" => config.enable_madam.to_string(),
        "enable_godfather" => config.enable_godfather.to_string(),
        "enable_joker" => config.enable_joker.to_string(),
        "enable_politician" => config.enable_politician.to_string(),
        "enable_judge" => config.enable_judge.to_string(),
        "enable_reporter" => config.enable_reporter.to_string(),
        "enable_hacker" => config.enable_hacker.to_string(),
        "enable_terrorist" => config.enable_terrorist.to_string(),
        "enable_lover" => config.enable_lover.to_string(),
        "enable_shaman" => config.enable_shaman.to_string(),
        "enable_priest" => config.enable_priest.to_string(),
        "enable_soldier" => config.enable_soldier.to_string(),
        "enable_nurse" => config.enable_nurse.to_string(),
        "enable_gangster" => config.enable_gangster.to_string(),
        "enable_prophet" => config.enable_prophet.to_string(),
        "enable_psychologist" => config.enable_psychologist.to_string(),
        "enable_thief" => config.enable_thief.to_string(),
        "enable_cult_team" => config.enable_cult_team.to_string(),
        "blacklist_user_ids" => config
            .blacklist_user_ids
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_form_updates(body: &str) -> std::result::Result<HashMap<String, String>, String> {
    let raw_form = parse_urlencoded(body);
    let mut updates = HashMap::new();
    for field in WEB_CONFIG_FIELDS {
        if matches!(field.kind, WebFieldKind::Bool) {
            updates.insert(
                field.name.to_string(),
                raw_form.contains_key(field.name).to_string(),
            );
            continue;
        }
        let raw_value = raw_form
            .get(field.name)
            .ok_or_else(|| format!("'{}' 값이 비어 있습니다.", field.label))?;
        let text_value = raw_value.trim();
        if matches!(field.kind, WebFieldKind::IntList) && text_value.is_empty() {
            updates.insert(field.name.to_string(), String::new());
            continue;
        }
        if text_value.is_empty() {
            return Err(format!("'{}' 값이 비어 있습니다.", field.label));
        }
        if matches!(field.kind, WebFieldKind::Int) {
            let parsed = text_value
                .parse::<u64>()
                .map_err(|_| format!("'{}' 값은 숫자여야 합니다.", field.label))?;
            if let Some(min_value) = field.min_value
                && parsed < min_value
            {
                return Err(format!(
                    "'{}' 값은 {min_value} 이상이어야 합니다.",
                    field.label
                ));
            }
        }
        updates.insert(field.name.to_string(), text_value.to_string());
    }
    Ok(updates)
}

fn apply_updates(
    config: &mut BotConfig,
    updates: &HashMap<String, String>,
) -> std::result::Result<(), String> {
    let previous = config.clone();
    for field in WEB_CONFIG_FIELDS {
        let value = updates
            .get(field.name)
            .ok_or_else(|| format!("'{}' 값이 비어 있습니다.", field.label))?;
        match field.kind {
            WebFieldKind::Bool => set_bool(config, field.name, value == "true")?,
            WebFieldKind::Text => set_text(config, field.name, value.clone())?,
            WebFieldKind::Int => set_int(config, field.name, value.parse::<u64>().unwrap_or(0))?,
            WebFieldKind::IntList => set_int_list(config, field.name, value)?,
        }
    }
    if let Err(error) = validate_config(config) {
        *config = previous;
        return Err(error);
    }
    Ok(())
}

fn set_bool(config: &mut BotConfig, name: &str, value: bool) -> std::result::Result<(), String> {
    match name {
        "game_enabled" => config.game_enabled = value,
        "reveal_death_roles" => config.reveal_death_roles = value,
        "reveal_public_police_status" => config.reveal_public_police_status = value,
        "reveal_morning_mafia_count" => config.reveal_morning_mafia_count = value,
        "anonymous_mode" => config.anonymous_mode = value,
        "use_agent" => config.use_agent = value,
        "use_vigilante" => config.use_vigilante = value,
        "enable_detective" => config.enable_detective = value,
        "enable_graverobber" => config.enable_graverobber = value,
        "enable_spy" => config.enable_spy = value,
        "enable_contractor" => config.enable_contractor = value,
        "enable_witch" => config.enable_witch = value,
        "enable_scientist" => config.enable_scientist = value,
        "enable_madam" => config.enable_madam = value,
        "enable_godfather" => config.enable_godfather = value,
        "enable_joker" => config.enable_joker = value,
        "enable_politician" => config.enable_politician = value,
        "enable_judge" => config.enable_judge = value,
        "enable_reporter" => config.enable_reporter = value,
        "enable_hacker" => config.enable_hacker = value,
        "enable_terrorist" => config.enable_terrorist = value,
        "enable_lover" => config.enable_lover = value,
        "enable_shaman" => config.enable_shaman = value,
        "enable_priest" => config.enable_priest = value,
        "enable_soldier" => config.enable_soldier = value,
        "enable_nurse" => config.enable_nurse = value,
        "enable_gangster" => config.enable_gangster = value,
        "enable_prophet" => config.enable_prophet = value,
        "enable_psychologist" => config.enable_psychologist = value,
        "enable_thief" => config.enable_thief = value,
        "enable_cult_team" => config.enable_cult_team = value,
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

fn set_text(config: &mut BotConfig, name: &str, value: String) -> std::result::Result<(), String> {
    match name {
        "participant_role" => config.participant_role = value,
        "manager_role" => config.manager_role = value,
        "anonymous_name_mode" => config.anonymous_name_mode = value,
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

fn set_int(config: &mut BotConfig, name: &str, value: u64) -> std::result::Result<(), String> {
    match name {
        "max_player_count" => config.max_player_count = value as u32,
        "night_seconds" => config.night_seconds = value,
        "discussion_seconds" => config.discussion_seconds = value,
        "vote_seconds" => config.vote_seconds = value,
        "chat_slowmode_seconds" => config.chat_slowmode_seconds = value,
        "default_mafia_count" => config.default_mafia_count = value as u32,
        "default_doctor_count" => config.default_doctor_count = value as u32,
        "default_police_count" => config.default_police_count = value as u32,
        "default_joker_count" => config.default_joker_count = value as u32,
        "citizen_special_count" => config.citizen_special_count = value as u32,
        "mafia_special_count" => config.mafia_special_count = value as u32,
        "neutral_special_count" => config.neutral_special_count = value as u32,
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

fn set_int_list(
    config: &mut BotConfig,
    name: &str,
    value: &str,
) -> std::result::Result<(), String> {
    match name {
        "blacklist_user_ids" => {
            let normalized = value.replace(',', " ");
            let mut values = Vec::new();
            for chunk in normalized.split_whitespace() {
                values.push(chunk.parse::<u64>().map_err(|_| {
                    "블랙리스트 유저 ID 목록에는 숫자 ID만 입력할 수 있습니다.".to_string()
                })?);
            }
            values.sort_unstable();
            values.dedup();
            config.blacklist_user_ids = values;
        }
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

fn validate_config(config: &BotConfig) -> std::result::Result<(), String> {
    if config.default_mafia_count < 1 {
        return Err("마피아는 최소 1명이어야 합니다.".to_string());
    }
    let citizen_enabled = enabled_special_count(config, CITIZEN_SPECIAL_ROLES);
    if config.citizen_special_count as usize > citizen_enabled {
        return Err("시민 특수룰 수가 활성화된 시민 특수 역할보다 많습니다.".to_string());
    }
    let mafia_enabled = enabled_special_count(config, MAFIA_SPECIAL_ROLES);
    if config.mafia_special_count as usize > mafia_enabled {
        return Err("마피아 특수룰 수가 활성화된 마피아 특수 역할보다 많습니다.".to_string());
    }
    let neutral_enabled = enabled_special_count(config, NEUTRAL_SPECIAL_ROLES);
    if config.neutral_special_count as usize > neutral_enabled {
        return Err("중립 특수룰 수가 활성화된 중립 특수 역할보다 많습니다.".to_string());
    }
    if config.mafia_special_count > config.default_mafia_count {
        return Err(format!(
            "마피아 특수룰 수는 전체 마피아 수보다 많을 수 없습니다. 현재 마피아 {}명, 마피아 특수 {}명입니다.",
            config.default_mafia_count, config.mafia_special_count
        ));
    }
    if config
        .default_mafia_count
        .saturating_sub(config.mafia_special_count)
        < 1
    {
        return Err("접선 전 특수 마피아만으로는 게임을 진행할 수 없습니다. 일반 마피아가 최소 1명 필요합니다.".to_string());
    }
    let minimum_players = minimum_player_count(config);
    let max_players = if config.max_player_count == 0 {
        MAX_GAME_PLAYERS
    } else {
        (config.max_player_count as usize).min(MAX_GAME_PLAYERS)
    };
    if max_players < minimum_players {
        return Err(format!(
            "현재 설정의 최소 시작 인원은 {minimum_players}명이라 최대 인원 {max_players}명으로 시작할 수 없습니다."
        ));
    }
    Ok(())
}

fn enabled_special_count(config: &BotConfig, roles: &[Role]) -> usize {
    roles
        .iter()
        .filter(|role| special_role_enabled(config, **role))
        .count()
}

fn special_role_enabled(config: &BotConfig, role: Role) -> bool {
    match role {
        Role::Detective => config.enable_detective,
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
        Role::Shaman => config.enable_shaman,
        Role::Priest => config.enable_priest,
        Role::Soldier => config.enable_soldier,
        Role::Nurse => config.enable_nurse,
        Role::Gangster => config.enable_gangster,
        Role::Prophet => config.enable_prophet,
        Role::Psychologist => config.enable_psychologist,
        Role::Thief => config.enable_thief,
        _ => true,
    }
}

fn special_role_player_count(role: Role) -> usize {
    if role == Role::Lover { 2 } else { 1 }
}

fn selected_special_player_count(config: &BotConfig, roles: &[Role], count: u32) -> usize {
    let mut candidates = roles
        .iter()
        .filter(|role| special_role_enabled(config, **role))
        .map(|role| special_role_player_count(*role))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(|left, right| right.cmp(left));
    candidates.into_iter().take(count as usize).sum()
}

fn minimum_player_count(config: &BotConfig) -> usize {
    let cult_count = if config.enable_cult_team { 2 } else { 0 };
    let selected_count = config
        .default_mafia_count
        .saturating_sub(config.mafia_special_count) as usize
        + config.default_doctor_count as usize
        + config.default_police_count as usize
        + selected_special_player_count(
            config,
            CITIZEN_SPECIAL_ROLES,
            config.citizen_special_count,
        )
        + selected_special_player_count(config, MAFIA_SPECIAL_ROLES, config.mafia_special_count)
        + selected_special_player_count(
            config,
            NEUTRAL_SPECIAL_ROLES,
            config.neutral_special_count,
        )
        + cult_count;
    3.max(selected_count)
        .max(config.default_mafia_count as usize * 2 + 1)
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

async fn read_http_request<S>(stream: &mut S) -> Result<HttpRequest>
where
    S: AsyncRead + Unpin,
{
    let mut buffer = Vec::with_capacity(8192);
    let mut temp = [0u8; 4096];
    let mut header_end = None;
    let mut content_length = 0usize;
    loop {
        let read = stream.read(&mut temp).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&temp[..read]);
        if header_end.is_none()
            && let Some(index) = find_header_end(&buffer)
        {
            header_end = Some(index);
            let headers = String::from_utf8_lossy(&buffer[..index]);
            content_length = parse_content_length(&headers).unwrap_or(0);
        }
        if let Some(index) = header_end
            && buffer.len() >= index + 4 + content_length
        {
            break;
        }
        if buffer.len() > 128 * 1024 {
            bail!("요청이 너무 큽니다.");
        }
    }
    let Some(index) = header_end else {
        bail!("HTTP 헤더를 찾지 못했습니다.");
    };
    let raw_headers = String::from_utf8_lossy(&buffer[..index]).to_string();
    let mut first_line = raw_headers
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = first_line.next().unwrap_or_default().to_string();
    let path = first_line.next().unwrap_or_default().to_string();
    let body_start = index + 4;
    let body_end = (body_start + content_length).min(buffer.len());
    let body = String::from_utf8_lossy(&buffer[body_start..body_end]).to_string();
    Ok(HttpRequest {
        method,
        path,
        headers: parse_http_headers(&raw_headers),
        body,
    })
}

fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn json_response(value: Value) -> String {
    json_response_with_status("200 OK", value)
}

fn json_error(status: &str, message: &str) -> String {
    json_response_with_status(status, json!({"error": message}))
}

fn json_response_with_status(status: &str, value: Value) -> String {
    let body = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, X-API-Key\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn api_options_response() -> String {
    "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, X-API-Key\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Max-Age: 600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_content_length(headers: &str) -> Option<usize> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse().ok()
        } else {
            None
        }
    })
}

fn parse_http_headers(headers: &str) -> HashMap<String, String> {
    headers
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}

fn parse_urlencoded(body: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for pair in body.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values.insert(percent_decode(key), percent_decode(value));
    }
    values
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                    output.push(hex);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BotConfig {
        BotConfig {
            game_enabled: true,
            participant_role: "participant".to_string(),
            manager_role: "manager".to_string(),
            default_mafia_count: 2,
            default_doctor_count: 1,
            default_police_count: 1,
            default_joker_count: 0,
            max_player_count: 0,
            night_seconds: 60,
            discussion_seconds: 60,
            vote_seconds: 30,
            chat_slowmode_seconds: 3,
            reveal_death_roles: true,
            reveal_public_police_status: true,
            reveal_morning_mafia_count: true,
            citizen_special_count: 0,
            mafia_special_count: 0,
            neutral_special_count: 0,
            enable_detective: true,
            enable_graverobber: true,
            enable_spy: true,
            enable_contractor: true,
            enable_witch: true,
            enable_scientist: true,
            enable_madam: true,
            enable_godfather: true,
            enable_joker: true,
            enable_politician: true,
            enable_judge: true,
            enable_reporter: true,
            enable_hacker: true,
            enable_terrorist: true,
            enable_lover: true,
            enable_shaman: true,
            enable_priest: true,
            enable_soldier: true,
            enable_nurse: true,
            enable_gangster: true,
            enable_prophet: true,
            enable_psychologist: true,
            enable_thief: true,
            enable_cult_team: false,
            use_agent: false,
            use_vigilante: false,
            anonymous_mode: false,
            anonymous_name_mode: "animal".to_string(),
            blacklist_user_ids: Vec::new(),
        }
    }

    fn test_state() -> WebSettingsState {
        WebSettingsState {
            config: Arc::new(RwLock::new(test_config())),
            config_path: Arc::new(PathBuf::from("unused-config.json")),
            api_keys: Arc::new(RwLock::new(ApiKeyStore::default())),
            api_keys_path: Arc::new(PathBuf::from("unused-api-keys.json")),
            stats: Arc::new(RwLock::new(StatsFile::default())),
            games: Arc::new(DashMap::new()),
            recruitments: Arc::new(DashMap::new()),
            sessions: Arc::new(DashMap::new()),
            started_at: Instant::now(),
            bot_name: "bot".to_string(),
            guild_count: 1,
            base_url: "https://mafia.example".to_string(),
        }
    }

    fn api_request(method: &str, path: &str, key: Option<(&str, &str)>) -> HttpRequest {
        let mut headers = HashMap::new();
        if let Some((name, value)) = key {
            headers.insert(name.to_ascii_lowercase(), value.to_string());
        }
        HttpRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers,
            body: String::new(),
        }
    }

    fn updates_for(config: &BotConfig) -> HashMap<String, String> {
        WEB_CONFIG_FIELDS
            .iter()
            .map(|field| (field.name.to_string(), config_value(config, field.name)))
            .collect()
    }

    fn form_body_for(config: &BotConfig) -> String {
        WEB_CONFIG_FIELDS
            .iter()
            .filter_map(|field| {
                let value = config_value(config, field.name);
                if matches!(field.kind, WebFieldKind::Bool) && value != "true" {
                    None
                } else {
                    Some(format!("{}={}", field.name, value.replace('\n', "%0A")))
                }
            })
            .collect::<Vec<_>>()
            .join("&")
    }

    #[test]
    fn rejects_all_special_mafia_and_rolls_back() {
        let mut config = test_config();
        let mut updates = updates_for(&config);
        updates.insert("default_mafia_count".to_string(), "1".to_string());
        updates.insert("mafia_special_count".to_string(), "1".to_string());

        assert!(apply_updates(&mut config, &updates).is_err());
        assert_eq!(config.default_mafia_count, 2);
        assert_eq!(config.mafia_special_count, 0);
    }

    #[test]
    fn counts_two_player_special_roles_for_web_minimum() {
        let mut config = test_config();
        config.default_mafia_count = 1;
        config.citizen_special_count = 1;
        config.max_player_count = 4;

        assert!(validate_config(&config).is_err());
    }

    #[tokio::test]
    async fn invalid_post_returns_error_without_lock_deadlock() {
        let config = test_config();
        let state = test_state();
        let token = "test-token".to_string();
        state.sessions.insert(
            token.clone(),
            WebSettingsSession {
                guild_id: 1,
                user_id: 2,
                user_label: "tester".to_string(),
                expires_at: Instant::now() + Duration::from_secs(60),
            },
        );
        let body = form_body_for(&config)
            .replace("default_mafia_count=2", "default_mafia_count=1")
            .replace("mafia_special_count=0", "mafia_special_count=1");

        let response = tokio::time::timeout(
            Duration::from_secs(1),
            route_request(
                &state,
                HttpRequest {
                    method: "POST".to_string(),
                    path: format!("{WEB_SETTINGS_PATH}/{token}"),
                    headers: HashMap::new(),
                    body,
                },
            ),
        )
        .await
        .expect("invalid settings POST should not deadlock");

        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    }

    #[tokio::test]
    async fn public_status_api_returns_json() {
        let state = test_state();
        let response = route_request(&state, api_request("GET", "/api/status", None)).await;

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("Content-Type: application/json"));
        assert!(response.contains(r#""base_url":"https://mafia.example/api""#));
    }

    #[tokio::test]
    async fn protected_api_requires_key() {
        let state = test_state();
        let response = route_request(&state, api_request("GET", "/api/v1/me", None)).await;

        assert!(response.starts_with("HTTP/1.1 401 Unauthorized"));
        assert!(response.contains("missing API key"));
    }

    #[tokio::test]
    async fn protected_api_accepts_bearer_key() {
        let state = test_state();
        let raw_key = {
            let mut store = state.api_keys.write().await;
            issue_api_key(&mut store, 1, 2, "integration".to_string())
        };
        let response = route_request(
            &state,
            api_request("GET", "/api/v1/me", Some(("Authorization", &format!("Bearer {raw_key}")))),
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("integration"));
    }

    #[tokio::test]
    async fn protected_api_blocks_other_guild() {
        let state = test_state();
        let raw_key = {
            let mut store = state.api_keys.write().await;
            issue_api_key(&mut store, 1, 2, "guild-one".to_string())
        };
        let response = route_request(
            &state,
            api_request("GET", "/api/v1/games/2", Some(("X-API-Key", &raw_key))),
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 403 Forbidden"));
    }

    #[tokio::test]
    async fn api_key_management_issues_and_revokes_key() {
        let mut state = test_state();
        let key_path = std::env::temp_dir().join(format!("mafia-api-keys-{}.json", Uuid::new_v4()));
        state.api_keys_path = Arc::new(key_path.clone());
        let token = "api-key-test";
        state.sessions.insert(
            token.to_string(),
            WebSettingsSession {
                guild_id: 1,
                user_id: 2,
                user_label: "tester".to_string(),
                expires_at: Instant::now() + Duration::from_secs(60),
            },
        );
        let create_response = route_request(
            &state,
            HttpRequest {
                method: "POST".to_string(),
                path: format!("{WEB_SETTINGS_PATH}/{token}/api-keys"),
                headers: HashMap::new(),
                body: "action=create&label=integration".to_string(),
            },
        )
        .await;
        assert!(create_response.starts_with("HTTP/1.1 200 OK"));
        assert!(create_response.contains("mfr_"));
        let key_id = state.api_keys.read().await.keys[0].id.clone();

        let revoke_response = route_request(
            &state,
            HttpRequest {
                method: "POST".to_string(),
                path: format!("{WEB_SETTINGS_PATH}/{token}/api-keys"),
                headers: HashMap::new(),
                body: format!("action=revoke&key_id={key_id}"),
            },
        )
        .await;
        assert!(revoke_response.starts_with("HTTP/1.1 200 OK"));
        assert!(state.api_keys.read().await.keys[0].revoked);
        let _ = std::fs::remove_file(key_path);
    }

    #[tokio::test]
    async fn protected_api_starts_ready_recruitment() {
        let state = test_state();
        let raw_key = {
            let mut store = state.api_keys.write().await;
            issue_api_key(&mut store, 1, 2, "host".to_string())
        };
        let recruitment = Arc::new(RwLock::new(Recruitment {
            host_user_id: serenity::UserId::new(2),
            participant_role_id: serenity::RoleId::new(3),
            role_counts: HashMap::new(),
            special_roles: Vec::new(),
            max_players: 8,
            minimum_players: 2,
            joined_ids: std::collections::HashSet::from([2, 3]),
            joined_names: HashMap::new(),
            spectator_ids: std::collections::HashSet::new(),
            spectator_names: HashMap::new(),
            accepting: true,
            cancelled: false,
            done: Arc::new(tokio::sync::Notify::new()),
        }));
        state
            .recruitments
            .insert(serenity::GuildId::new(1), recruitment.clone());
        let response = route_request(
            &state,
            HttpRequest {
                method: "POST".to_string(),
                path: "/api/v1/recruitments/1/actions".to_string(),
                headers: HashMap::from([("x-api-key".to_string(), raw_key)]),
                body: r#"{"action":"start"}"#.to_string(),
            },
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(!recruitment.read().await.accepting);
    }

    #[test]
    fn api_key_store_never_serializes_raw_key() {
        let mut store = ApiKeyStore::default();
        let raw_key = issue_api_key(&mut store, 1, 2, "test".to_string());
        let serialized = serde_json::to_string(&store).unwrap();

        assert!(!serialized.contains(&raw_key));
        assert!(serialized.contains("key_hash"));
    }

    #[test]
    fn parses_api_key_headers_case_insensitively() {
        let headers = parse_http_headers("GET / HTTP/1.1\r\nX-API-Key: key-value\r\n");

        assert_eq!(headers.get("x-api-key").map(String::as_str), Some("key-value"));
    }

    #[test]
    fn api_docs_separate_public_and_protected_endpoints() {
        let html = render_api_docs_page("https://mafia.example/");

        assert!(html.contains("공개 조회 API"));
        assert!(html.contains("보호 관리 API"));
        assert!(html.contains("/api/v1/games/{guild_id}/actions"));
        assert!(html.contains("https://mafia.example/api/v1/games/123"));
        assert!(!html.contains("example.com"));
        assert!(html.contains("overflow-wrap: anywhere"));
        assert!(html.contains("word-break: break-word"));
        assert!(html.contains("site-shell"));
        assert!(html.contains("응답 코드"));
    }
}
