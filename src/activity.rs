// 역할: Discord Activity용 REST API + WebSocket 서버
//        프론트엔드에 게임 상태 제공, 플레이어 액션 수신·처리

use crate::RunningGame;
use anyhow::Result;
use axum::{
    Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json,
};
use dashmap::DashMap;
use mafia_remake::model::{Phase, Role};
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
};
use uuid::Uuid;

// ─────────────────────────────────────────────
// 공유 상태
// ─────────────────────────────────────────────

#[derive(Clone)]
pub struct ActivityState {
    pub games: Arc<DashMap<serenity::GuildId, Arc<RwLock<RunningGame>>>>,
    pub sessions: Arc<DashMap<String, ActivitySession>>,
    pub client_id: String,
    pub client_secret: String,
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
    ) -> Self {
        Self {
            games,
            sessions: Arc::new(DashMap::new()),
            client_id,
            client_secret,
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
pub struct GameStateDto {
    pub phase: String,
    pub day_number: u32,
    pub phase_ends_at: Option<u64>, // unix ms (추후 phase 타이머 연동 시 채울 것)
    pub players: Vec<PlayerDto>,
    pub my_role: Option<String>,
    pub my_team: Option<String>,
    pub can_act: bool,
    pub my_night_target: Option<String>,    // 내가 오늘 밤 지목한 대상
    pub vote_targets: HashMap<String, u32>, // targetId → 득표수
    pub nominee: Option<String>,
    pub confirm_yes: u32,
    pub confirm_no: u32,
    pub winner: Option<String>,
    pub public_status: String,
    pub in_game: bool,
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
    pub action: String,         // "night_action" | "day_vote" | "confirm_vote" | "skip_vote"
    pub target_id: Option<String>,
    pub confirm: Option<bool>,  // confirm_vote용
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
        .route("/auth", get(auth_handler))
        .route("/state", get(state_handler))
        .route("/action", post(action_handler))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let mut router = Router::new().nest("/activity/api", api).layer(cors);

    if let Some(dir) = static_dir {
        router = router.nest_service("/", ServeDir::new(dir));
    }

    router
}

pub async fn run_activity_server(state: ActivityState, host: String, port: u16, static_dir: Option<String>) {
    let router = activity_router(state, static_dir);
    let addr = format!("{host}:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Activity 서버 시작 실패 ({addr}): {e}");
            return;
        }
    };
    println!("Discord Activity 서버 시작: http://{addr}");
    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("Activity 서버 오류: {e}");
    }
}

// ─────────────────────────────────────────────
// OAuth 인증
// ─────────────────────────────────────────────

