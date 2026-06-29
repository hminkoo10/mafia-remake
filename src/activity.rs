// 역할: Discord Activity용 REST API + WebSocket 서버
//        프론트엔드에 게임 상태 제공, 플레이어 액션 수신·처리

use crate::{
    RunningGame,
    runner::{effective_night_role, night_targets},
};
use anyhow::Result;
use axum::{
    Json, Router,
    body::Body,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use dashmap::DashMap;
use mafia_remake::{
    game::{MafiaGame, majority_required},
    model::{Phase, Player, Role},
};
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{RwLock, broadcast};
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
};
use uuid::Uuid;

include!(concat!(env!("OUT_DIR"), "/activity_static.rs"));

// ─────────────────────────────────────────────
// 공유 상태
// ─────────────────────────────────────────────

#[derive(Clone)]
pub struct ActivityState {
    pub games: Arc<DashMap<serenity::GuildId, Arc<RwLock<RunningGame>>>>,
    pub sessions: Arc<DashMap<String, ActivitySession>>,
    pub client_id: String,
    pub client_secret: String,
    pub discord_updates: broadcast::Sender<ActivityDiscordUpdate>,
}

#[derive(Debug, Clone, Copy)]
pub enum ActivityDiscordUpdate {
    PrivateRoleStatus {
        guild_id: serenity::GuildId,
        role: Role,
    },
}

#[derive(Clone)]
pub struct ActivitySession {
    pub user_id: u64,
    #[allow(dead_code)]
    pub username: String,
    #[allow(dead_code)]
    pub guild_id: u64,
    pub expires_at: Instant,
}

impl ActivityState {
    pub fn new(
        games: Arc<DashMap<serenity::GuildId, Arc<RwLock<RunningGame>>>>,
        client_id: String,
        client_secret: String,
        discord_updates: broadcast::Sender<ActivityDiscordUpdate>,
    ) -> Self {
        Self {
            games,
            sessions: Arc::new(DashMap::new()),
            client_id,
            client_secret,
            discord_updates,
        }
    }

    fn get_session(&self, token: &str) -> Option<ActivitySession> {
        let session = self.sessions.get(token)?.clone();
        if session.expires_at < Instant::now() {
            drop(session);
            self.sessions.remove(token);
            return None;
        }
        Some(session)
    }
}

// ─────────────────────────────────────────────
// DTO 타입
// ─────────────────────────────────────────────

#[derive(Serialize)]
pub struct PlayerDto {
    pub id: String,
    pub name: String,
    pub alive: bool,
    pub is_you: bool,
    pub role: Option<String>,      // 본인 역할 / 공개된 역할 / 게임 종료 후
    pub role_team: Option<String>, // "Citizen" | "Mafia" | "Cult" | "Neutral"
}

