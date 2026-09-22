// casino_web.rs — 카지노 웹: 개인 링크 페이지(SPA), 상태/명령 API, 웹소켓 푸시.
// Discord Activity와 같은 axum 서버에 얹힌다 (/casino/...).

use crate::casino_hub::{
    CasinoMe, CasinoSession, SharedHub, TableSummary, now_ms, session_expired_message,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{
        Path as AxumPath, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use mafia_remake::casino::{CasinoCommand, GameKind, TableView};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

include!(concat!(env!("OUT_DIR"), "/casino_static.rs"));

#[derive(Clone)]
pub struct CasinoWebState {
    pub hub: SharedHub,
    pub static_dir: Option<String>,
}

pub fn casino_router(state: CasinoWebState) -> Router {
    Router::new()
        .route("/casino/api/state", get(state_handler))
        .route("/casino/api/command", post(command_handler))
        .route("/casino/api/ws", get(ws_handler))
        .route("/casino", get(casino_index))
        .route("/casino/", get(casino_index))
        .route("/casino/{*path}", get(casino_asset))
        .with_state(state)
}

// ------------------------------------------------------------ 딜러 아바타

/// 딜러(소피아) 얼굴을 잘라 256x256 PNG로 만든 웹훅 아바타. 한 번만 만든다.
pub fn dealer_avatar_png() -> Option<&'static [u8]> {
    static AVATAR: std::sync::OnceLock<Option<Vec<u8>>> = std::sync::OnceLock::new();
    AVATAR
        .get_or_init(|| {
            let asset = CASINO_ASSETS
                .iter()
                .find(|asset| asset.path == "/dealer.png")?;
            let image = image::load_from_memory(asset.body).ok()?;
            // 원본 dealer.png(1536x1024)에서 얼굴 부분 (웹 패널의 dealer-avatar와 같은 구도).
            let face = image.crop_imm(540, 10, 450, 450).resize_exact(
                256,
                256,
                image::imageops::FilterType::Lanczos3,
            );
            let mut bytes = std::io::Cursor::new(Vec::new());
            face.write_to(&mut bytes, image::ImageFormat::Png).ok()?;
            Some(bytes.into_inner())
        })
        .as_deref()
}

// ------------------------------------------------------------ 개발 모드

/// `mafia --casino-dev`: Discord 연결 없이 카지노 웹만 띄운다 (UI 개발·점검용).
/// 테스트 계정 3개(각 50,000코인)와 홀덤·블랙잭 테이블 하나씩을 만들고 개인 링크를 출력한다.
pub async fn run_dev_server(workspace_root: &Path) -> anyhow::Result<()> {
    use crate::casino_hub::{CasinoHub, TableBinding, personal_link};

    let port: u16 = std::env::var("CASINO_DEV_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8811);
    let state_path = workspace_root.join("casino-dev.json");
    let stats_path = workspace_root.join("casino-dev-stats.json");
    let _ = std::fs::remove_file(&state_path);
    let users = [
        (1001u64, "테스터A"),
        (1002u64, "테스터B"),
        (1003u64, "테스터C"),
    ];
    let mut stats = mafia_remake::stats::StatsFile::default();
    for (id, name) in users {
        crate::stats::refund_coins(&mut stats, id, name, 50_000);
    }
    let hub: SharedHub = Arc::new(CasinoHub::load(
        state_path,
        Arc::new(tokio::sync::RwLock::new(stats)),
        Arc::new(stats_path),
    ));
    let binding = TableBinding {
        guild_id: 0,
        channel_id: 0,
        ..Default::default()
    };
    let holdem = hub
        .create_table(GameKind::Holdem, "개발 홀덤", 0, binding.clone())
        .map_err(anyhow::Error::msg)?;
    let blackjack = hub
        .create_table(GameKind::Blackjack, "개발 블랙잭", 0, binding)
        .map_err(anyhow::Error::msg)?;
    let base = format!("http://localhost:{port}");
    println!("카지노 개발 서버 (Discord 연결 없음): {base}/casino/");
    for (id, name) in users {
        let token = hub.issue_session(id, name.to_string());
        println!("{name}: {}", personal_link(&base, &token, &holdem));
        println!(
            "{name} (블랙잭): {}",
            personal_link(&base, &token, &blackjack)
        );
    }
    let ticker = hub.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            ticker.tick_all().await;
        }
    });
    let router = casino_router(CasinoWebState {
        hub,
        static_dir: std::env::var("CASINO_STATIC_DIR").ok(),
    });
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

// ------------------------------------------------------------ 정적 페이지

async fn casino_index(State(state): State<CasinoWebState>) -> Response {
    serve_asset(&state, "/index.html")
}

async fn casino_asset(
    State(state): State<CasinoWebState>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    let asset_path = format!("/{path}");
    if path.starts_with("assets/") || path.contains('.') {
        if let Some(response) = try_serve_asset(&state, &asset_path) {
            return response;
        }
        if path.starts_with("assets/") {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    // /casino/<토큰> 같은 SPA 경로는 index.html을 준다.
    serve_asset(&state, "/index.html")
}

fn serve_asset(state: &CasinoWebState, asset_path: &str) -> Response {
    try_serve_asset(state, asset_path).unwrap_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "casino-web/dist가 없습니다. `cd casino-web && npm ci && npm run build` 후 다시 빌드하세요.",
        )
            .into_response()
    })
}

