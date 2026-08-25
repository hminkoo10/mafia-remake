// runner 테스트 모듈 (src/runner.rs에서 분리)

use super::*;
use crate::channel::tests::dead_chat_test_running;

/// 익명 게임 진행 중과 같은 상태: `game.players`의 이름이 별명으로 덮여 있고
/// 실명은 `anonymous_original_names`에만 남아 있다. 참가자 순서는 무작위이므로
/// 별명은 user_id 순으로 매긴다.
fn anonymous_result_test_running() -> RunningGame {
    let mut running = dead_chat_test_running();
    running.anonymous_enabled = true;
    let mut user_ids = running
        .game
        .players
        .iter()
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    user_ids.sort_unstable();
    for (index, user_id) in user_ids.into_iter().enumerate() {
        running
            .anonymous_aliases
            .insert(user_id, format!("{}번", index + 1));
    }
    for player in &mut running.game.players {
        running
            .anonymous_original_names
            .insert(player.user_id, player.name.clone());
        player
            .name
            .clone_from(&running.anonymous_aliases[&player.user_id]);
    }
    running
}

#[test]
fn anonymous_game_result_names_reveal_the_player_behind_each_alias() {
    let running = anonymous_result_test_running();
    let player = running
        .game
        .players
        .iter()
        .find(|player| player.user_id == 3)
        .unwrap();

    assert_eq!(player.name, "3번");
    assert_eq!(game_result_display_name(&running, player), "3번 = p3");
}

#[test]
fn stats_snapshot_records_real_names_for_anonymous_games() {
    let running = anonymous_result_test_running();

    let snapshot = stats_game_snapshot(&running);

    let mut names = snapshot
        .players
        .iter()
        .map(|player| player.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["p1", "p2", "p3", "p4"]);
}

#[test]
fn anonymous_rank_change_lines_show_alias_and_real_name() {
    let running = anonymous_result_test_running();
    let log = vec![stats::GameRatingLogItem {
        user_id: 2,
        name: "p2".to_string(),
        role: Role::Citizen.value().to_string(),
        before: 1000,
        after: 1030,
        before_rank: "실버".to_string(),
        after_rank: "실버".to_string(),
        delta: 30,
        team_delta: 30,
        role_delta: 0,
        streak_delta: 0,
        win_streak: 1,
        best_win_streak: 1,
        reasons: Vec::new(),
    }];

    let labeled = rating_log_with_result_labels(&running, &log);

    assert_eq!(labeled[0].name, "2번 = p2");
}

#[test]
fn non_anonymous_game_result_names_are_left_alone() {
    let mut running = anonymous_result_test_running();
    running.anonymous_enabled = false;
    let player = running.game.players[0].clone();
    let alias = player.name.clone();

    assert_eq!(game_result_display_name(&running, &player), alias);
    assert_eq!(stats_game_snapshot(&running).players[0].name, alias);
}

#[test]
fn stale_game_loop_cannot_remove_new_game_entry() {
    let games = DashMap::new();
    let stale = Arc::new(());
    let current = Arc::new(());
    games.insert(1_u64, current.clone());

    assert!(!remove_current_entry(&games, 1, &stale));
    assert!(Arc::ptr_eq(games.get(&1).unwrap().value(), &current));
    assert!(remove_current_entry(&games, 1, &current));
    assert!(games.is_empty());
}

#[test]
fn night_wait_does_not_restart_after_delayed_anonymous_broadcast() {
    let started_at = Instant::now();
    let deadline = started_at + Duration::from_secs(30);

    assert_eq!(
        remaining_night_wait(deadline, started_at + Duration::from_secs(20)),
        Duration::from_secs(10)
    );
    assert_eq!(
        remaining_night_wait(deadline, started_at + Duration::from_secs(35)),
        Duration::ZERO
    );
}

#[test]
fn confirmation_summary_uses_submitted_vote_threshold() {
    let result = ConfirmVoteResult {
        vote_counts: HashMap::from([(true, 3), (false, 2)]),
        ..Default::default()
    };
    let context = ConfirmationVoteContext {
        eligible_voters: 7,
        submitted_voters: 5,
    };
    assert_eq!(
        confirmation_vote_summary(&result, context),
        "찬성 3표 / 반대 2표 / 미투표 2명\n처형 기준: 찬성 3표 이상 (투표수 5표 기준)"
    );
}

#[test]
fn terrorist_execution_message_matches_public_format() {
    let terrorist = Player::new(1, "구현민", Role::Terrorist);
    let target = Player::new(2, "설재경", Role::Mafia);

    assert_eq!(
        terrorist_execution_message(&terrorist, &target),
        "[테러리스트 구현민님이 설재경님을 습격하였습니다.]"
    );
}