#[derive(Serialize)]
pub struct ContractorTargetDto {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct GameStateDto {
    pub game_key: String,
    pub phase: String,
    pub day_number: u32,
    pub phase_ends_at: Option<u64>,
    pub players: Vec<PlayerDto>,
    pub my_role: Option<String>,
    pub my_team: Option<String>,
    pub can_act: bool,
    pub my_night_target: Option<String>,
    pub my_action_result: Option<String>, // 밤 행동 결과 텍스트
    pub night_target_ids: Vec<String>,
    pub night_action_can_skip: bool,
    pub special_action: Option<String>,
    pub special_action_target_ids: Vec<String>,
    pub vote_targets: HashMap<String, u32>,
    pub vote_skip_count: u32,
    pub nominee: Option<String>,
    pub confirm_yes: u32,
    pub confirm_no: u32,
    pub winner: Option<String>,
    pub public_status: String,
    pub in_game: bool,
    pub day_skip_count: u32,                          // 낮 스킵 투표 현황
    pub day_skip_threshold: u32,                      // 과반 기준 인원수
    pub contractor_can_act: bool,                     // 청부업자 청부 가능 여부
    pub contractor_targets: Vec<ContractorTargetDto>, // 청부 가능 대상 목록
    pub contractor_guess_roles: Vec<String>,          // 추측 가능 직업 목록
}

#[derive(Deserialize)]
pub struct AuthQuery {
    pub code: String,
    pub guild_id: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub session_token: String,
    pub user_id: String,
    pub username: String,
}

#[derive(Deserialize)]
pub struct StateQuery {
    pub guild_id: String,
}

#[derive(Deserialize)]
pub struct WsQuery {
    pub guild_id: String,
    pub token: String,
}

#[derive(Deserialize)]
pub struct ActionRequest {
    pub guild_id: String,
    pub action: String, // "night_action" | "day_vote" | "confirm_vote" | "skip_vote" | "contractor_action"
    pub target_id: Option<String>,
    pub secondary_target_id: Option<String>,
    pub confirm: Option<bool>, // confirm_vote용
    // contractor_action용
    pub contract_target_ids: Option<[String; 2]>,
    pub contract_roles: Option<[String; 2]>,
}

#[derive(Serialize)]
pub struct ActionResponse {
    pub ok: bool,
    pub message: Option<String>,
}

// ─────────────────────────────────────────────
// 라우터
// ─────────────────────────────────────────────

pub fn activity_router(state: ActivityState, static_dir: Option<String>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let api = Router::new()
        .route("/client-config", get(client_config_handler))
        .route("/auth", get(auth_handler))
        .route("/state", get(state_handler))
        .route("/action", post(action_handler))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let mut router = Router::new()
        .nest("/activity/api", api)
        .fallback(embedded_activity_asset)
        .layer(cors);

    if let Some(dir) = static_dir {
        if Path::new(&dir).join("index.html").is_file() {
            router = router.fallback_service(ServeDir::new(dir));
        } else {
            println!("Embedded Activity UI active.");
        }
    }

    router
}

async fn embedded_activity_asset(uri: Uri) -> Response {
    let request_path = uri.path();
    let path = match request_path {
        "/" | "" => "/index.html",
        path => path,
    };

    if let Some(asset) = ACTIVITY_ASSETS.iter().find(|asset| asset.path == path) {
        return embedded_response(asset);
    }

    if request_path.starts_with("/activity/api") || request_path.starts_with("/activity/ws") {
        return StatusCode::NOT_FOUND.into_response();
    }

    ACTIVITY_ASSETS
        .iter()
        .find(|asset| asset.path == "/index.html")
        .map(embedded_response)
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

fn embedded_response(asset: &EmbeddedActivityAsset) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, asset.content_type)
        .header(header::CACHE_CONTROL, cache_control(asset.path))
        .body(Body::from(asset.body.to_vec()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn cache_control(path: &str) -> &'static str {
    if path == "/index.html" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    }
}

async fn client_config_handler(State(state): State<ActivityState>) -> impl IntoResponse {
    Json(serde_json::json!({ "client_id": state.client_id }))
}

pub async fn run_activity_server(
    state: ActivityState,
    host: String,
    port: u16,
    static_dir: Option<String>,
    tls_cert: Option<String>,
    tls_key: Option<String>,
) {
    let router = activity_router(state, static_dir);
    let addr: std::net::SocketAddr = match format!("{host}:{port}").parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Activity 서버 주소 파싱 실패: {e}");
            return;
        }
    };

    if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
        let config = match axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert, &key).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("TLS 인증서 로드 실패 ({cert}, {key}): {e}");
                return;
            }
        };
        println!("Discord Activity 서버 시작 (HTTPS): https://{addr}");
        if let Err(e) = axum_server::bind_rustls(addr, config)
            .serve(router.into_make_service())
            .await
        {
            eprintln!("Activity 서버 오류: {e}");
        }
    } else {
        let listener = match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Activity 서버 시작 실패 ({addr}): {e}");
                return;
            }
        };
        println!("Discord Activity 서버 시작 (HTTP): http://{addr}");
        if let Err(e) = axum::serve(listener, router).await {
            eprintln!("Activity 서버 오류: {e}");
        }
    }
}

// ─────────────────────────────────────────────
// OAuth 인증
// ─────────────────────────────────────────────

async fn auth_handler(
    State(state): State<ActivityState>,
    Query(query): Query<AuthQuery>,
) -> impl IntoResponse {
    println!("[auth] code={:?} guild_id={:?}", query.code, query.guild_id);

    let guild_id: u64 = match query.guild_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid guild_id" })),
            )
                .into_response();
        }
    };

    // mock_code: 로컬 개발용 (DiscordSDKMock)
    if query.code == "mock_code" {
        println!("[auth] mock mode → returning dummy session_token");
        return Json(serde_json::json!({
            "session_token": "mock_session_token",
            "user_id": "0",
            "username": "Mock User",
        }))
        .into_response();
    }

    // Discord OAuth2 코드 → access_token
    let token_res = reqwest::Client::new()
        .post("https://discord.com/api/oauth2/token")
        .form(&[
            ("client_id", state.client_id.as_str()),
            ("client_secret", state.client_secret.as_str()),
            ("grant_type", "authorization_code"),
            ("code", query.code.as_str()),
        ])
        .send()
        .await;

    let access_token = match token_res {
        Ok(res) if res.status().is_success() => {
            let body: serde_json::Value = res.json().await.unwrap_or_default();
            body["access_token"].as_str().unwrap_or("").to_string()
        }
        Ok(res) => {
            let status = res.status();
            let body: serde_json::Value = res.json().await.unwrap_or_default();
            eprintln!("Discord token exchange failed: status={status}, body={body}");
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "token exchange failed", "detail": body })),
            )
                .into_response();
        }
        Err(e) => {
            eprintln!("Discord token exchange error: {e}");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "token exchange failed", "detail": e.to_string() }))).into_response();
        }
    };

    // access_token → 유저 정보
    let user_res = reqwest::Client::new()
        .get("https://discord.com/api/users/@me")
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await;

    let (user_id, username) = match user_res {
        Ok(res) if res.status().is_success() => {
            let body: serde_json::Value = res.json().await.unwrap_or_default();
            let id = body["id"].as_str().unwrap_or("0").to_string();
            let name = body["global_name"]
                .as_str()
                .or_else(|| body["username"].as_str())
                .unwrap_or("Unknown")
                .to_string();
            (id, name)
        }
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "user fetch failed" })),
            )
                .into_response();
        }
    };

    let user_id_u64: u64 = user_id.parse().unwrap_or(0);
    let session_token = Uuid::new_v4().to_string();
    state.sessions.insert(
        session_token.clone(),
        ActivitySession {
            user_id: user_id_u64,
            username: username.clone(),
            guild_id,
            expires_at: Instant::now() + Duration::from_secs(3600),
        },
    );

    Json(AuthResponse {
        session_token,
        user_id,
        username,
    })
    .into_response()
}