fn try_serve_asset(state: &CasinoWebState, asset_path: &str) -> Option<Response> {
    if let Some(dir) = state.static_dir.as_deref() {
        let file = Path::new(dir).join(asset_path.trim_start_matches('/'));
        if file.is_file() {
            if let Ok(body) = std::fs::read(&file) {
                return Some(
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, content_type_for(asset_path))
                        .header(header::CACHE_CONTROL, cache_control(asset_path))
                        .body(Body::from(body))
                        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
                );
            }
        }
    }
    CASINO_ASSETS
        .iter()
        .find(|asset| asset.path == asset_path)
        .map(|asset| {
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, asset.content_type)
                .header(header::CACHE_CONTROL, cache_control(asset.path))
                .body(Body::from(asset.body.to_vec()))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        })
}

fn content_type_for(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn cache_control(path: &str) -> &'static str {
    if path == "/index.html" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    }
}

// ------------------------------------------------------------ 인증

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn error_response(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": { "code": code, "message": message.into() } })),
    )
        .into_response()
}

fn session_or_error(
    state: &CasinoWebState,
    token: Option<String>,
) -> Result<CasinoSession, Response> {
    token
        .and_then(|token| state.hub.session(&token))
        .ok_or_else(|| {
            error_response(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                session_expired_message(),
            )
        })
}

// ------------------------------------------------------------ 상태

#[derive(Debug, Deserialize)]
pub struct StateQuery {
    pub table: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StateResponse {
    pub server_time: i64,
    pub me: CasinoMe,
    pub table: Option<TableView>,
    pub tables: Vec<TableSummary>,
}

async fn build_state(
    state: &CasinoWebState,
    session: &CasinoSession,
    requested: Option<&str>,
) -> StateResponse {
    let hub = &state.hub;
    let me = hub.me(session).await;
    let tables = hub.summaries().await;
    let table_id = requested
        .map(str::to_string)
        .filter(|id| hub.tables.contains_key(id))
        .or_else(|| me.seated_table.clone())
        .or_else(|| tables.first().map(|summary| summary.id.clone()));
    let table = match table_id {
        Some(id) => hub.view_for(&id, Some(session.user_id)).await,
        None => None,
    };
    StateResponse {
        server_time: now_ms(),
        me,
        table,
        tables,
    }
}

async fn state_handler(
    State(state): State<CasinoWebState>,
    headers: HeaderMap,
    Query(query): Query<StateQuery>,
) -> Response {
    let session = match session_or_error(&state, bearer_token(&headers).or(query.token)) {
        Ok(session) => session,
        Err(response) => return response,
    };
    Json(build_state(&state, &session, query.table.as_deref()).await).into_response()
}

// ------------------------------------------------------------ 명령

#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    pub table_id: String,
    #[serde(default)]
    pub version: Option<u64>,
    pub command: CasinoCommand,
}

async fn command_handler(
    State(state): State<CasinoWebState>,
    headers: HeaderMap,
    Json(request): Json<CommandRequest>,
) -> Response {
    let session = match session_or_error(&state, bearer_token(&headers)) {
        Ok(session) => session,
        Err(response) => return response,
    };
    let result = state
        .hub
        .apply(
            &request.table_id,
            session.user_id,
            &session.name,
            &request.command,
            request.version,
        )
        .await;
    match result {
        Ok(_) => Json(build_state(&state, &session, Some(&request.table_id)).await).into_response(),
        Err(error) => {
            let status = match error.code {
                "STALE_STATE" => StatusCode::CONFLICT,
                "NOT_FOUND" => StatusCode::NOT_FOUND,
                "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
                _ => StatusCode::BAD_REQUEST,
            };
            error_response(status, error.code, error.message)
        }
    }
}

// ------------------------------------------------------------ 웹소켓

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub token: String,
    pub table: Option<String>,
}

async fn ws_handler(
    State(state): State<CasinoWebState>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let session = match state.hub.session(&query.token) {
        Some(session) => session,
        None => return (StatusCode::UNAUTHORIZED, session_expired_message()).into_response(),
    };
    let token = query.token.clone();
    ws.on_upgrade(move |socket| handle_ws(socket, state, session, token, query.table))
}

async fn handle_ws(
    mut socket: WebSocket,
    state: CasinoWebState,
    session: CasinoSession,
    session_token: String,
    table: Option<String>,
) {
    let mut updates = state.hub.updates.subscribe();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut current_table = table;
    loop {
        let send_now = tokio::select! {
            _ = interval.tick() => true,
            update = updates.recv() => match update {
                Ok(update) => current_table.as_deref().is_none_or(|id| id == update.table_id),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => true,
                Err(_) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    // {"table": "<id>"} 로 보는 테이블을 바꿀 수 있다.
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(id) = value.get("table").and_then(|v| v.as_str()) {
                            current_table = Some(id.to_string());
                        }
                    }
                    true
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => false,
            },
        };
        if !send_now {
            continue;
        }
        if state.hub.sessions.get(&session_token).is_none() {
            // 세션이 사라졌으면(만료·재시작) 연결을 끊어 클라이언트가 새 링크를 받게 한다.
            break;
        }
        let payload = build_state(&state, &session, current_table.as_deref()).await;
        let Ok(json) = serde_json::to_string(&payload) else {
            continue;
        };
        if socket.send(Message::Text(json.into())).await.is_err() {
            break;
        }
    }
}