#[test]
fn mercenary_contract_message_hides_client_identity() {
    assert_eq!(
        mercenary_contract_received_message(),
        "누군가로부터 의뢰를 받았습니다."
    );
}

#[test]
fn mercenary_role_message_does_not_reveal_client_name() {
    let mut game = MafiaGame::new(
        vec![
            (1, "마피아".to_string()),
            (2, "용병".to_string()),
            (3, "비밀의뢰인".to_string()),
        ],
        1,
        0,
        0,
        Vec::new(),
    )
    .unwrap();
    game.get_player_mut(2).unwrap().role = Role::Mercenary;
    game.get_player_mut(3).unwrap().role = Role::Citizen;
    game.mercenary_client_ids.clear();
    game.mercenary_client_ids.insert(2, 3);

    let message = role_message(&game, game.get_player(2).unwrap());

    assert!(!message.contains("비밀의뢰인"));
    assert!(!message.contains("의뢰인:"));
}

#[test]
fn scientist_role_message_names_mafia_team_before_first_death() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Scientist".to_string()),
            (3, "Three".to_string()),
        ],
        1,
        0,
        0,
        Vec::new(),
    )
    .unwrap();
    game.get_player_mut(2).unwrap().role = Role::Scientist;

    let message = role_message(&game, game.get_player(2).unwrap());

    assert!(message.contains("진영: **마피아팀**"));
    assert!(!message.contains("진영: **시민팀**"));
}

#[test]
fn confirmation_summary_requires_strict_majority_for_even_votes() {
    let result = ConfirmVoteResult {
        vote_counts: HashMap::from([(true, 4), (false, 4)]),
        ..Default::default()
    };
    let context = ConfirmationVoteContext {
        eligible_voters: 9,
        submitted_voters: 8,
    };
    assert_eq!(
        confirmation_vote_summary(&result, context),
        "찬성 4표 / 반대 4표 / 미투표 1명\n처형 기준: 찬성 5표 이상 (투표수 8표 기준)"
    );
    assert_eq!(
        confirmation_rejection_message(&result, context),
        "찬성과 반대가 같아 처형하지 않습니다."
    );
}

#[test]
fn confirmation_summary_can_hide_vote_counts() {
    let result = ConfirmVoteResult {
        vote_counts: HashMap::from([(true, 3), (false, 2)]),
        ..Default::default()
    };
    let context = ConfirmationVoteContext {
        eligible_voters: 7,
        submitted_voters: 5,
    };

    assert_eq!(
        confirmation_vote_summary_section(&result, context, false),
        ""
    );
}

#[test]
fn confirmation_summary_displays_raw_counts_with_weighted_threshold() {
    let result = ConfirmVoteResult {
        vote_counts: HashMap::from([(true, 1), (false, 1)]),
        weighted_vote_counts: HashMap::from([(true, 2), (false, 1)]),
        ..Default::default()
    };
    let context = ConfirmationVoteContext {
        eligible_voters: 2,
        submitted_voters: 2,
    };

    assert_eq!(
        confirmation_vote_summary(&result, context),
        "찬성 1표 / 반대 1표 / 미투표 0명\n처형 기준: 찬성 처리값 2 이상 (처리 투표수 3 기준)"
    );
}

#[test]
fn confirmation_rejection_message_reports_no_leading() {
    let result = ConfirmVoteResult {
        vote_counts: HashMap::from([(true, 2), (false, 3)]),
        ..Default::default()
    };
    let context = ConfirmationVoteContext {
        eligible_voters: 5,
        submitted_voters: 5,
    };

    assert_eq!(
        confirmation_rejection_message(&result, context),
        "반대가 많아 처형하지 않습니다."
    );
}