// ─────────────────────────────────────────────
// 게임 상태 조회
// ─────────────────────────────────────────────

fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-Session-Token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

async fn state_handler(
    State(state): State<ActivityState>,
    headers: HeaderMap,
    Query(query): Query<StateQuery>,
) -> impl IntoResponse {
    let token = match extract_session_token(&headers) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "missing token" })),
            )
                .into_response();
        }
    };
    let session = match state.get_session(&token) {
        Some(s) => s,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid session" })),
            )
                .into_response();
        }
    };

    let guild_id: u64 = match query.guild_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid guild_id" })),
            )
                .into_response();
        }
    };

    let guild_key = serenity::GuildId::new(guild_id);
    let game_state = match state.games.get(&guild_key) {
        Some(running_arc) => {
            let mut running = running_arc.write().await;
            build_game_state(&mut running, session.user_id)
        }
        None => GameStateDto {
            game_key: String::new(),
            in_game: false,
            phase: "Ended".into(),
            day_number: 0,
            phase_ends_at: None,
            players: vec![],
            my_role: None,
            my_team: None,
            can_act: false,
            my_night_target: None,
            my_action_result: None,
            night_target_ids: vec![],
            night_action_can_skip: false,
            special_action: None,
            special_action_target_ids: vec![],
            vote_targets: HashMap::new(),
            vote_skip_count: 0,
            nominee: None,
            confirm_yes: 0,
            confirm_no: 0,
            winner: None,
            public_status: "진행 중인 게임이 없습니다.".into(),
            day_skip_count: 0,
            day_skip_threshold: 0,
            contractor_can_act: false,
            contractor_targets: vec![],
            contractor_guess_roles: vec![],
        },
    };

    Json(game_state).into_response()
}

// ─────────────────────────────────────────────
// 액션 제출
// ─────────────────────────────────────────────

