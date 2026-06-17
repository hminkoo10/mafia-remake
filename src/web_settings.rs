use crate::{Recruitment, RunningGame};
use anyhow::{Result, bail};
use dashmap::DashMap;
use mafia_remake::config::{self, BotConfig};
use mafia_remake::model::{
    CITIZEN_SPECIAL_ROLES, MAFIA_SPECIAL_ROLES, NEUTRAL_SPECIAL_ROLES, Role,
};
use mafia_remake::stats::{self, StatsFile};
use poise::serenity_prelude as serenity;
use rand::RngCore;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

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

#[derive(Clone)]
pub struct WebSettingsState {
    pub config: Arc<RwLock<BotConfig>>,
    pub config_path: Arc<PathBuf>,
    pub stats: Arc<RwLock<StatsFile>>,
    pub games: Arc<DashMap<serenity::GuildId, Arc<RwLock<RunningGame>>>>,
    pub recruitments: Arc<DashMap<serenity::GuildId, Arc<RwLock<Recruitment>>>>,
    pub sessions: Arc<DashMap<String, WebSettingsSession>>,
    pub started_at: Instant,
    pub bot_name: String,
    pub guild_count: usize,
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
  :root { color-scheme: light dark; }
  body { font-family: -apple-system, "Segoe UI", "Apple SD Gothic Neo", sans-serif;
         max-width: 720px; margin: 32px auto; padding: 0 16px; line-height: 1.5; }
  h1 { font-size: 1.4rem; }
  h2 { font-size: 1.1rem; margin: 0 0 12px; }
  a { color: #5865F2; }
  .meta { color: #888; font-size: 0.9rem; margin-bottom: 24px; }
  .nav { display: flex; flex-wrap: wrap; gap: 10px; margin: 14px 0 20px; }
  .nav a { text-decoration: none; padding: 8px 12px; border: 1px solid #8884; border-radius: 8px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; margin: 16px 0; }
  .card { border: 1px solid #8884; border-radius: 8px; padding: 14px; background: #8881; }
  .card strong { display: block; font-size: 1.4rem; margin-top: 4px; }
  .panel { border: 1px solid #8884; border-radius: 8px; padding: 16px; margin: 16px 0; }
  .pill { display: inline-block; padding: 3px 9px; border: 1px solid #8884; border-radius: 999px; }
  code { background: #8882; padding: 2px 6px; border-radius: 6px; }
  table { width: 100%; border-collapse: collapse; }
  th, td { padding: 8px 6px; border-bottom: 1px solid #8883; text-align: left; }
  fieldset { border: 1px solid #8884; border-radius: 10px; padding: 8px 16px; margin-bottom: 16px; }
  legend { padding: 0 6px; font-weight: 600; }
  .row { display: flex; align-items: center; justify-content: space-between;
         gap: 16px; padding: 8px 0; border-bottom: 1px solid #8882; }
  .row:last-child { border-bottom: none; }
  .row span { flex: 1; }
  input[type="text"], input[type="number"], textarea { padding: 6px 10px; border-radius: 6px;
         border: 1px solid #8886; min-width: 160px; font-size: 0.95rem; }
  textarea { min-height: 72px; width: min(360px, 100%); resize: vertical; }
  input[type="checkbox"] { width: 20px; height: 20px; }
  button { margin-top: 16px; padding: 10px 22px; border: none; border-radius: 8px;
           background: #5865F2; color: white; font-size: 1rem; cursor: pointer; }
  button:hover { background: #4752c4; }
  .message { padding: 12px 16px; border-radius: 8px; margin-bottom: 16px; }
  .message.error { background: #f8d7da; color: #842029; }
</style>
"#;

pub fn settings_path() -> &'static str {
    WEB_SETTINGS_PATH
}

pub fn session_ttl_minutes() -> u64 {
    (WEB_SETTINGS_SESSION_TTL_SECONDS / 60).max(1)
}

pub fn base_url(host: &str, port: u16) -> String {
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
    format!("http://{display_host}:{port}")
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

pub async fn run_server(state: WebSettingsState, host: String, port: u16) -> Result<()> {
    let listener = TcpListener::bind((host.as_str(), port)).await?;
    println!("Rust web settings server ready: {host}:{port}");
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

async fn handle_connection(mut stream: TcpStream, state: WebSettingsState) -> Result<()> {
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
    if request.method == "GET"
        && let Some(response) = route_public_request(state, path, query).await
    {
        return response;
    }
    let Some(token) = path.strip_prefix(&format!("{WEB_SETTINGS_PATH}/")) else {
        return http_response(
            "404 Not Found",
            &render_message_page("404", "요청한 페이지를 찾을 수 없습니다."),
        );
    };
    purge_expired_sessions(&state.sessions);
    let Some(session) = state.sessions.get(token).map(|entry| entry.clone()) else {
        return http_response("410 Gone", &expired_page());
    };
    let _session_scope = (session.guild_id, session.user_id);

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
        "/api" | "/api/docs" => Some(http_response("200 OK", &render_api_docs_page())),
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
<h1>🕵️ 마피아 게임 웹 설정</h1>
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
</body>
</html>"#,
        html_escape(&session.user_label),
        status_html,
        html_escape(action)
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
        r#"<!DOCTYPE html><html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex">{refresh}<title>{}</title>{WEB_PAGE_STYLE}</head><body><h1>{}</h1>{}{body}</body></html>"#,
        html_escape(title),
        html_escape(title),
        render_nav(),
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
        render_leaderboard_table(leaderboard, true)
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

fn render_leaderboard_page(leaderboard: &Value, stats_summary: &Value) -> String {
    let body = format!(
        r#"<p class="meta">현재 기준: <span class="pill">{}</span></p>{}{}"#,
        safe_text(leaderboard.get("metric_name")),
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
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}승 {}패</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                safe_text(entry.get("rank")),
                safe_text(entry.get("name")),
                safe_text(entry.get("rating")),
                safe_text(entry.get("wins")),
                safe_text(entry.get("losses")),
                safe_text(entry.get("winrate_text")),
                safe_text(entry.get("games")),
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
        r#"<section class="panel">{title}<table><thead><tr><th>순위</th><th>이름</th><th>레이팅</th><th>승패</th><th>승률</th><th>판수</th><th>게임시간</th></tr></thead><tbody>{rows}</tbody></table></section>"#
    )
}

fn render_api_docs_page() -> String {
    let body = r#"<p class="meta">웹 상태판에서 사용하는 공개 API입니다. 모든 응답은 JSON입니다.</p>
<section class="panel"><h2>엔드포인트</h2>
<p><code>GET /health</code> 봇 웹 서버 생존 확인</p>
<p><code>GET /api/status</code> 봇 상태, 진행 중 게임, 공개 설정 요약</p>
<p><code>GET /api/games</code> 진행 중 게임 목록</p>
<p><code>GET /api/settings</code> 공개 설정 요약</p>
<p><code>GET /api/stats</code> 전적 요약</p>
<p><code>GET /api/leaderboard/{metric}</code> rating, wins, winrate, games, mafia, playtime 리더보드</p>
</section>"#;
    base_html("마피아 봇 API 문서", body, false)
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
<h1>{}</h1>
<p>{}</p>
</body>
</html>"#,
        html_escape(title),
        html_escape(title),
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
    body: String,
}

async fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest> {
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
    let headers = String::from_utf8_lossy(&buffer[..index]).to_string();
    let mut first_line = headers
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = first_line.next().unwrap_or_default().to_string();
    let path = first_line.next().unwrap_or_default().to_string();
    let body_start = index + 4;
    let body_end = (body_start + content_length).min(buffer.len());
    let body = String::from_utf8_lossy(&buffer[body_start..body_end]).to_string();
    Ok(HttpRequest { method, path, body })
}

fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn json_response(value: Value) -> String {
    let body = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
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
        let sessions = Arc::new(DashMap::new());
        let token = "test-token".to_string();
        sessions.insert(
            token.clone(),
            WebSettingsSession {
                guild_id: 1,
                user_id: 2,
                user_label: "tester".to_string(),
                expires_at: Instant::now() + Duration::from_secs(60),
            },
        );
        let state = WebSettingsState {
            config: Arc::new(RwLock::new(config.clone())),
            config_path: Arc::new(PathBuf::from("unused-config.json")),
            stats: Arc::new(RwLock::new(StatsFile::default())),
            games: Arc::new(DashMap::new()),
            recruitments: Arc::new(DashMap::new()),
            sessions,
            started_at: Instant::now(),
            bot_name: "bot".to_string(),
            guild_count: 1,
        };
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
                    body,
                },
            ),
        )
        .await
        .expect("invalid settings POST should not deadlock");

        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    }
}