#[test]
fn prophet_victory_message_names_the_prophet() {
    let players = (1..=5)
        .map(|user_id| (user_id, format!("P{user_id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, Vec::new()).unwrap();
    let prophet = game.get_player_mut(2).unwrap();
    prophet.role = Role::Prophet;
    prophet.name = "설재경".to_string();
    game.phase = Phase::Day;
    game.day_number = 4;

    assert_eq!(
        prophet_victory_message(&game, Winner::Citizen).as_deref(),
        Some("예언자 설재경님의 힘으로 시민팀이 승리하였습니다!")
    );
    assert_eq!(prophet_victory_message(&game, Winner::Mafia), None);
}

#[test]
fn contractor_components_stay_within_discord_limits() {
    let targets = (0..30)
        .map(|index| Player::new(1000 + index, format!("대상{index}"), Role::Citizen))
        .collect::<Vec<_>>();
    let components = contractor_contract_components(
        serenity::GuildId::new(1),
        42,
        &targets,
        &ContractorContractDraft::default(),
    );
    let json = serde_json::to_value(&components).unwrap();
    let rows = json.as_array().unwrap();

    // 대상 셀렉트 2개 + 대상별 직업 셀렉트 2개 + 버튼 1줄 = Discord 상한인 5줄.
    assert_eq!(rows.len(), 5);
    let mut select_ids = Vec::new();
    for row in rows {
        for component in row["components"].as_array().unwrap() {
            if let Some(options) = component.get("options").and_then(|value| value.as_array()) {
                assert!(!options.is_empty());
                assert!(options.len() <= 25);
                select_ids.push(component["custom_id"].as_str().unwrap().to_string());
            }
        }
    }
    assert_eq!(
        select_ids,
        [
            "contractor_target:1:42:0",
            "contractor_target:1:42:1",
            "contractor_role:1:42:0",
            "contractor_role:1:42:1",
        ]
    );
}

/// 두 직업 목록 모두 셀렉트 상한 안에 들어와야 한다. 하나라도 넘으면 Discord가
/// 메시지를 거부해 청부 화면 자체가 뜨지 않는다.
#[test]
fn every_contractor_role_group_fits_one_select() {
    for group in [
        ContractorGuessRoleGroup::Citizen,
        ContractorGuessRoleGroup::MafiaCultNeutral,
    ] {
        let count = contractor_guessable_roles_for_group(group).count();
        assert!(count > 0 && count <= 25, "{group:?} has {count} roles");
    }
}

/// 게임 결과에는 티어와 능력이 공개된다.
#[test]
fn game_result_tier_text_shows_tier_and_ability() {
    let mut running = dead_chat_test_running();
    let first = running.game.players[0].user_id;
    let second = running.game.players[1].user_id;
    running.game.player_tiers.insert(first, 4);
    running.game.player_tiers.insert(second, 2);
    running
        .game
        .tier_abilities
        .insert(first, vec![mafia_remake::model::TierAbility::Cleanup]);

    assert_eq!(game_result_tier_text(&running.game, first), "4티어 [수습]");
    assert_eq!(game_result_tier_text(&running.game, second), "2티어");
    // 배정 기록이 없으면 기본 2티어로 표기한다.
    assert_eq!(game_result_tier_text(&running.game, 999_999), "2티어");
}

#[test]
fn game_result_image_renders_png() {
    let rows = vec![
        GameResultImageRow {
            name: "Long Reason".to_string(),
            role: Role::Doctor.value().to_string(),
            team: "시민팀".to_string(),
            tier_text: "3티어 [가호]".to_string(),
            alive: true,
            before: Some(1043),
            after: Some(1077),
            before_rank: Some("실버".to_string()),
            after_rank: Some("골드".to_string()),
            delta: Some(34),
            team_delta: Some(29),
            role_delta: Some(1),
            streak_delta: Some(4),
            win_streak: Some(3),
            best_win_streak: Some(7),
            reasons: vec![
                "소속 진영 승리, 의사 보호 운영 기여 +1, 3연승 보너스 +4, 레이팅 구간 보정 x1.20"
                    .to_string(),
            ],
        },
        GameResultImageRow {
            name: "Alpha".to_string(),
            role: Role::Mafia.value().to_string(),
            team: "마피아팀".to_string(),
            tier_text: "3티어 [가호]".to_string(),
            alive: true,
            before: Some(1000),
            after: Some(1032),
            before_rank: Some("실버".to_string()),
            after_rank: Some("골드".to_string()),
            delta: Some(32),
            team_delta: Some(24),
            role_delta: Some(4),
            streak_delta: Some(4),
            win_streak: Some(2),
            best_win_streak: Some(2),
            reasons: vec!["소속 진영 승리".to_string()],
        },
        GameResultImageRow {
            name: "Beta".to_string(),
            role: Role::Doctor.value().to_string(),
            team: "시민팀".to_string(),
            tier_text: "3티어 [가호]".to_string(),
            alive: false,
            before: Some(1000),
            after: Some(982),
            before_rank: Some("실버".to_string()),
            after_rank: Some("골드".to_string()),
            delta: Some(-18),
            team_delta: Some(-20),
            role_delta: Some(2),
            streak_delta: Some(0),
            win_streak: Some(0),
            best_win_streak: Some(4),
            reasons: vec!["패배".to_string()],
        },
    ];

    assert_eq!(game_result_delta_detail(&rows[0]), "팀 +29 · 직업 +1");

    let image = render_game_result_image(Winner::Mafia, 310, rows).unwrap();

    assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(image.len() > 1024);
    let decoded = image::load_from_memory(&image).unwrap().to_rgb8();
    assert_eq!(decoded.width(), 2240);
    assert!(decoded.height() > 520);
    assert_eq!(*decoded.get_pixel(1222, 266), image_color("#dcfce7"));
}