async fn action_handler(
    State(state): State<ActivityState>,
    headers: HeaderMap,
    Json(body): Json<ActionRequest>,
) -> impl IntoResponse {
    let token = match extract_session_token(&headers) {
        Some(t) => t,
        None => {
            return Json(ActionResponse {
                ok: false,
                message: Some("인증 필요".into()),
            })
            .into_response();
        }
    };
    let session = match state.get_session(&token) {
        Some(s) => s,
        None => {
            return Json(ActionResponse {
                ok: false,
                message: Some("세션 만료".into()),
            })
            .into_response();
        }
    };

    let guild_id: u64 = match body.guild_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Json(ActionResponse {
                ok: false,
                message: Some("잘못된 guild_id".into()),
            })
            .into_response();
        }
    };

    let guild_key = serenity::GuildId::new(guild_id);
    let running_arc = match state.games.get(&guild_key) {
        Some(r) => r.clone(),
        None => {
            return Json(ActionResponse {
                ok: false,
                message: Some("진행 중인 게임 없음".into()),
            })
            .into_response();
        }
    };

    let mut running = running_arc.write().await;
    let user_id = session.user_id;

    let mut discord_update = None;
    let result: Result<Option<String>, String> = match body.action.as_str() {
        "night_action" => {
            let target = body
                .target_id
                .as_deref()
                .and_then(|s| s.parse::<u64>().ok());
            match running.game.submit_night_action(user_id, target) {
                Ok(_) => {
                    let actor = running.game.get_player(user_id).cloned();
                    if actor.as_ref().is_some_and(|player| {
                        player.role == Role::Mafia
                            || (player.role == Role::Thief
                                && running.game.thief_night_role(player) == Some(Role::Mafia))
                    }) {
                        discord_update = Some(ActivityDiscordUpdate::PrivateRoleStatus {
                            guild_id: guild_key,
                            role: Role::Mafia,
                        });
                    }
                    let message = if actor.as_ref().is_some_and(|player| {
                        player.role == Role::Thief
                            && running.game.thief_night_role(player) == Some(Role::Police)
                    }) {
                        running.game.police_result_for_actor(user_id)
                    } else if actor
                        .as_ref()
                        .is_some_and(|player| player.role == Role::Police)
                    {
                        if running.game.police_result_ready() {
                            Some(running.game.police_result_message())
                        } else {
                            Some("다른 경찰의 선택이 남아 있어 조사 결과가 아직 확정되지 않았습니다.".to_string())
                        }
                    } else {
                        None
                    };
                    if let Some(message) = &message
                        && actor.as_ref().is_some_and(|player| {
                            player.role == Role::Thief
                                && running.game.thief_night_role(player) == Some(Role::Police)
                        })
                    {
                        running
                            .activity_night_results
                            .insert(user_id, message.clone());
                    }
                    if running.game.all_night_actions_submitted() {
                        running.night_notify.notify_one();
                    }
                    Ok(message)
                }
                Err(error) => Err(error.to_string()),
            }
        }
        "day_vote" => {
            let target = body
                .target_id
                .as_deref()
                .and_then(|s| s.parse::<u64>().ok());
            running
                .game
                .submit_day_vote(user_id, target)
                .map_err(|e| e.to_string())
                .map(|_| {
                    if running.game.all_day_votes_submitted() {
                        running.vote_notify.notify_one();
                    }
                    None
                })
        }
        "confirm_vote" => {
            let agree = body.confirm.unwrap_or(false);
            running
                .game
                .submit_confirmation_vote(user_id, agree)
                .map_err(|e| e.to_string())
                .map(|_| {
                    if running.game.all_confirm_votes_submitted() {
                        running.confirm_notify.notify_one();
                    }
                    None
                })
        }
        "skip_vote" => {
            if running.game.phase != Phase::Day {
                return Json(ActionResponse {
                    ok: false,
                    message: Some("지금 진행 중인 낮 토론이 없습니다.".into()),
                })
                .into_response();
            }
            let alive_ids = running
                .game
                .alive_players()
                .into_iter()
                .map(|player| player.user_id)
                .collect::<std::collections::HashSet<_>>();
            if !alive_ids.contains(&user_id) {
                return Json(ActionResponse {
                    ok: false,
                    message: Some("생존 중인 참가자만 바로 투표를 선택할 수 있습니다.".into()),
                })
                .into_response();
            }
            running.day_skip_voter_ids.insert(user_id);
            let required_votes = majority_required(alive_ids.len());
            if running.day_skip_voter_ids.len() >= required_votes {
                running.day_skip_confirmed = true;
                running.day_extension_active = false;
                running.day_notify.notify_waiters();
            }
            Ok(None)
        }
        "contractor_action" => {
            let (ids, roles) = match (&body.contract_target_ids, &body.contract_roles) {
                (Some(ids), Some(roles)) => (ids, roles),
                _ => {
                    return Json(ActionResponse {
                        ok: false,
                        message: Some("contract_target_ids, contract_roles 필요".into()),
                    })
                    .into_response();
                }
            };
            let t1: u64 = match ids[0].parse() {
                Ok(v) => v,
                Err(_) => {
                    return Json(ActionResponse {
                        ok: false,
                        message: Some("잘못된 target_id".into()),
                    })
                    .into_response();
                }
            };
            let t2: u64 = match ids[1].parse() {
                Ok(v) => v,
                Err(_) => {
                    return Json(ActionResponse {
                        ok: false,
                        message: Some("잘못된 target_id".into()),
                    })
                    .into_response();
                }
            };
            let r1 = match role_from_str(&roles[0]) {
                Some(r) => r,
                None => {
                    return Json(ActionResponse {
                        ok: false,
                        message: Some(format!("알 수 없는 직업: {}", roles[0])),
                    })
                    .into_response();
                }
            };
            let r2 = match role_from_str(&roles[1]) {
                Some(r) => r,
                None => {
                    return Json(ActionResponse {
                        ok: false,
                        message: Some(format!("알 수 없는 직업: {}", roles[1])),
                    })
                    .into_response();
                }
            };
            running
                .game
                .submit_contractor_contract(user_id, t1, r1, t2, r2)
                .map_err(|e| e.to_string())
                .map(|_| {
                    if running.game.all_night_actions_submitted() {
                        running.night_notify.notify_one();
                    }
                    None
                })
        }
        "hacker_action" => {
            let Some(target_id) = body.target_id.as_deref().and_then(|id| id.parse().ok()) else {
                return Json(ActionResponse {
                    ok: false,
                    message: Some("target_id 필요".into()),
                })
                .into_response();
            };
            running
                .game
                .submit_hacker_action(user_id, target_id)
                .map(Some)
                .map_err(|e| e.to_string())
        }
        "vigilante_action" => {
            let Some(target_id) = body.target_id.as_deref().and_then(|id| id.parse().ok()) else {
                return Json(ActionResponse {
                    ok: false,
                    message: Some("target_id 필요".into()),
                })
                .into_response();
            };
            running
                .game
                .submit_vigilante_investigation(user_id, target_id)
                .map(Some)
                .map_err(|e| e.to_string())
        }
        "psychologist_action" => {
            let (Some(first_target_id), Some(second_target_id)) = (
                body.target_id.as_deref().and_then(|id| id.parse().ok()),
                body.secondary_target_id
                    .as_deref()
                    .and_then(|id| id.parse().ok()),
            ) else {
                return Json(ActionResponse {
                    ok: false,
                    message: Some("서로 다른 두 target_id 필요".into()),
                })
                .into_response();
            };
            running
                .game
                .submit_psychologist_observation(user_id, first_target_id, second_target_id)
                .map(Some)
                .map_err(|e| e.to_string())
        }
        "hypnotist_action" => running
            .game
            .submit_hypnotist_wake(user_id)
            .map(Some)
            .map_err(|e| e.to_string()),
        "thief_action" => Ok(Some(
            "도벽은 별도 행동이 아니라 지목 투표한 대상에게 자동으로 적용됩니다.".to_string(),
        )),
        _ => Err(format!("알 수 없는 액션: {}", body.action)),
    };

    drop(running);
    if let Some(update) = discord_update {
        let _ = state.discord_updates.send(update);
    }

    match result {
        Ok(message) => Json(ActionResponse { ok: true, message }).into_response(),
        Err(msg) => Json(ActionResponse {
            ok: false,
            message: Some(msg),
        })
        .into_response(),
    }
}

