// web_settings 테스트 모듈 (src/web_settings.rs에서 분리)

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
        recruitment_seconds: 60,
        night_seconds: 60,
        discussion_seconds: 60,
        vote_seconds: 30,
        chat_slowmode_seconds: 3,
        attendance_coins: 10_000,
        star_player_coins: 1_000,
        holdem_rake_bp: 250,
        holdem_rake_cap: 500,
        gift_fee_bp: 300,
        bet_loss_treasury_bp: 5_000,
        jackpot_share_bp: 2_000,
        jackpot_payout_bp: 5_000,
        jackpot_loser_bp: 5_000,
        jackpot_winner_bp: 2_500,
        weekly_cashback_bp: 1_000,
        weekly_cashback_cap: 20_000,
        daily_rolling_bp: 20,
        daily_rolling_cap: 5_000,
        relief_threshold: 2_000,
        relief_amount: 5_000,
        relief_gift_lock_hours: 24,
        stock_enabled: true,
        stock_day_minutes: 60,
        stock_fee_ppm: 150,
        stock_tax_ppm: 2_000,
        stock_limit_bp: 3_000,
        stock_vi_bp: 1_000,
        stock_drift_bp_week: 10,
        stock_vol_pct: 100,
        stock_news_pct: 100,
        stock_holding_limit_bp: 500,
        stock_order_limit_pct: 20,
        stock_found_min_capital: 1_000_000,
        stock_found_fee_bp: 100,
        stock_ipo_fee_bp: 100,
        stock_listing_min_equity: 1_000_000,
        stock_lockup_days: 24,
        stock_max_companies: 1,
        stock_system_companies: 12,
        coupon_api_url: String::new(),
        coupon_api_key: String::new(),
        coupon_coins_per_point: 10_000,
        log_channel_id: 0,
        home_guild_id: 0,
        reveal_death_roles: true,
        reveal_public_police_status: true,
        reveal_morning_mafia_count: true,
        show_confirmation_vote_counts: true,
        citizen_special_count: 0,
        mafia_special_count: 0,
        neutral_special_count: 0,
        enable_detective: true,
        enable_inspector: true,
        enable_graverobber: true,
        enable_spy: true,
        enable_contractor: true,
        enable_fraudster: false,
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
        enable_civil_servant: false,
        enable_paparazzi: false,
        enable_shaman: true,
        enable_priest: true,
        enable_soldier: true,
        enable_nurse: true,
        enable_gangster: true,
        enable_prophet: true,
        enable_psychologist: true,
        enable_hypnotist: true,
        enable_mercenary: true,
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
        completed_replays: Arc::new(RwLock::new(VecDeque::new())),
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
fn lover_does_not_inflate_web_minimum() {
    let mut config = test_config();
    config.default_mafia_count = 1;
    config.citizen_special_count = 1;
    config.max_player_count = 4;

    assert!(validate_config(&config).is_ok());
}

#[test]
fn base_joker_count_is_included_in_web_minimum() {
    let mut config = test_config();
    config.default_mafia_count = 1;
    config.default_doctor_count = 0;
    config.default_police_count = 0;
    config.default_joker_count = 3;

    assert_eq!(minimum_player_count(&config), 4);

    config.enable_joker = false;
    assert_eq!(minimum_player_count(&config), 3);
}

#[test]
fn recruitment_seconds_is_settable_and_clamped() {
    let mut config = test_config();

    set_int(&mut config, "recruitment_seconds", 300).unwrap();
    assert_eq!(config.recruitment_seconds, 300);
    assert_eq!(config.effective_recruitment_seconds(), 300);
    assert_eq!(config_value(&config, "recruitment_seconds"), "300");

    // 극단값은 모집 루프가 버틸 범위로 잘라 저장한다.
    set_int(&mut config, "recruitment_seconds", 0).unwrap();
    assert_eq!(config.recruitment_seconds, config::MIN_RECRUITMENT_SECONDS);
    set_int(&mut config, "recruitment_seconds", 99_999).unwrap();
    assert_eq!(config.recruitment_seconds, config::MAX_RECRUITMENT_SECONDS);
}

