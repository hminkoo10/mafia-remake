// casino_web.rs — 카지노 웹: 개인 링크 페이지(SPA), 상태/명령 API, 웹소켓 푸시.
// Discord Activity와 같은 axum 서버에 얹힌다 (/casino/...).

use crate::casino_hub::{
    CasinoMe, CasinoSession, SharedHub, TableSummary, casino_command_action, now_ms,
    session_expired_message,
};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{
        Path as AxumPath, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use mafia_remake::casino::{CasinoCommand, GameKind, SettingsRequest, TableSettings, TableView};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast::error::{RecvError, TryRecvError};
use tower_http::compression::CompressionLayer;

include!(concat!(env!("OUT_DIR"), "/casino_static.rs"));

#[derive(Clone)]
pub struct CasinoWebState {
    pub hub: SharedHub,
    pub static_dir: Option<String>,
}

pub fn casino_router(state: CasinoWebState) -> Router {
    // 상태 JSON(수십 KB, 반복이 많다)은 gzip으로 줄여 휴대폰에서의 왕복을 빠르게 한다.
    // 웹소켓 업그레이드와 범위 요청으로 받는 영상은 압축하지 않는다.
    let api = Router::new()
        .route("/casino/api/state", get(state_handler))
        .route("/casino/api/command", post(command_handler))
        .layer(CompressionLayer::new());
    Router::new()
        .merge(api)
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

#[cfg(test)]
mod tests {
    use super::{
        ByteRange, byte_range, cache_control, content_type_for, dealer_avatar_png, drain_updates,
        heartbeat, is_safe_asset_path, read_static_file, table_switch,
    };
    use crate::casino_hub::{CasinoUpdate, UpdateKind};

    #[test]
    fn buffered_updates_are_drained_into_one_rebuild() {
        let (sender, mut receiver) = tokio::sync::broadcast::channel(8);
        let update = |id: &str| CasinoUpdate {
            table_id: id.to_string(),
            kind: UpdateKind::RevealTick,
        };
        for id in ["b", "a", "c"] {
            sender.send(update(id)).unwrap();
        }
        // 쌓인 알림을 한 번에 비우고, 보는 테이블 알림이 섞여 있었는지만 본다.
        assert_eq!(drain_updates(&mut receiver, Some("a")), Some(true));
        assert_eq!(drain_updates(&mut receiver, Some("a")), Some(false));
        sender.send(update("b")).unwrap();
        assert_eq!(drain_updates(&mut receiver, Some("a")), Some(false));
        // 보는 테이블이 없으면 모든 알림이 관계있다.
        sender.send(update("b")).unwrap();
        assert_eq!(drain_updates(&mut receiver, None), Some(true));
        drop(sender);
        assert_eq!(drain_updates(&mut receiver, Some("a")), None);
    }

    #[test]
    fn lagged_receivers_rebuild_once() {
        let (sender, mut receiver) = tokio::sync::broadcast::channel(2);
        for id in ["x", "y", "z", "w"] {
            sender
                .send(CasinoUpdate {
                    table_id: id.to_string(),
                    kind: UpdateKind::Changed,
                })
                .unwrap();
        }
        // 밀려서 놓친 알림이 있으면 무엇이 바뀌었는지 모르므로 다시 만든다.
        assert_eq!(drain_updates(&mut receiver, Some("a")), Some(true));
    }

    #[tokio::test]
    async fn state_api_is_gzipped_on_request_and_carries_the_timing_rules() {
        use crate::casino_hub::{CasinoHub, TableBinding};
        use mafia_remake::casino::{GameKind, TableSettings};
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!(
            "mafia-casino-web-gzip-{}-{}",
            std::process::id(),
            mafia_remake::atomic_file::next_seq()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Arc::new(
            CasinoHub::load(
                dir.join("casino.json"),
                Arc::new(tokio::sync::RwLock::new(
                    mafia_remake::stats::StatsFile::default(),
                )),
                Arc::new(dir.join("stats.json")),
            )
            .unwrap(),
        );
        hub.create_table(
            GameKind::Blackjack,
            "압축",
            1,
            TableBinding::default(),
            TableSettings::default(),
        )
        .unwrap();
        let token = hub.issue_session(5, "손님".to_string());
        let router = super::casino_router(super::CasinoWebState {
            hub,
            static_dir: None,
        });
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        let url = format!("http://{address}/casino/api/state");
        let client = reqwest::Client::new();

        let gzipped = client
            .get(&url)
            .bearer_auth(&token)
            .header("accept-encoding", "gzip")
            .send()
            .await
            .unwrap();
        assert_eq!(gzipped.status(), 200);
        assert_eq!(
            gzipped
                .headers()
                .get("content-encoding")
                .and_then(|value| value.to_str().ok()),
            Some("gzip")
        );

        let plain = client.get(&url).bearer_auth(&token).send().await.unwrap();
        assert!(plain.headers().get("content-encoding").is_none());
        let body: serde_json::Value = plain.json().await.unwrap();
        assert_eq!(body["me"]["user_id"], "5");
        let rules = &body["table"]["rules"];
        assert!(rules["card_flight_ms"].as_i64().unwrap() > 0);
        assert!(rules["card_flip_ms"].as_i64().unwrap() > 0);

        // 해시로만 저장한 세션도 토큰 원문으로 찾고, 틀린 토큰은 거절한다.
        let rejected = client.get(&url).bearer_auth("wrong").send().await.unwrap();
        assert_eq!(rejected.status(), 401);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn heartbeats_carry_only_the_server_time() {
        let value: serde_json::Value = serde_json::from_str(&heartbeat(1_234)).unwrap();
        assert_eq!(value, serde_json::json!({ "server_time": 1_234 }));
    }

    #[test]
    fn asset_paths_cannot_leave_the_static_dir() {
        assert!(is_safe_asset_path("dealers/sophia-idle.webm"));
        assert!(is_safe_asset_path("assets/index.js"));
        assert!(is_safe_asset_path("index.html"));
        // GET /casino/..%2F..%2F.env 등: 퍼센트 디코딩 뒤의 값.
        assert!(!is_safe_asset_path("../stats.json"));
        assert!(!is_safe_asset_path("../../.env"));
        assert!(!is_safe_asset_path("a/../../x"));
        assert!(!is_safe_asset_path("assets/.."));
        assert!(!is_safe_asset_path("./index.html"));
        assert!(!is_safe_asset_path("..\\x"));
        assert!(!is_safe_asset_path("assets\\..\\..\\config.json"));
        assert!(!is_safe_asset_path("/etc/passwd"));
        assert!(!is_safe_asset_path("index.html\0.png"));
        assert!(!is_safe_asset_path(""));
        #[cfg(windows)]
        assert!(!is_safe_asset_path("C:/Windows/win.ini"));
    }

    #[test]
    fn static_files_outside_the_dir_are_not_read() {
        let base = std::env::temp_dir().join(format!(
            "mafia-casino-static-test-{}-{}",
            std::process::id(),
            super::now_ms()
        ));
        let static_dir = base.join("dist");
        std::fs::create_dir_all(static_dir.join("assets")).unwrap();
        std::fs::write(static_dir.join("assets/app.js"), b"ok").unwrap();
        std::fs::write(base.join("stats.json"), b"secret").unwrap();

        assert_eq!(
            read_static_file(&static_dir, "assets/app.js").as_deref(),
            Some(&b"ok"[..])
        );
        assert_eq!(read_static_file(&static_dir, "../stats.json"), None);
        assert_eq!(
            read_static_file(&static_dir, "assets/../../stats.json"),
            None
        );
        assert_eq!(read_static_file(&static_dir, "missing.js"), None);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn websocket_frames_only_switch_to_a_different_table() {
        assert_eq!(
            table_switch(Some("a"), r#"{"table":"b"}"#),
            Some("b".to_string())
        );
        assert_eq!(
            table_switch(None, r#"{"table":"a"}"#),
            Some("a".to_string())
        );
        assert_eq!(table_switch(Some("a"), r#"{"table":"a"}"#), None);
        assert_eq!(table_switch(Some("a"), "{}"), None);
        assert_eq!(table_switch(None, "{}"), None);
        assert_eq!(table_switch(Some("a"), r#"{"table":7}"#), None);
        assert_eq!(table_switch(Some("a"), "not json"), None);
    }

    #[test]
    fn media_byte_ranges_follow_rfc_7233() {
        assert_eq!(byte_range(None, 100), ByteRange::Full);
        assert_eq!(byte_range(Some("bytes=0-1"), 100), ByteRange::Part(0, 1));
        assert_eq!(byte_range(Some("bytes=10-"), 100), ByteRange::Part(10, 99));
        assert_eq!(byte_range(Some("bytes=-30"), 100), ByteRange::Part(70, 99));
        assert_eq!(
            byte_range(Some("bytes=90-500"), 100),
            ByteRange::Part(90, 99)
        );
        assert_eq!(
            byte_range(Some("bytes=100-"), 100),
            ByteRange::Unsatisfiable
        );
        assert_eq!(byte_range(Some("bytes=5-2"), 100), ByteRange::Full);
        assert_eq!(byte_range(Some("bytes=0-1,5-6"), 100), ByteRange::Full);
        assert_eq!(byte_range(Some("items=0-1"), 100), ByteRange::Full);
    }

    #[test]
    fn dealer_clips_have_video_mime_types() {
        assert_eq!(content_type_for("/dealers/sophia-deal.webm"), "video/webm");
        assert_eq!(content_type_for("/dealers/sophia-flip.mp4"), "video/mp4");
    }

    #[test]
    fn replaceable_dealer_assets_are_not_cached_for_a_year() {
        assert_eq!(
            cache_control("/dealers/sophia-idle.webm"),
            "public, max-age=300"
        );
        assert_eq!(
            cache_control("/dealers/sophia-table.png"),
            "public, max-age=300"
        );
        assert_eq!(
            cache_control("/assets/index-hash.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control("/index.html"), "no-cache");
    }

    #[test]
    fn dealer_avatar_is_a_png_cropped_from_the_embedded_image() {
        let png = dealer_avatar_png().expect("dealer.png is embedded and decodable");
        assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
        let image = image::load_from_memory(png).expect("generated avatar decodes");
        assert_eq!((image.width(), image.height()), (256, 256));
    }
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
    )?);
    hub.start_saver();
    let binding = TableBinding {
        guild_id: 0,
        channel_id: 0,
        ..Default::default()
    };
    let holdem = hub
        .create_table(
            GameKind::Holdem,
            "개발 홀덤",
            0,
            binding.clone(),
            TableSettings::default(),
        )
        .map_err(anyhow::Error::msg)?;
    let blackjack = hub
        .create_table(
            GameKind::Blackjack,
            "개발 블랙잭",
            0,
            binding.clone(),
            TableSettings::default(),
        )
        .map_err(anyhow::Error::msg)?;
    // 방 설정 확인용 고액 테이블 (베팅 500~25,000).
    let high_limit = TableSettings::build(
        GameKind::Blackjack,
        SettingsRequest {
            min_bet: Some(500),
            max_bet: Some(25_000),
            min_buy_in: Some(10_000),
            max_buy_in: Some(50_000),
            ..Default::default()
        },
    )
    .map_err(anyhow::Error::msg)?;
    hub.create_table(
        GameKind::Blackjack,
        "개발 고액 블랙잭",
        0,
        binding,
        high_limit,
    )
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
        let mut interval = tokio::time::interval(Duration::from_millis(250));
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
    headers: HeaderMap,
) -> Response {
    let asset_path = format!("/{path}");
    if path.starts_with("assets/") || path.contains('.') {
        let range = headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok());
        if let Some(response) = try_serve_asset(&state, &asset_path, range) {
            return response;
        }
        return StatusCode::NOT_FOUND.into_response();
    }
    // /casino/<토큰> 같은 SPA 경로는 index.html을 준다.
    serve_asset(&state, "/index.html")
}

fn serve_asset(state: &CasinoWebState, asset_path: &str) -> Response {
    try_serve_asset(state, asset_path, None).unwrap_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "casino-web/dist가 없습니다. `cd casino-web && npm ci && npm run build` 후 다시 빌드하세요.",
        )
            .into_response()
    })
}

fn try_serve_asset(
    state: &CasinoWebState,
    asset_path: &str,
    range: Option<&str>,
) -> Option<Response> {
    if let Some(dir) = state.static_dir.as_deref() {
        if let Some(body) = read_static_file(Path::new(dir), asset_path.trim_start_matches('/')) {
            return Some(asset_response(
                asset_path,
                content_type_for(asset_path),
                Bytes::from(body),
                range,
            ));
        }
    }
    CASINO_ASSETS
        .iter()
        .find(|asset| asset.path == asset_path)
        .map(|asset| {
            asset_response(
                asset.path,
                asset.content_type,
                Bytes::from_static(asset.body),
                range,
            )
        })
}

/// `{*path}`는 퍼센트 디코딩된 값이라 `..%2F`로 `..`가 들어올 수 있다.
/// 평범한 이름 조각만 허용한다 (`..`·`.`·루트·드라이브 접두사·백슬래시·NUL 거부).
fn is_safe_asset_path(relative: &str) -> bool {
    !relative.is_empty()
        && !relative.contains(['\\', '\0'])
        && Path::new(relative)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// CASINO_STATIC_DIR 안의 파일만 읽는다. 심볼릭 링크 등으로 밖을 가리키면 거부한다.
fn read_static_file(dir: &Path, relative: &str) -> Option<Vec<u8>> {
    if !is_safe_asset_path(relative) {
        return None;
    }
    let file = dir.join(relative);
    if !file.is_file() {
        return None;
    }
    let root = dir.canonicalize().ok()?;
    let resolved = file.canonicalize().ok()?;
    if !resolved.starts_with(&root) {
        return None;
    }
    std::fs::read(&resolved).ok()
}

/// 요청한 바이트 범위. Safari(iOS·Discord 앱 포함)는 영상을 범위 요청으로만 재생한다.
#[derive(Debug, PartialEq, Eq)]
enum ByteRange {
    /// 범위가 없거나 해석할 수 없음: 전체를 준다.
    Full,
    /// 시작~끝(포함).
    Part(usize, usize),
    /// 파일 밖을 가리킴: 416.
    Unsatisfiable,
}

fn byte_range(range: Option<&str>, len: usize) -> ByteRange {
    let Some(spec) = range.and_then(|value| value.trim().strip_prefix("bytes=")) else {
        return ByteRange::Full;
    };
    // 여러 범위는 지원하지 않는다 (전체를 준다).
    let Some((start, end)) = spec.split_once('-').filter(|_| !spec.contains(',')) else {
        return ByteRange::Full;
    };
    if len == 0 {
        return ByteRange::Unsatisfiable;
    }
    let (start, end) = match (start.trim(), end.trim()) {
        ("", suffix) => match suffix.parse::<usize>() {
            Ok(0) => return ByteRange::Unsatisfiable,
            Ok(count) => (len.saturating_sub(count), len - 1),
            Err(_) => return ByteRange::Full,
        },
        (start, end) => {
            let Ok(start) = start.parse::<usize>() else {
                return ByteRange::Full;
            };
            let end = if end.is_empty() {
                len - 1
            } else {
                match end.parse::<usize>() {
                    Ok(end) if end >= start => end.min(len - 1),
                    _ => return ByteRange::Full,
                }
            };
            (start, end)
        }
    };
    if start >= len {
        return ByteRange::Unsatisfiable;
    }
    ByteRange::Part(start, end)
}

fn asset_response(
    asset_path: &str,
    content_type: &str,
    body: Bytes,
    range: Option<&str>,
) -> Response {
    let len = body.len();
    let builder = Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, cache_control(asset_path))
        .header(header::ACCEPT_RANGES, "bytes");
    let response = match byte_range(range, len) {
        ByteRange::Full => builder.status(StatusCode::OK).body(Body::from(body)),
        ByteRange::Part(start, end) => builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
            .body(Body::from(body.slice(start..=end))),
        ByteRange::Unsatisfiable => builder
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{len}"))
            .body(Body::empty()),
    };
    response.unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
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
        Some("webm") => "video/webm",
        Some("mp4") => "video/mp4",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn cache_control(path: &str) -> &'static str {
    if path == "/index.html" {
        "no-cache"
    } else if path.starts_with("/dealers/") {
        "public, max-age=300"
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
    // 화면의 공개 판정과 server_time은 같은 시각 하나를 쓴다.
    let (table, server_time) = match table_id {
        Some(id) => match hub.view_now(&id, Some(session.user_id)).await {
            Some((view, now)) => (Some(view), now),
            None => (None, now_ms()),
        },
        None => (None, now_ms()),
    };
    StateResponse {
        server_time,
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
    let started = Instant::now();
    let build_started = Instant::now();
    let response = build_state(&state, &session, query.table.as_deref()).await;
    let build_ms = build_started.elapsed().as_millis();
    if started.elapsed() >= Duration::from_secs(1) {
        eprintln!(
            "casino slow request: endpoint=state total_ms={} table_id={} apply_ms=0 save_ms=0 build_state_ms={build_ms}",
            started.elapsed().as_millis(),
            query.table.as_deref().unwrap_or("none")
        );
    }
    Json(response).into_response()
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
    let started = Instant::now();
    let apply_started = Instant::now();
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
    let apply_ms = apply_started.elapsed().as_millis();
    match result {
        Ok(_) => {
            let build_started = Instant::now();
            let response = build_state(&state, &session, Some(&request.table_id)).await;
            let build_ms = build_started.elapsed().as_millis();
            if started.elapsed() >= Duration::from_secs(1) && apply_ms < 1_000 {
                eprintln!(
                    "casino slow request: endpoint=command total_ms={} table_id={} action={} apply_ms={apply_ms} save_ms=0 build_state_ms={build_ms}",
                    started.elapsed().as_millis(),
                    request.table_id,
                    casino_command_action(&request.command),
                );
            }
            Json(response).into_response()
        }
        Err(error) => {
            if started.elapsed() >= Duration::from_secs(1) && apply_ms < 1_000 {
                eprintln!(
                    "casino slow request: endpoint=command total_ms={} table_id={} action={} apply_ms={apply_ms} save_ms=0 build_state_ms=0",
                    started.elapsed().as_millis(),
                    request.table_id,
                    casino_command_action(&request.command),
                );
            }
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

/// 카지노 웹소켓으로 받는 메시지·프레임의 최대 크기.
const WS_MAX_MESSAGE_BYTES: usize = 16 * 1024;
/// 느린 연결에 보내기가 이만큼 막히면 연결을 끊는다 (클라이언트가 다시 붙는다).
const WS_SEND_TIMEOUT: Duration = Duration::from_secs(5);

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
    // 클라이언트는 {"table": "<id>"}만 보낸다. 기본값(64MiB)으로 메모리를 쓰게 두지 않는다.
    ws.max_message_size(WS_MAX_MESSAGE_BYTES)
        .max_frame_size(WS_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_ws(socket, state, session, token, query.table))
}

/// 바뀐 게 없는 주기에 보내는 하트비트: 시계 맞춤·연결 확인용으로 서버 시각만 담는다
/// (`me`가 없으니 클라이언트는 전체 상태가 아님을 안다).
fn heartbeat(server_time: i64) -> String {
    serde_json::json!({ "server_time": server_time }).to_string()
}

/// 이 알림이 지금 보는 테이블과 관계있는지 (보는 테이블이 없으면 모두).
fn concerns(current: Option<&str>, table_id: &str) -> bool {
    current.is_none_or(|id| id == table_id)
}

/// 쌓여 있는 알림을 모두 비운다. 관계있는 알림이 있었으면 Some(true), 채널이 닫혔으면 None.
/// 느린 연결에서 알림마다 전체 상태를 하나씩 보내며 밀리지 않고, 한 번만 다시 만든다.
fn drain_updates(
    updates: &mut tokio::sync::broadcast::Receiver<crate::casino_hub::CasinoUpdate>,
    current: Option<&str>,
) -> Option<bool> {
    let mut relevant = false;
    loop {
        match updates.try_recv() {
            Ok(update) => relevant |= concerns(current, &update.table_id),
            Err(TryRecvError::Lagged(_)) => relevant = true,
            Err(TryRecvError::Empty) => return Some(relevant),
            Err(TryRecvError::Closed) => return None,
        }
    }
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
    // 느려서 놓친 주기를 몰아서 보내지 않는다.
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut current_table = table;
    // 마지막으로 보낸 화면 (me·table·tables 직렬화). 같으면 전체 대신 하트비트만 보낸다.
    let mut last_sent: Option<String> = None;
    loop {
        // 1초 주기, 보는 테이블의 알림, 테이블 전환 요청 중 하나로 깬다.
        tokio::select! {
            _ = interval.tick() => {},
            update = updates.recv() => {
                let first = match update {
                    Ok(update) => concerns(current_table.as_deref(), &update.table_id),
                    Err(RecvError::Lagged(_)) => true,
                    Err(RecvError::Closed) => break,
                };
                match drain_updates(&mut updates, current_table.as_deref()) {
                    Some(rest) if first || rest => {}
                    Some(_) => continue,
                    None => break,
                }
            },
            incoming = socket.recv() => match incoming {
                // {"table": "<id>"} 로 보는 테이블을 바꿀 수 있다. 바뀔 때만 새로 보낸다.
                Some(Ok(Message::Text(text))) => match table_switch(current_table.as_deref(), &text) {
                    Some(id) => {
                        current_table = Some(id);
                    }
                    None => continue,
                },
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => continue,
            },
        };
        if state.hub.session(&session_token).is_none() {
            // 세션이 사라졌으면(만료) 연결을 끊어 클라이언트가 새 링크를 받게 한다.
            break;
        }
        let build_started = Instant::now();
        let payload = build_state(&state, &session, current_table.as_deref()).await;
        if build_started.elapsed() >= Duration::from_millis(500) {
            eprintln!(
                "casino slow websocket build_state: total_ms={} table_id={}",
                build_started.elapsed().as_millis(),
                current_table.as_deref().unwrap_or("none")
            );
        }
        // 요청한 테이블이 없어져 다른 테이블을 보냈으면 이제 그 테이블의 알림을 따른다.
        current_table = payload.table.as_ref().map(|table| table.id.clone());
        let Ok(snapshot) = serde_json::to_string(&(&payload.me, &payload.table, &payload.tables))
        else {
            continue;
        };
        // 화면이 그대로면 하트비트만 보낸다. 알림(공개 시각이 지남 등)으로 깼을 때도 보낸다:
        // 클라이언트는 하트비트의 server_time으로 "이 시각까지 화면이 그대로"임을 확인하고 연출을 끝낸다.
        let text = if last_sent.as_deref() == Some(snapshot.as_str()) {
            heartbeat(payload.server_time)
        } else {
            let Ok(json) = serde_json::to_string(&payload) else {
                continue;
            };
            last_sent = Some(snapshot);
            json
        };
        match tokio::time::timeout(WS_SEND_TIMEOUT, socket.send(Message::Text(text.into()))).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => break,
        }
    }
}

/// 웹소켓 텍스트 프레임이 {"table": "<id>"}로 다른 테이블을 요청하면 그 id.
/// 지금 보는 테이블이거나 다른 프레임(`{}` 등)이면 None: 상태를 다시 만들지 않는다.
fn table_switch(current: Option<&str>, text: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(text).ok()?;
    let id = value.get("table")?.as_str()?;
    (current != Some(id)).then(|| id.to_string())
}