// ─────────────────────────────────────────────
// WebSocket (폴링 기반 실시간 업데이트)
// ─────────────────────────────────────────────

async fn ws_handler(
    State(state): State<ActivityState>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    // WebSocket 업그레이드 요청은 브라우저에서 커스텀 헤더를 보낼 수 없으므로
    // 토큰을 쿼리 파라미터로 받는다.
    let session = match state.get_session(&query.token) {
        Some(s) => s,
        None => return (StatusCode::UNAUTHORIZED, "invalid session").into_response(),
    };
    let guild_id: u64 = match query.guild_id.parse() {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid guild_id").into_response(),
    };

    ws.on_upgrade(move |socket| handle_ws(socket, state, session.user_id, guild_id))
}

async fn handle_ws(mut socket: WebSocket, state: ActivityState, user_id: u64, guild_id: u64) {
    let guild_key = serenity::GuildId::new(guild_id);
    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let dto = match state.games.get(&guild_key) {
                    Some(arc) => {
                        let mut running = arc.write().await;
                        build_game_state(&mut running, user_id)
                    }
                    None => GameStateDto {
                        game_key: String::new(),
                        in_game: false,
                        phase: "Ended".into(),
                        day_number: 0,
                        phase_ends_at: None,
                        players: vec![],
                        my_role: None,
                        my_team: None,
                        can_act: false,
                        my_night_target: None,
                        my_action_result: None,
                        night_target_ids: vec![],
                        night_action_can_skip: false,
                        special_action: None,
                        special_action_target_ids: vec![],
                        vote_targets: HashMap::new(),
                        vote_skip_count: 0,
                        nominee: None,
                        confirm_yes: 0,
                        confirm_no: 0,
                        winner: None,
                        public_status: "진행 중인 게임이 없습니다.".into(),
                        day_skip_count: 0,
                        day_skip_threshold: 0,
                        contractor_can_act: false,
                        contractor_targets: vec![],
                        contractor_guess_roles: vec![],
                    },
                };

                let json = match serde_json::to_string(&dto) {
                    Ok(j) => j,
                    Err(_) => continue,
                };

                if socket.send(Message::Text(json.into())).await.is_err() {
                    break; // 클라이언트 연결 끊김
                }
            }
            Some(msg) = socket.recv() => {
                match msg {
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {} // ping 등 무시
                }
            }
        }
    }
}

// ─────────────────────────────────────────────
// 게임 상태 직렬화 헬퍼
// ─────────────────────────────────────────────