#[test]
fn lover_uses_two_citizen_special_slots() {
    let mut config = test_config();
    config.citizen_special_count = 2;
    config.enable_detective = false;
    config.enable_graverobber = false;
    config.enable_politician = false;
    config.enable_judge = false;
    config.enable_reporter = false;
    config.enable_hacker = false;
    config.enable_terrorist = false;
    config.enable_shaman = false;
    config.enable_priest = false;
    config.enable_soldier = false;
    config.enable_nurse = false;
    config.enable_gangster = false;
    config.enable_prophet = false;
    config.enable_psychologist = false;
    config.enable_hypnotist = false;
    config.enable_mercenary = false;

    let roles = crate::channel::choose_special_roles(&config).unwrap();
    let role_counts = crate::channel::selected_role_counts(&config, &roles).unwrap();

    assert_eq!(roles, vec![Role::Lover]);
    assert_eq!(role_counts.get(&Role::Lover), Some(&2));
    assert_eq!(crate::channel::minimum_player_count(&role_counts), 6);
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
        api_request(
            "GET",
            "/api/v1/me",
            Some(("Authorization", &format!("Bearer {raw_key}"))),
        ),
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
async fn protected_replay_api_returns_completed_replay() {
    let state = test_state();
    let raw_key = {
        let mut store = state.api_keys.write().await;
        issue_api_key(&mut store, 1, 2, "replay".to_string())
    };
    state.completed_replays.write().await.push_front(json!({
        "game_key": "game-1",
        "guild_id": 1,
        "channel_id": 10,
        "status": "completed",
        "phase": "종료",
        "phase_key": "Ended",
        "day_number": 3,
        "elapsed_seconds": 123,
        "winner": "시민",
        "winner_key": "Citizen",
        "participants": [],
        "events": [{"kind": "day_vote"}],
        "rating_log": [],
    }));

    let list_response = route_request(
        &state,
        api_request("GET", "/api/v1/replays", Some(("X-API-Key", &raw_key))),
    )
    .await;
    assert!(list_response.starts_with("HTTP/1.1 200 OK"));
    assert!(list_response.contains("game-1"));
    assert!(list_response.contains(r#""event_count":1"#));

    let replay_response = route_request(
        &state,
        api_request(
            "GET",
            "/api/v1/replays/game-1",
            Some(("X-API-Key", &raw_key)),
        ),
    )
    .await;
    assert!(replay_response.starts_with("HTTP/1.1 200 OK"));
    assert!(replay_response.contains("day_vote"));

    let guild_response = route_request(
        &state,
        api_request(
            "GET",
            "/api/v1/games/1/replay",
            Some(("X-API-Key", &raw_key)),
        ),
    )
    .await;
    assert!(guild_response.starts_with("HTTP/1.1 200 OK"));
    assert!(guild_response.contains("game-1"));
}

#[tokio::test]
async fn protected_compatible_stats_and_replay_api_match_laravel_contract() {
    let state = test_state();
    let raw_key = {
        let mut store = state.api_keys.write().await;
        issue_api_key(&mut store, 1, 2, "laravel".to_string())
    };
    let mut roles = HashMap::new();
    roles.insert("mafia".to_string(), 2);
    state.stats.write().await.users.insert(
        "11".to_string(),
        stats::PlayerStats {
            name: "Alice".to_string(),
            games: 3,
            wins: 2,
            losses: 1,
            rating: 1120,
            roles,
            ..Default::default()
        },
    );
    state.completed_replays.write().await.push_front(json!({
        "game_key": "game-1",
        "game_id": "game-1",
        "guild_id": 1,
        "channel_id": 10,
        "status": "completed",
        "started_at": "2026-07-08T21:00:00Z",
        "ended_at": "2026-07-08T21:45:00Z",
        "phase": "Ended",
        "phase_key": "Ended",
        "day_number": 3,
        "elapsed_seconds": 123,
        "winner": "Citizen",
        "winner_key": "Citizen",
        "participants": [
            {
                "user_id": 11,
                "name": "Alice",
                "initial_role": "Mafia",
                "initial_role_key": "Mafia",
                "initial_team": "mafia",
                "final_role": "Mafia",
                "final_role_key": "Mafia",
                "final_team": "mafia",
                "alive": true,
                "death_order": null
            },
            {
                "user_id": 12,
                "name": "Bob",
                "initial_role": "Citizen",
                "initial_role_key": "Citizen",
                "initial_team": "citizen",
                "final_role": "Citizen",
                "final_role_key": "Citizen",
                "final_team": "citizen",
                "alive": false,
                "death_order": 1
            }
        ],
        "events": [
            {
                "seq": 0,
                "id": "e_000000",
                "timestamp": "2026-07-08T21:00:00Z",
                "day_number": 0,
                "phase": "Recruiting",
                "phase_key": "Recruiting",
                "kind": "game_started",
                "actor": null,
                "target_user_ids": [],
                "details": {"player_count": 2}
            },
            {
                "seq": 1,
                "id": "e_000001",
                "timestamp": "2026-07-08T21:10:00Z",
                "day_number": 1,
                "phase": "Day",
                "phase_key": "Day",
                "kind": "day_vote",
                "actor": {"user_id": 11, "name": "Alice"},
                "target_user_ids": [12],
                "details": {"choice": "player"}
            },
            {
                "seq": 2,
                "id": "e_000002",
                "timestamp": "2026-07-08T21:12:00Z",
                "day_number": 1,
                "phase": "ConfirmationVote",
                "phase_key": "ConfirmationVote",
                "kind": "confirmation_vote_resolved",
                "actor": null,
                "target_user_ids": [12],
                "details": {
                    "executed_user_id": 12,
                    "approved": true,
                    "vote_counts": [{"approve": true, "count": 2}],
                    "weighted_vote_counts": [{"approve": true, "count": 2}]
                }
            }
        ],
        "rating_log": [],
    }));

    let recent_response = route_request(
        &state,
        api_request(
            "GET",
            "/api/v1/games/recent?limit=5",
            Some(("X-API-Key", &raw_key)),
        ),
    )
    .await;
    assert!(recent_response.starts_with("HTTP/1.1 200 OK"));
    assert!(recent_response.contains(r#""player_count":2"#));

    let recent_alias_response = route_request(
        &state,
        api_request(
            "GET",
            "/games/recent?limit=5",
            Some(("X-API-Key", &raw_key)),
        ),
    )
    .await;
    assert!(recent_alias_response.starts_with("HTTP/1.1 200 OK"));
    assert!(recent_alias_response.contains(r#""player_count":2"#));

    let game_response = route_request(
        &state,
        api_request("GET", "/api/v1/game/game-1", Some(("X-API-Key", &raw_key))),
    )
    .await;
    assert!(game_response.starts_with("HTTP/1.1 200 OK"));
    assert!(game_response.contains(r#""nickname":"Alice""#));

    let result_response = route_request(
        &state,
        api_request(
            "GET",
            "/api/v1/game/game-1/result",
            Some(("X-API-Key", &raw_key)),
        ),
    )
    .await;
    assert!(result_response.starts_with("HTTP/1.1 200 OK"));
    assert!(result_response.contains(r#""cause_of_death":"execution""#));

    let events_response = route_request(
        &state,
        api_request(
            "GET",
            "/api/v1/game/game-1/events",
            Some(("X-API-Key", &raw_key)),
        ),
    )
    .await;
    assert!(events_response.starts_with("HTTP/1.1 200 OK"));
    assert!(events_response.contains(r#""type":"vote""#));
    assert!(events_response.contains(r#""type":"death""#));
    assert!(events_response.contains(r#""role_revealed":"citizen""#));
    assert!(events_response.contains(r#""vote_count":2"#));

    let events_alias_response = route_request(
        &state,
        api_request("GET", "/game/game-1/events", Some(("X-API-Key", &raw_key))),
    )
    .await;
    assert!(events_alias_response.starts_with("HTTP/1.1 200 OK"));
    assert!(events_alias_response.contains(r#""type":"death""#));

    let leaderboard_response = route_request(
        &state,
        api_request(
            "GET",
            "/api/v1/stats/leaderboard?sort=games&limit=5",
            Some(("X-API-Key", &raw_key)),
        ),
    )
    .await;
    assert!(leaderboard_response.starts_with("HTTP/1.1 200 OK"));
    assert!(leaderboard_response.contains(r#""nickname":"Alice""#));

    let user_response = route_request(
        &state,
        api_request(
            "GET",
            "/api/v1/stats/user/11",
            Some(("X-API-Key", &raw_key)),
        ),
    )
    .await;
    assert!(user_response.starts_with("HTTP/1.1 200 OK"));
    assert!(user_response.contains(r#""total_games":3"#));

    let user_games_response = route_request(
        &state,
        api_request(
            "GET",
            "/api/v1/stats/user/11/games?per_page=5",
            Some(("X-API-Key", &raw_key)),
        ),
    )
    .await;
    assert!(user_games_response.starts_with("HTTP/1.1 200 OK"));
    assert!(user_games_response.contains(r#""result":"loss""#));

    let user_games_alias_response = route_request(
        &state,
        api_request(
            "GET",
            "/stats/user/11/games?per_page=5",
            Some(("X-API-Key", &raw_key)),
        ),
    )
    .await;
    assert!(user_games_alias_response.starts_with("HTTP/1.1 200 OK"));
    assert!(user_games_alias_response.contains(r#""result":"loss""#));
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
        spectator_role_id: None,
        role_counts: HashMap::new(),
        special_roles: Vec::new(),
        max_players: 8,
        minimum_players: 2,
        joined_ids: std::collections::HashSet::from([2, 3]),
        joined_names: HashMap::new(),
        spectator_ids: std::collections::HashSet::new(),
        spectator_names: HashMap::new(),
        spectator_role_granted: Default::default(),
        accepting: true,
        cancelled: false,
        auto_start_players: None,
        recruitment_seconds: 60,
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

    assert_eq!(
        headers.get("x-api-key").map(String::as_str),
        Some("key-value")
    );
}

#[test]
fn api_docs_separate_public_and_protected_endpoints() {
    let html = render_api_docs_page("https://mafia.example/");

    assert!(html.contains("공개 조회 API"));
    assert!(html.contains("보호 관리 API"));
    assert!(html.contains("/api/v1/games/recent"));
    assert!(html.contains("/api/v1/game/{game_key}/events"));
    assert!(html.contains("/api/v1/stats/user/{user_id}/games"));
    assert!(html.contains("GET /games/recent"));
    assert!(html.contains("GET /game/{game_key}/events"));
    assert!(html.contains("GET /stats/user/{user_id}/games"));
    assert!(html.contains("/api/v1/games/{guild_id}/actions"));
    assert!(html.contains("/api/v1/games/{guild_id}/replay"));
    assert!(html.contains("/api/v1/replays/{game_key}"));
    assert!(html.contains("https://mafia.example/api/v1/games/123"));
    assert!(!html.contains("example.com"));
    assert!(html.contains("overflow-wrap: anywhere"));
    assert!(html.contains("word-break: break-word"));
    assert!(html.contains("site-shell"));
    assert!(html.contains("응답 코드"));
}

#[test]
fn roles_page_renders_detailed_guides() {
    let html = render_roles_page();

    assert!(html.contains("역할 설명"));
    assert!(html.contains(r#"<a href="/roles">역할 설명</a>"#));
    assert!(html.contains("마피아 비밀방의 처치 선택 현황"));
    assert!(html.contains("최면술사"));
    assert!(html.contains("운영 포인트"));
    assert!(html.contains("주의:"));
    assert!(html.contains("role-help"));
    assert!(html.contains("role-rating"));
    assert!(html.contains("레이팅 요소"));
    assert!(html.contains("role-grid"));
}

/// 티어 페이지는 실제 풀에 있는 모든 능력을 담아야 한다 (코드와 자동 동기화).
#[test]
fn tiers_page_lists_every_pool_ability() {
    let html = render_tiers_page();

    assert!(html.contains("티어 능력 설명"));
    assert!(html.contains(r#"<a href="/tiers">티어 능력</a>"#));
    assert!(html.contains("2티어 40% / 3티어 30% / 4티어 15% / 5티어 10% / 6티어 5%"));
    let mut all = TIER3_ABILITIES.to_vec();
    all.extend_from_slice(TIER4_MAFIA_ABILITIES);
    all.extend(tier4_pool(Role::CultLeader));
    all.extend_from_slice(TIER4_CITIZEN_ABILITIES);
    for role in [
        Role::Spy,
        Role::Fraudster,
        Role::Madam,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Contractor,
        Role::Godfather,
        Role::Villain,
    ] {
        all.extend(tier4_pool(role));
    }
    for ability in all {
        assert!(
            html.contains(&format!("<h3>{}</h3>", ability.value())),
            "{:?} 누락",
            ability
        );
        assert!(
            html.contains(&html_escape(ability.description())),
            "{:?} 설명 누락",
            ability
        );
    }
}

#[test]
fn rating_page_explains_rating_for_players() {
    let html = render_rating_page();

    assert!(html.contains("레이팅 설명"));
    assert!(html.contains(r#"<a href="/rating">레이팅 설명</a>"#));
    assert!(html.contains("초기 레이팅"));
    assert!(html.contains("티어 강등 보호"));
    assert!(html.contains("승리 기본"));
    assert!(html.contains("랭크표"));
    assert!(html.contains(">X<"));
    assert!(html.contains("/랭크컷"));
    assert!(html.contains("자주 묻는 질문"));
    assert!(html.contains("졌는데 왜 점수가 안 깎였나요?"));
}

#[tokio::test]
async fn stalled_request_read_is_dropped_after_timeout() {
    // 연결만 열고 요청을 보내지 않는 클라이언트가 태스크를 영원히 붙잡으면 안 된다.
    let (mut client, server) = tokio::io::duplex(1024);

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        handle_connection(server, test_state(), Duration::from_millis(50)),
    )
    .await
    .expect("stalled request read should time out");

    assert!(result.is_ok());
    let mut received = Vec::new();
    client.read_to_end(&mut received).await.unwrap();
    assert!(received.is_empty(), "stalled connection gets no response");
}

#[tokio::test]
async fn stalled_response_write_is_dropped_after_timeout() {
    // 요청은 보내고 응답을 읽지 않는 클라이언트도 쓰기 제한 시간 뒤에 끊는다.
    let (mut client, server) = tokio::io::duplex(64);
    client
        .write_all(b"GET /api/status HTTP/1.1\r\n\r\n")
        .await
        .unwrap();

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        handle_connection(server, test_state(), Duration::from_millis(50)),
    )
    .await
    .expect("stalled response write should time out");

    assert!(result.is_ok());
}

#[tokio::test]
async fn full_connection_slots_drop_new_connections() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let slots = Arc::new(Semaphore::new(1));

    let _first_client = TcpStream::connect(addr).await.unwrap();
    let (_first, slot) = accept_connection(&listener, &slots)
        .await
        .expect("first connection gets a slot");

    let mut second_client = TcpStream::connect(addr).await.unwrap();
    assert!(accept_connection(&listener, &slots).await.is_none());
    // 슬롯이 없어 거절된 연결은 서버가 바로 닫는다.
    let mut buf = [0u8; 1];
    let read = tokio::time::timeout(Duration::from_secs(5), second_client.read(&mut buf))
        .await
        .expect("rejected connection should be closed");
    assert!(matches!(read, Ok(0) | Err(_)));

    // 슬롯이 반납되면 다시 받는다.
    drop(slot);
    let _third_client = TcpStream::connect(addr).await.unwrap();
    assert!(accept_connection(&listener, &slots).await.is_some());
}

#[tokio::test]
async fn oversized_content_length_is_rejected_without_panicking() {
    // Content-Length가 usize::MAX여도 넘침으로 슬라이스가 뒤집히지 않고 거부된다.
    let mut request: &[u8] =
        b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 18446744073709551615\r\n\r\nabc";
    assert!(read_http_request(&mut request).await.is_err());
    let mut request: &[u8] = b"POST / HTTP/1.1\r\nContent-Length: 200000\r\n\r\nabc";
    assert!(read_http_request(&mut request).await.is_err());
    // 짧게 끊긴 본문은 받은 만큼만 읽는다.
    let mut request: &[u8] = b"POST /x HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc";
    let parsed = read_http_request(&mut request).await.unwrap();
    assert_eq!((parsed.path.as_str(), parsed.body.as_str()), ("/x", "abc"));
}

#[test]
fn economy_percent_fields_round_trip_through_the_form() {
    assert_eq!(parse_percent_bp("2.5"), Some(250));
    assert_eq!(parse_percent_bp("0.2"), Some(20));
    assert_eq!(parse_percent_bp(".05"), Some(5));
    assert_eq!(parse_percent_bp("100"), Some(10_000));
    for bad in ["", "-1", "100.01", "1.234", "abc", "1000", "."] {
        assert_eq!(parse_percent_bp(bad), None, "{bad}");
    }
    assert_eq!(percent_text(250), "2.5");
    assert_eq!(percent_text(20), "0.2");
    assert_eq!(percent_text(5), "0.05");
    assert_eq!(percent_text(5_000), "50");

    let mut config = test_config();
    let mut updates = updates_for(&config);
    // 화면에서 읽은 값을 그대로 저장하면 설정이 바뀌지 않는다.
    apply_updates(&mut config, &updates).unwrap();
    assert_eq!(config.holdem_rake_bp, 250);
    assert_eq!(config.daily_rolling_bp, 20);
    updates.insert("holdem_rake_bp".to_string(), "3.25".to_string());
    updates.insert("relief_amount".to_string(), "7000".to_string());
    apply_updates(&mut config, &updates).unwrap();
    assert_eq!(config.holdem_rake_bp, 325);
    assert_eq!(config.relief_amount, 7_000);
    assert_eq!(config.economy_rules().holdem_rake_bp, 325);
}

#[test]
fn economy_form_rejects_bad_percentages_and_jackpot_shares_over_100() {
    let config = test_config();
    let body = form_body_for(&config).replace("gift_fee_bp=3", "gift_fee_bp=150");
    assert!(parse_form_updates(&body).is_err());

    let mut config = test_config();
    let mut updates = updates_for(&config);
    updates.insert("jackpot_loser_bp".to_string(), "80".to_string());
    updates.insert("jackpot_winner_bp".to_string(), "30".to_string());
    let error = apply_updates(&mut config, &updates).unwrap_err();
    assert!(error.contains("100%"), "{error}");
    assert_eq!(config.jackpot_loser_bp, 5_000, "실패하면 원래 설정 그대로");
}

#[test]
fn only_certificate_rejections_are_worth_logging() {
    use std::io::{Error, ErrorKind};
    let tls = |error: rustls::Error| Error::new(ErrorKind::InvalidData, error);
    // 브라우저·Cloudflare가 인증서를 거부하면 남긴다.
    assert!(is_certificate_rejection(&tls(
        rustls::Error::AlertReceived(rustls::AlertDescription::UnknownCA)
    )));
    assert!(is_certificate_rejection(&tls(
        rustls::Error::AlertReceived(rustls::AlertDescription::CertificateExpired)
    )));
    // 포트 스캐너가 내는 실패는 남기지 않는다.
    assert!(!is_certificate_rejection(&tls(
        rustls::Error::PeerIncompatible(rustls::PeerIncompatible::NoCipherSuitesInCommon)
    )));
    assert!(!is_certificate_rejection(&tls(
        rustls::Error::InvalidMessage(rustls::InvalidMessage::InvalidContentType)
    )));
    assert!(!is_certificate_rejection(&Error::from(
        ErrorKind::ConnectionReset
    )));
    assert!(!is_certificate_rejection(&Error::new(
        ErrorKind::UnexpectedEof,
        "tls handshake eof"
    )));
}

#[test]
fn clients_hanging_up_are_not_server_errors() {
    use std::io::{Error, ErrorKind};
    assert!(is_peer_disconnect(&anyhow::Error::from(Error::from(
        ErrorKind::ConnectionReset
    ))));
    assert!(is_peer_disconnect(&anyhow::Error::from(Error::from(
        ErrorKind::BrokenPipe
    ))));
    assert!(is_peer_disconnect(
        &anyhow::Error::from(Error::from(ErrorKind::ConnectionReset)).context("응답 쓰기")
    ));
    assert!(!is_peer_disconnect(&anyhow::anyhow!(
        "설정 파일을 쓰지 못했습니다"
    )));
    assert!(!is_peer_disconnect(&anyhow::Error::from(Error::from(
        ErrorKind::PermissionDenied
    ))));
}