async fn auth_handler(
    State(state): State<ActivityState>,
    Query(query): Query<AuthQuery>,
) -> impl IntoResponse {
    let guild_id: u64 = match query.guild_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "invalid guild_id" }))).into_response();
        }
    };

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
        _ => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "token exchange failed" }))).into_response();
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
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "user fetch failed" }))).into_response();
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
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "missing token" }))).into_response(),
    };
    let session = match state.get_session(&token) {
        Some(s) => s,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "invalid session" }))).into_response(),
    };

    let guild_id: u64 = match query.guild_id.parse() {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "invalid guild_id" }))).into_response(),
    };

    let guild_key = serenity::GuildId::new(guild_id);
    let game_state = match state.games.get(&guild_key) {
        Some(running_arc) => {
            let mut running = running_arc.write().await;
            build_game_state(&mut running, session.user_id)
        }
        None => GameStateDto {
            in_game: false,
            phase: "Ended".into(),
            day_number: 0,
            phase_ends_at: None,
            players: vec![],
            my_role: None,
            my_team: None,
            can_act: false,
            my_night_target: None,
            vote_targets: HashMap::new(),
            nominee: None,
            confirm_yes: 0,
            confirm_no: 0,
            winner: None,
            public_status: "진행 중인 게임이 없습니다.".into(),
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
            return Json(ActionResponse { ok: false, message: Some("인증 필요".into()) }).into_response();
        }
    };
    let session = match state.get_session(&token) {
        Some(s) => s,
        None => {
            return Json(ActionResponse { ok: false, message: Some("세션 만료".into()) }).into_response();
        }
    };

    let guild_id: u64 = match body.guild_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Json(ActionResponse { ok: false, message: Some("잘못된 guild_id".into()) }).into_response();
        }
    };

    let guild_key = serenity::GuildId::new(guild_id);
    let running_arc = match state.games.get(&guild_key) {
        Some(r) => r.clone(),
        None => {
            return Json(ActionResponse { ok: false, message: Some("진행 중인 게임 없음".into()) }).into_response();
        }
    };

    let mut running = running_arc.write().await;
    let user_id = session.user_id;

    let result: Result<(), String> = match body.action.as_str() {
        "night_action" => {
            let target = body.target_id.as_deref()
                .and_then(|s| s.parse::<u64>().ok());
            running.game.submit_night_action(user_id, target)
                .map_err(|e| e.to_string())
                .map(|_| {
                    if running.game.all_night_actions_submitted() {
                        running.night_notify.notify_one();
                    }
                })
        }
        "day_vote" => {
            let target = body.target_id.as_deref()
                .and_then(|s| s.parse::<u64>().ok());
            running.game.submit_day_vote(user_id, target)
                .map_err(|e| e.to_string())
                .map(|_| {
                    if running.game.all_day_votes_submitted() {
                        running.vote_notify.notify_one();
                    }
                })
        }
        "confirm_vote" => {
            let agree = body.confirm.unwrap_or(false);
            running.game.submit_confirmation_vote(user_id, agree)
                .map_err(|e| e.to_string())
                .map(|_| {
                    if running.game.all_confirm_votes_submitted() {
                        running.confirm_notify.notify_one();
                    }
                })
        }
        "skip_vote" => {
            running.day_skip_voter_ids.insert(user_id);
            running.day_notify.notify_one();
            Ok(())
        }
        _ => Err(format!("알 수 없는 액션: {}", body.action)),
    };

    match result {
        Ok(()) => Json(ActionResponse { ok: true, message: None }).into_response(),
        Err(msg) => Json(ActionResponse { ok: false, message: Some(msg) }).into_response(),
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

async fn handle_ws(
    mut socket: WebSocket,
    state: ActivityState,
    user_id: u64,
    guild_id: u64,
) {
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
                        in_game: false,
                        phase: "Ended".into(),
                        day_number: 0,
                        phase_ends_at: None,
                        players: vec![],
                        my_role: None,
                        my_team: None,
                        can_act: false,
                        my_night_target: None,
                        vote_targets: HashMap::new(),
                        nominee: None,
                        confirm_yes: 0,
                        confirm_no: 0,
                        winner: None,
                        public_status: "진행 중인 게임이 없습니다.".into(),
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

    let me = game.get_player(user_id);
    let game_ended = matches!(game.phase, Phase::Ended);
    let reveal_roles = running.reveal_death_roles;

    // 플레이어 목록
    let players = game.all_players().iter().map(|p| {
        let is_you = p.user_id == user_id;
        let role_visible = is_you
            || game_ended
            || (reveal_roles && !p.alive)
            || game.is_publicly_revealed(p);

        let display_name = if running.anonymous_enabled {
            running.anonymous_aliases.get(&p.user_id)
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
            role: if role_visible { Some(role_name(p.role)) } else { None },
            role_team: if role_visible { Some(role_team(p.role)) } else { None },
        }
    }).collect();

    // 내 역할 / 팀
    let (my_role, my_team) = match me {
        Some(p) => (Some(role_name(p.role)), Some(role_team(p.role))),
        None => (None, None),
    };

    // 행동 가능 여부
    let can_act = me.map(|p| p.alive).unwrap_or(false) && match game.phase {
        Phase::Night => game.night_action_actors().iter().any(|a| a.user_id == user_id),
        Phase::Vote => true,
        Phase::ConfirmVote => game.alive_players().iter().any(|p| p.user_id == user_id),
        _ => false,
    };

    // 밤 지목 대상
    let my_night_target = if matches!(game.phase, Phase::Night) {
        game.get_night_action_target(user_id).map(|id| id.to_string())
    } else {
        None
    };

    // 낮 투표 현황
    let vote_targets = game.current_vote_counts()
        .into_iter()
        .map(|(id, count)| (id.to_string(), count as u32))
        .collect();

    // 현재 지목된 플레이어: Vote 이후 단계에서는 running에 저장됨
    let nominee = running.final_defense_user_id.map(|id| id.to_string())
        .or_else(|| {
            // Vote 단계: 최다 득표자
            game.current_vote_counts()
                .into_iter()
                .max_by_key(|(_, c)| *c)
                .map(|(id, _)| id.to_string())
        });

    // 찬반 현황
    let (confirm_yes, confirm_no) = game.current_confirm_counts();

    // 승자
    let winner = game.winner().map(|w| format!("{w:?}"));

    // 공개 상태 텍스트
    let public_status = game.public_status();

    GameStateDto {
        in_game: true,
        phase: phase_str.to_string(),
        day_number: game.day_number,
        phase_ends_at: None, // TODO: 타이머 연동
        players,
        my_role,
        my_team,
        can_act,
        my_night_target,
        vote_targets,
        nominee,
        confirm_yes: confirm_yes as u32,
        confirm_no: confirm_no as u32,
        winner,
        public_status,
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

fn role_team(role: Role) -> String {
    match role {
        Role::Mafia | Role::Godfather | Role::Gangster | Role::Spy
        | Role::Madam | Role::Witch => "Mafia",
        Role::CultLeader | Role::Fanatic => "Cult",
        Role::Joker | Role::Contractor | Role::Villain => "Neutral",
        _ => "Citizen",
    }
    .to_string()
}