fn build_game_state(running: &mut RunningGame, user_id: u64) -> GameStateDto {
    let game = &mut running.game;
    let phase_str = match game.phase {
        Phase::Night => "Night",
        Phase::Day => "Day",
        Phase::Vote => "Vote",
        Phase::FinalDefense => "FinalDefense",
        Phase::ConfirmVote => "ConfirmVote",
        Phase::Ended => "Ended",
    };

    let me = game.get_player(user_id).cloned();
    let game_ended = matches!(game.phase, Phase::Ended);
    let reveal_roles = running.reveal_death_roles;
    // me 레퍼런스가 살아있으면 game 뮤터블 메서드를 못 쓰므로 필요한 값을 미리 복사
    let me_alive = me.as_ref().is_some_and(|player| player.alive);
    let me_role_for_targets = me.clone();

    let special_action = if me_alive
        && game
            .hacker_day_actors()
            .iter()
            .any(|player| player.user_id == user_id)
    {
        Some("hacker".to_string())
    } else if me_alive
        && game
            .vigilante_day_actors()
            .iter()
            .any(|player| player.user_id == user_id)
    {
        Some("vigilante".to_string())
    } else if me_alive
        && game
            .psychologist_day_actors()
            .iter()
            .any(|player| player.user_id == user_id)
    {
        Some("psychologist".to_string())
    } else if me_alive
        && game
            .hypnotist_day_actors()
            .iter()
            .any(|player| player.user_id == user_id)
    {
        Some("hypnotist".to_string())
    } else {
        None
    };

    let contractor_can_act =
        me_alive && game.contractor_can_use_contract(user_id) && matches!(game.phase, Phase::Night);
    let night_action_available = me_alive
        && matches!(game.phase, Phase::Night)
        && game
            .night_action_actors()
            .iter()
            .any(|player| player.user_id == user_id);
    let (night_target_ids, night_action_can_skip) = if night_action_available && !contractor_can_act
    {
        if let Some(player) = &me_role_for_targets {
            let role = effective_night_role(game, player);
            (
                night_targets(game, player)
                    .into_iter()
                    .map(|target| target.user_id.to_string())
                    .collect(),
                role == Role::Reporter,
            )
        } else {
            (vec![], false)
        }
    } else {
        (vec![], false)
    };
    let special_action_target_ids = if special_action.as_deref() == Some("hypnotist") {
        vec![]
    } else if special_action.is_some() {
        game.alive_players()
            .into_iter()
            .filter(|player| player.user_id != user_id)
            .map(|player| player.user_id.to_string())
            .collect()
    } else {
        vec![]
    };

    // 플레이어 목록
    let players = game
        .all_players()
        .iter()
        .map(|p| {
            let is_you = p.user_id == user_id;
            let role_visible =
                is_you || game_ended || (reveal_roles && !p.alive) || game.is_publicly_revealed(p);

            let display_name = if running.anonymous_enabled {
                running
                    .anonymous_aliases
                    .get(&p.user_id)
                    .cloned()
                    .unwrap_or_else(|| p.name.clone())
            } else {
                p.name.clone()
            };

            PlayerDto {
                id: p.user_id.to_string(),
                name: display_name,
                alive: p.alive,
                is_you,
                role: role_visible.then(|| role_name(game.visible_role(p))),
                role_team: role_visible.then(|| player_team(game, p)),
            }
        })
        .collect();

    // 내 역할 / 팀
    let (my_role, my_team) = match me.as_ref() {
        Some(player) => (
            Some(role_name(game.visible_role(player))),
            Some(player_team(game, player)),
        ),
        None => (None, None),
    };

    // 행동 가능 여부 (me 레퍼런스 해제 후 game 뮤터블 메서드 호출)
    let can_act = me_alive
        && match game.phase {
            Phase::Night => night_action_available && !contractor_can_act,
            Phase::Day | Phase::Vote => true,
            Phase::ConfirmVote => game.alive_players().iter().any(|p| p.user_id == user_id),
            _ => false,
        };

    // 밤 지목 대상
    let my_night_target = if matches!(game.phase, Phase::Night) {
        game.get_night_action_target(user_id)
            .map(|id| id.to_string())
    } else {
        None
    };

    // 밤 행동 결과 (낮에만 표시)
    let my_action_result = if matches!(game.phase, Phase::Day) {
        running.activity_night_results.get(&user_id).cloned()
    } else {
        None
    };

    // 낮 투표 현황
    let vote_targets = if matches!(game.phase, Phase::Vote) {
        game.current_vote_counts()
            .into_iter()
            .map(|(id, count)| (id.to_string(), count as u32))
            .collect()
    } else {
        HashMap::new()
    };
    let vote_skip_count = if matches!(game.phase, Phase::Vote) {
        game.day_votes
            .values()
            .filter(|target| target.is_none())
            .count() as u32
    } else {
        0
    };

    // 현재 지목된 플레이어: Vote 이후 단계에서는 running에 저장됨
    let nominee = if matches!(game.phase, Phase::FinalDefense | Phase::ConfirmVote) {
        running.final_defense_user_id.map(|id| id.to_string())
    } else {
        None
    };

    // 찬반 현황
    let (confirm_yes, confirm_no) = game.current_confirm_counts();

    // 승자
    let winner = game.winner().map(|w| format!("{w:?}"));

    // 공개 상태 텍스트
    let public_status = game.public_status();

    // 낮 스킵 현황
    let alive_count = game.alive_players().len() as u32;
    let (day_skip_count, day_skip_threshold) = if matches!(game.phase, Phase::Day) {
        (
            running.day_skip_voter_ids.len() as u32,
            majority_required(alive_count as usize) as u32,
        )
    } else {
        (0, 0)
    };

    let contractor_targets = if contractor_can_act {
        if let Some(me_player) = &me_role_for_targets {
            game.contractor_contract_targets(me_player)
                .iter()
                .map(|p| ContractorTargetDto {
                    id: p.user_id.to_string(),
                    name: p.name.clone(),
                })
                .collect()
        } else {
            vec![]
        }
    } else {
        vec![]
    };
    let contractor_guess_roles = if contractor_can_act {
        mafia_remake::model::CONTRACTOR_GUESS_ROLES
            .iter()
            .map(|r| r.value().to_string())
            .collect()
    } else {
        vec![]
    };

    let phase_ends_at = phase_deadline_unix_ms(running.phase_deadline, game.phase);

    GameStateDto {
        game_key: running.activity_game_key.clone(),
        in_game: true,
        phase: phase_str.to_string(),
        day_number: game.day_number,
        phase_ends_at,
        players,
        my_role,
        my_team,
        can_act,
        my_night_target,
        my_action_result,
        night_target_ids,
        night_action_can_skip,
        special_action,
        special_action_target_ids,
        vote_targets,
        vote_skip_count,
        nominee,
        confirm_yes: confirm_yes as u32,
        confirm_no: confirm_no as u32,
        winner,
        public_status,
        day_skip_count,
        day_skip_threshold,
        contractor_can_act,
        contractor_targets,
        contractor_guess_roles,
    }
}

fn phase_deadline_unix_ms(deadline: Option<Instant>, phase: Phase) -> Option<u64> {
    if matches!(phase, Phase::Ended) {
        return None;
    }
    let remaining = deadline?.saturating_duration_since(Instant::now());
    let deadline = SystemTime::now().checked_add(remaining)?;
    let millis = deadline.duration_since(UNIX_EPOCH).ok()?.as_millis();
    Some(millis.min(u128::from(u64::MAX)) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activity_test_running(mut game: MafiaGame) -> RunningGame {
        let initial_roles = game
            .players
            .iter()
            .map(|player| (player.user_id, player.role))
            .collect();
        let participant_user_ids = game.players.iter().map(|player| player.user_id).collect();
        game.phase = Phase::Day;
        RunningGame {
            guild_id: serenity::GuildId::new(1),
            channel_id: serenity::ChannelId::new(10),
            participant_user_ids,
            spectator_user_ids: Default::default(),
            game,
            reveal_death_roles: true,
            anonymous_enabled: false,
            started_at: Instant::now(),
            activity_game_key: "test-game".to_string(),
            phase_deadline: None,
            initial_roles,
            memos: Default::default(),
            game_status_message_id: None,
            game_status_text: None,
            anonymous_aliases: Default::default(),
            anonymous_original_names: Default::default(),
            anonymous_input_channel_ids: Default::default(),
            anonymous_input_channel_owners: Default::default(),
            anonymous_dead_input_channel_ids: Default::default(),
            anonymous_dead_input_channel_owners: Default::default(),
            dead_chat_unlocked_ids: Default::default(),
            pending_dead_chat_user_ids: Default::default(),
            anonymous_shaman_input_channel_ids: Default::default(),
            anonymous_shaman_input_channel_owners: Default::default(),
            anonymous_role_input_channel_ids: Default::default(),
            anonymous_role_input_channels: Default::default(),
            anonymous_role_input_status_message_ids: Default::default(),
            anonymous_role_status_texts: Default::default(),
            anonymous_channel_topics: Default::default(),
            anonymous_webhook_urls: Default::default(),
            original_game_channel_overwrites: Default::default(),
            game_channel_overwrites: Default::default(),
            member_channel_overwrites: Default::default(),
            original_slowmode_delays: Default::default(),
            private_channel_ids: Default::default(),
            private_role_status_message_ids: Default::default(),
            private_role_status_texts: Default::default(),
            memo_channel_ids: Default::default(),
            shaman_channel_id: None,
            shaman_status_message_id: None,
            shaman_status_text: None,
            frog_channel_id: None,
            frog_game_channel_overwrites: Default::default(),
            madam_seduction_channel_overwrites: Default::default(),
            day_chat_open: true,
            final_defense_user_id: None,
            day_skip_voter_ids: Default::default(),
            day_skip_confirmed: false,
            day_extension_voter_ids: Default::default(),
            day_extension_active: false,
            day_extension_confirmed: false,
            night_timed_events_due: false,
            contractor_contract_drafts: Default::default(),
            activity_night_results: Default::default(),
            night_notify: Arc::new(tokio::sync::Notify::new()),
            vote_notify: Arc::new(tokio::sync::Notify::new()),
            confirm_notify: Arc::new(tokio::sync::Notify::new()),
            day_notify: Arc::new(tokio::sync::Notify::new()),
            stats_recorded: false,
        }
    }

    fn activity_test_game() -> MafiaGame {
        MafiaGame::new(
            vec![
                (1, "p1".to_string()),
                (2, "p2".to_string()),
                (3, "p3".to_string()),
                (4, "p4".to_string()),
                (5, "p5".to_string()),
            ],
            1,
            0,
            0,
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn active_phase_deadline_is_unix_ms() {
        let deadline = Instant::now() + Duration::from_secs(30);
        let millis = phase_deadline_unix_ms(Some(deadline), Phase::Night).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        assert!(millis >= now);
    }

    #[test]
    fn ended_phase_has_no_deadline() {
        let deadline = Instant::now() + Duration::from_secs(30);
        assert_eq!(phase_deadline_unix_ms(Some(deadline), Phase::Ended), None);
    }

    #[test]
    fn activity_state_preserves_dead_players() {
        let mut game = activity_test_game();
        game.get_player_mut(2).unwrap().alive = false;
        let mut running = activity_test_running(game);

        let state = build_game_state(&mut running, 1);

        assert!(
            !state
                .players
                .iter()
                .find(|player| player.id == "2")
                .unwrap()
                .alive
        );
        assert!(
            state
                .players
                .iter()
                .find(|player| player.id == "1")
                .unwrap()
                .alive
        );
    }

    #[test]
    fn dead_chat_requires_unlock_after_death() {
        let mut game = activity_test_game();
        game.get_player_mut(2).unwrap().alive = false;
        let mut running = activity_test_running(game);
        let player = running.game.get_player(2).unwrap().clone();

        assert!(!crate::channel::can_use_anonymous_dead_chat(
            &running, &player
        ));
        assert!(!crate::channel::can_use_anonymous_shaman_chat(
            &running, &player
        ));

        running.dead_chat_unlocked_ids.insert(player.user_id);

        assert!(crate::channel::can_use_anonymous_dead_chat(
            &running, &player
        ));
        assert!(crate::channel::can_use_anonymous_shaman_chat(
            &running, &player
        ));
    }

    #[test]
    fn activity_vote_state_is_phase_scoped_and_counts_skip() {
        let mut running = activity_test_running(activity_test_game());
        running.game.phase = Phase::Vote;
        running.game.day_votes.insert(1, Some(2));
        running.game.day_votes.insert(2, None);

        let vote_state = build_game_state(&mut running, 1);
        assert_eq!(vote_state.vote_targets.get("2"), Some(&1));
        assert_eq!(vote_state.vote_skip_count, 1);

        running.game.phase = Phase::Night;
        let night_state = build_game_state(&mut running, 1);
        assert!(night_state.vote_targets.is_empty());
        assert_eq!(night_state.vote_skip_count, 0);
        assert_eq!(night_state.nominee, None);
    }

    #[test]
    fn activity_role_names_round_trip() {
        let roles = [
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
            Role::Hypnotist,
            Role::Mercenary,
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
            Role::Frog,
            Role::Villain,
            Role::Citizen,
        ];

        for role in roles {
            assert_eq!(role_from_str(&role_name(role)), Some(role));
        }
    }

    #[test]
    fn activity_team_uses_game_team_rules() {
        let mut game = MafiaGame::new(
            vec![
                (1, "p1".to_string()),
                (2, "p2".to_string()),
                (3, "p3".to_string()),
            ],
            1,
            0,
            0,
            vec![],
        )
        .unwrap();

        for role in [
            Role::Mafia,
            Role::Spy,
            Role::Contractor,
            Role::Thief,
            Role::Witch,
            Role::Scientist,
            Role::Madam,
            Role::Godfather,
            Role::Villain,
        ] {
            assert_eq!(player_team(&game, &Player::new(99, "test", role)), "Mafia");
        }
        for role in [
            Role::Gangster,
            Role::Fanatic,
            Role::Hypnotist,
            Role::Mercenary,
            Role::Citizen,
        ] {
            assert_eq!(
                player_team(&game, &Player::new(99, "test", role)),
                "Citizen"
            );
        }
        assert_eq!(
            player_team(&game, &Player::new(99, "test", Role::CultLeader)),
            "Cult"
        );
        assert_eq!(
            player_team(&game, &Player::new(99, "test", Role::Joker)),
            "Neutral"
        );

        game.culted_ids.insert(99);
        assert_eq!(
            player_team(&game, &Player::new(99, "test", Role::Thief)),
            "Mafia"
        );
        assert_eq!(
            player_team(&game, &Player::new(99, "test", Role::Fanatic)),
            "Cult"
        );
    }
}

fn role_name(role: Role) -> String {
    match role {
        Role::Citizen => "시민",
        Role::Mafia => "마피아",
        Role::Police => "경찰",
        Role::Doctor => "의사",
        Role::Agent => "요원",
        Role::Vigilante => "자경단",
        Role::Detective => "탐정",
        Role::Reporter => "기자",
        Role::Hacker => "해커",
        Role::Terrorist => "테러리스트",
        Role::Politician => "정치인",
        Role::Judge => "판사",
        Role::Psychologist => "심리학자",
        Role::Hypnotist => "최면술사",
        Role::Mercenary => "용병",
        Role::Thief => "도둑",
        Role::Soldier => "군인",
        Role::Nurse => "간호사",
        Role::Prophet => "예언자",
        Role::Shaman => "영매",
        Role::Lover => "연인",
        Role::Godfather => "대부",
        Role::Gangster => "건달",
        Role::Spy => "스파이",
        Role::CultLeader => "교주",
        Role::Fanatic => "광신도",
        Role::Madam => "마담",
        Role::Witch => "마녀",
        Role::Scientist => "과학자",
        Role::Contractor => "청부업자",
        Role::Joker => "조커",
        Role::Priest => "성직자",
        Role::Frog => "개구리",
        Role::Graverobber => "도굴꾼",
        Role::Villain => "악인",
    }
    .to_string()
}

fn player_team(game: &MafiaGame, player: &Player) -> String {
    if player.role == Role::Thief {
        "Mafia"
    } else if game.is_cult_team(player) {
        "Cult"
    } else if game.is_mafia_team(player) {
        "Mafia"
    } else if player.role == Role::Joker {
        "Neutral"
    } else {
        "Citizen"
    }
    .to_string()
}

fn role_from_str(s: &str) -> Option<Role> {
    use Role::*;
    Some(match s {
        "마피아" => Mafia,
        "의사" => Doctor,
        "간호사" => Nurse,
        "경찰" => Police,
        "요원" => Agent,
        "자경단" | "자경단원" => Vigilante,
        "기자" => Reporter,
        "해커" => Hacker,
        "탐정" | "사립탐정" => Detective,
        "영매" => Shaman,
        "성직자" => Priest,
        "군인" => Soldier,
        "건달" => Gangster,
        "예언자" => Prophet,
        "심리학자" => Psychologist,
        "최면술사" => Hypnotist,
        "용병" => Mercenary,
        "스파이" => Spy,
        "청부업자" => Contractor,
        "도둑" => Thief,
        "마녀" => Witch,
        "과학자" => Scientist,
        "마담" => Madam,
        "도굴꾼" => Graverobber,
        "대부" => Godfather,
        "조커" => Joker,
        "정치인" => Politician,
        "판사" => Judge,
        "테러리스트" => Terrorist,
        "연인" => Lover,
        "교주" => CultLeader,
        "광신도" => Fanatic,
        "개구리" => Frog,
        "악인" => Villain,
        "시민" => Citizen,
        _ => return None,
    })
}
