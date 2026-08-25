// stats 테스트 모듈 (src/stats.rs에서 분리)

use super::*;

fn rating_test_game() -> MafiaGame {
    MafiaGame::new(
        vec![
            (1, "Alpha".to_string()),
            (2, "Beta".to_string()),
            (3, "Gamma".to_string()),
            (4, "Delta".to_string()),
        ],
        1,
        1,
        1,
        Vec::new(),
    )
    .unwrap()
}

fn initial_roles(game: &MafiaGame) -> HashMap<u64, Role> {
    game.players
        .iter()
        .map(|player| (player.user_id, player.role))
        .collect()
}

#[test]
fn win_rate_handles_zero_games() {
    assert_eq!(win_rate_text(0, 0), "0.0%");
    assert_eq!(win_rate_text(3, 4), "75.0%");
}

#[test]
fn old_stats_without_role_selection_history_still_loads() {
    let stats: StatsFile = serde_json::from_str(r#"{"users":{}}"#).unwrap();

    assert!(stats.role_selection_history.is_empty());
}

#[test]
fn role_balance_falls_back_to_lifetime_counts_without_history() {
    let mut stats = StatsFile::default();
    stats.users.insert(
        "1".to_string(),
        PlayerStats {
            roles: HashMap::from([(Role::Shaman.value().to_string(), 3)]),
            ..Default::default()
        },
    );

    let counts = role_appearance_counts(&stats);

    assert_eq!(counts.get(&Role::Shaman).copied(), Some(3));
}

#[test]
fn started_role_history_overrides_old_lifetime_counts() {
    let mut stats = StatsFile::default();
    stats.users.insert(
        "1".to_string(),
        PlayerStats {
            roles: HashMap::from([(Role::Shaman.value().to_string(), 100)]),
            ..Default::default()
        },
    );

    record_role_selection(&mut stats, [Role::Mafia, Role::Detective, Role::Detective]);
    let counts = role_appearance_counts(&stats);

    assert_eq!(
        stats.role_selection_history[0].roles,
        vec![
            Role::Mafia.value().to_string(),
            Role::Detective.value().to_string()
        ]
    );
    assert_eq!(counts.get(&Role::Detective).copied(), Some(64));
    assert!(!counts.contains_key(&Role::Shaman));
}

#[test]
fn role_selection_history_is_bounded() {
    let mut stats = StatsFile::default();
    for _ in 0..(ROLE_BALANCE_RECENT_GAMES + 5) {
        record_role_selection(&mut stats, [Role::Detective]);
    }

    assert_eq!(
        stats.role_selection_history.len(),
        ROLE_BALANCE_RECENT_GAMES
    );
}

#[test]
fn role_balance_penalizes_recent_appearances_more() {
    let history = |role: Role, ended_at: &str| RatingHistoryItem {
        ended_at: ended_at.to_string(),
        before: 1000,
        after: 1000,
        delta: 0,
        team_delta: 0,
        role_delta: 0,
        streak_delta: 0,
        role: role.value().to_string(),
        team: "citizen".to_string(),
        winner: Winner::Citizen.value().to_string(),
        players: 8,
        rating_reasons: Vec::new(),
    };
    let mut stats = StatsFile::default();
    stats.users.insert(
        "1".to_string(),
        PlayerStats {
            rating_history: vec![history(Role::Detective, "2026-01-01T00:00:00+09:00")],
            ..Default::default()
        },
    );
    stats.users.insert(
        "2".to_string(),
        PlayerStats {
            rating_history: vec![history(Role::Shaman, "2026-01-02T00:00:00+09:00")],
            ..Default::default()
        },
    );

    let scores = role_appearance_counts(&stats);

    assert!(scores[&Role::Shaman] > scores[&Role::Detective]);
}

#[test]
fn assignment_history_counts_special_mafia_roles() {
    let mut stats = StatsFile::default();
    stats.users.insert(
        "7".to_string(),
        PlayerStats {
            games: 6,
            roles: HashMap::from([
                (Role::Mafia.value().to_string(), 1),
                (Role::Spy.value().to_string(), 2),
                (Role::Citizen.value().to_string(), 3),
            ]),
            rating_history: vec![RatingHistoryItem {
                ended_at: "2026-01-02T00:00:00+09:00".to_string(),
                before: 1000,
                after: 1000,
                delta: 0,
                team_delta: 0,
                role_delta: 0,
                streak_delta: 0,
                role: Role::Spy.value().to_string(),
                team: "citizen".to_string(),
                winner: Winner::Citizen.value().to_string(),
                players: 8,
                rating_reasons: Vec::new(),
            }],
            ..Default::default()
        },
    );

    let histories = player_assignment_histories(&stats, &[7]);
    let history = &histories[&7];

    assert_eq!(history.games, 6);
    assert_eq!(history.mafia_role_games, 3);
    assert_eq!(history.recent_roles, vec![Role::Spy]);
}

/// 중지된 게임의 배정도 리센시에 최신 기록으로 반영된다. 반영이 없으면
/// 중지 직후 다음 판이 같은 이력을 보고 같은 팀을 거의 그대로 재현한다.
#[test]
fn aborted_assignments_count_toward_recent_roles_without_touching_records() {
    let mut stats = StatsFile::default();
    stats.users.insert(
        "7".to_string(),
        PlayerStats {
            games: 3,
            rating_history: vec![RatingHistoryItem {
                ended_at: "2026-01-02T00:00:00+09:00".to_string(),
                before: 1000,
                after: 1000,
                delta: 0,
                team_delta: 0,
                role_delta: 0,
                streak_delta: 0,
                role: Role::Citizen.value().to_string(),
                team: "citizen".to_string(),
                winner: Winner::Citizen.value().to_string(),
                players: 8,
                rating_reasons: Vec::new(),
            }],
            ..Default::default()
        },
    );

    record_aborted_assignments(&mut stats, [(7, "Seven".to_string(), Role::Mafia)]);

    let entry = &stats.users["7"];
    // 승패·게임 수·역할 횟수는 그대로다.
    assert_eq!(entry.games, 3);
    assert_eq!(entry.wins, 0);
    assert_eq!(entry.losses, 0);
    assert!(entry.roles.is_empty());
    assert_eq!(entry.rating_history.len(), 1);

    let histories = player_assignment_histories(&stats, &[7]);
    // 중지된 판(마피아)이 가장 최근 기록으로 잡힌다 (Local::now가 과거
    // 하드코딩 날짜보다 뒤라 정렬상 앞에 온다).
    assert_eq!(histories[&7].recent_roles, vec![Role::Mafia, Role::Citizen]);
}

/// 유동 티어: 커트라인은 배치를 마친 플레이어들의 분포에서 나온다.
#[test]
fn rating_rank_is_relative_to_the_player_pool() {
    let mut stats = StatsFile::default();
    for (index, rating) in [900i64, 950, 1000, 1050, 1100, 1150, 1200, 1250, 1300, 1400]
        .into_iter()
        .enumerate()
    {
        let entry = ensure_player_stats(&mut stats, index as u64 + 1, "p");
        entry.rating = rating;
        entry.rating_games = PLACEMENT_GAMES;
    }

    // 10명 풀: 1등 X, 2등 SS, 꼴찌 C.
    assert_eq!(rating_rank(&stats, 1400, PLACEMENT_GAMES), "X");
    assert_eq!(rating_rank(&stats, 1300, PLACEMENT_GAMES), "SS");
    assert_eq!(rating_rank(&stats, 900, PLACEMENT_GAMES), "C");
    // 배치가 끝나지 않으면 랭크가 없다.
    assert_eq!(rating_rank(&stats, 1400, PLACEMENT_GAMES - 1), "배치");

    // 커트라인은 현재 분포에서 나온다.
    let cutoffs = rank_cutoffs(&stats).unwrap();
    assert_eq!(cutoffs[0], ("X", 1400));
    assert_eq!(cutoffs[1], ("SS", 1250));

    // 같은 점수라도 풀이 강해지면 랭크가 내려간다 (유동 커트라인).
    for index in 0..10u64 {
        let entry = ensure_player_stats(&mut stats, 100 + index, "q");
        entry.rating = 1500;
        entry.rating_games = PLACEMENT_GAMES;
    }
    assert_ne!(rating_rank(&stats, 1300, PLACEMENT_GAMES), "SS");
}

#[test]
fn rank_change_log_only_lists_rank_crossings() {
    let logs = vec![
        GameRatingLogItem {
            user_id: 1,
            name: "Alpha".to_string(),
            role: Role::Doctor.value().to_string(),
            before: 1180,
            after: 1210,
            before_rank: "실버".to_string(),
            after_rank: "골드".to_string(),
            delta: 20,
            team_delta: 15,
            role_delta: 5,
            streak_delta: 0,
            win_streak: 1,
            best_win_streak: 3,
            reasons: vec![],
        },
        GameRatingLogItem {
            user_id: 2,
            name: "Beta".to_string(),
            role: Role::Mafia.value().to_string(),
            before: 1000,
            after: 1030,
            before_rank: "실버".to_string(),
            after_rank: "실버".to_string(),
            delta: 30,
            team_delta: 29,
            role_delta: 1,
            streak_delta: 0,
            win_streak: 2,
            best_win_streak: 2,
            reasons: vec![],
        },
    ];

    let chunks = game_rank_change_chunks(&logs, 3500);

    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].contains("Alpha"));
    assert!(chunks[0].contains("실버 -> 골드"));
    assert!(!chunks[0].contains("Beta"));
}

/// v2: 승리는 최소 +5, 패배는 최대 -20이며 활약이 커도 0을 넘지 못한다.
#[test]
fn rating_v2_keeps_wins_positive_and_losses_bounded() {
    assert_eq!(WIN_DELTA_MIN, 5);
    assert_eq!(LOSS_DELTA_MIN, -20);
    assert!(WIN_BASE_DELTA >= 2.0 * -LOSS_BASE_DELTA);
}

#[test]
fn leaderboard_sorts_by_rating() {
    let mut stats = StatsFile::default();
    stats.users.insert(
        "1".to_string(),
        PlayerStats {
            name: "Alpha".to_string(),
            games: 3,
            wins: 1,
            losses: 2,
            rating: 980,
            ..Default::default()
        },
    );
    stats.users.insert(
        "2".to_string(),
        PlayerStats {
            name: "Beta".to_string(),
            games: 2,
            wins: 2,
            losses: 0,
            rating: 1120,
            ..Default::default()
        },
    );

    let text = leaderboard_text(&stats, "rating");
    assert!(text.starts_with("기준: **레이팅**\n1. **Beta**"));
    assert!(text.contains("2. **Alpha**"));
}

#[test]
fn win_streak_updates_and_sorts() {
    let game = rating_test_game();
    let roles = initial_roles(&game);
    let citizen_id = game
        .players
        .iter()
        .find(|player| game.is_citizen_team(player))
        .map(|player| player.user_id)
        .unwrap();
    let mut stats = StatsFile::default();

    record_game_stats(&mut stats, &game, &roles, 120, Winner::Citizen);
    record_game_stats(&mut stats, &game, &roles, 120, Winner::Citizen);

    let entry = stats.users.get(&citizen_id.to_string()).unwrap();
    assert_eq!(entry.win_streak, 2);
    assert_eq!(entry.best_win_streak, 2);

    record_game_stats(&mut stats, &game, &roles, 120, Winner::Mafia);

    let entry = stats.users.get(&citizen_id.to_string()).unwrap();
    assert_eq!(entry.win_streak, 0);
    assert_eq!(entry.best_win_streak, 2);

    let mut ranking = StatsFile::default();
    ranking.users.insert(
        "1".to_string(),
        PlayerStats {
            name: "Alpha".to_string(),
            games: 5,
            wins: 4,
            win_streak: 1,
            best_win_streak: 4,
            ..Default::default()
        },
    );
    ranking.users.insert(
        "2".to_string(),
        PlayerStats {
            name: "Beta".to_string(),
            games: 4,
            wins: 3,
            win_streak: 3,
            best_win_streak: 3,
            ..Default::default()
        },
    );

    let entries = leaderboard_entries(&ranking, "streak", 10);
    assert_eq!(entries[0].0, "2");
    assert_eq!(leaderboard_metric_name("streak"), "연승");
}

#[test]
fn win_streak_bonus_increases_rating_gain() {
    let game = rating_test_game();
    let roles = initial_roles(&game);
    let citizen = game
        .players
        .iter()
        .find(|player| game.is_citizen_team(player))
        .unwrap();

    let mut baseline = StatsFile::default();
    baseline.users.insert(
        citizen.user_id.to_string(),
        PlayerStats {
            name: citizen.name.clone(),
            games: 4,
            wins: 3,
            losses: 1,
            win_streak: 0,
            best_win_streak: 3,
            rating_games: 4,
            ..Default::default()
        },
    );
    let baseline_log = record_game_stats(&mut baseline, &game, &roles, 120, Winner::Citizen);
    let baseline_item = baseline_log
        .iter()
        .find(|item| item.name == citizen.name)
        .unwrap();

    let mut streaking = StatsFile::default();
    streaking.users.insert(
        citizen.user_id.to_string(),
        PlayerStats {
            name: citizen.name.clone(),
            games: 4,
            wins: 4,
            losses: 0,
            win_streak: 4,
            best_win_streak: 4,
            rating_games: 4,
            ..Default::default()
        },
    );
    let streak_log = record_game_stats(&mut streaking, &game, &roles, 120, Winner::Citizen);
    let streak_item = streak_log
        .iter()
        .find(|item| item.name == citizen.name)
        .unwrap();

    assert!(streak_item.delta > baseline_item.delta);
    assert!(streak_item.streak_delta > baseline_item.streak_delta);
    assert_eq!(streak_item.win_streak, 5);
    assert_eq!(streak_item.best_win_streak, 5);
    assert!(
        streak_item
            .reasons
            .iter()
            .any(|reason| reason.contains("연승 보너스"))
    );
}

#[test]
fn play_duration_formats_short_and_long_values() {
    assert_eq!(play_duration_text(12), "1분 미만");
    assert_eq!(play_duration_text(72), "1분");
    assert_eq!(play_duration_text(3700), "61분");
}

#[test]
fn successful_role_event_is_recorded_in_rating_history() {
    let mut game = rating_test_game();
    let doctor = game
        .players
        .iter()
        .find(|player| player.role == Role::Doctor)
        .cloned()
        .unwrap();
    game.record_rating_event(doctor.user_id, 5, "마피아 공격 치료 성공");
    let mut stats = StatsFile::default();

    record_game_stats(
        &mut stats,
        &game,
        &initial_roles(&game),
        120,
        Winner::Citizen,
    );

    let history = stats
        .users
        .get(&doctor.user_id.to_string())
        .unwrap()
        .rating_history
        .last()
        .unwrap();
    assert!(history.role_delta >= 5);
    assert!(
        history
            .rating_reasons
            .iter()
            .any(|reason| reason.contains("치료 성공"))
    );
}

#[test]
fn uncontacted_scientist_ratings_follow_mafia_team() {
    let mut game = rating_test_game();
    let scientist_id = game.players[0].user_id;
    game.get_player_mut(scientist_id).unwrap().role = Role::Scientist;
    game.scientist_contacted.remove(&scientist_id);
    let roles = initial_roles(&game);
    let mut stats = StatsFile::default();

    record_game_stats(&mut stats, &game, &roles, 120, Winner::Mafia);
    let entry = stats.users.get(&scientist_id.to_string()).unwrap();
    assert_eq!(entry.wins, 1);
    assert_eq!(entry.losses, 0);
    assert_eq!(entry.mafia_team_games, 1);
    assert_eq!(entry.rating_history.last().unwrap().team, "mafia");
}

#[test]
fn role_rating_adjustment_is_capped() {
    let mut game = rating_test_game();
    let doctor = game
        .players
        .iter()
        .find(|player| player.role == Role::Doctor)
        .cloned()
        .unwrap();
    game.record_rating_event(doctor.user_id, 9, "첫 번째 기여");
    game.record_rating_event(doctor.user_id, 8, "두 번째 기여");

    let (role_delta, reasons) = role_rating_adjustment(&game, &doctor, Role::Doctor, true);

    assert_eq!(role_delta, ROLE_DELTA_CAP);
    assert!(reasons.iter().any(|reason| reason == "직업 보정 상한 적용"));
}

#[test]
fn inactive_surviving_role_receives_small_penalty() {
    let mut game = rating_test_game();
    game.day_number = 2;
    let doctor = game
        .players
        .iter()
        .find(|player| player.role == Role::Doctor)
        .cloned()
        .unwrap();

    let (role_delta, reasons) = role_rating_adjustment(&game, &doctor, Role::Doctor, false);

    assert_eq!(role_delta, -2);
    assert!(reasons.iter().any(|reason| reason.contains("미사용")));
}

#[test]
fn every_role_has_role_specific_rating_element() {
    let game = rating_test_game();
    let player = game.players.first().unwrap().clone();
    let roles = [
        Role::Mafia,
        Role::Doctor,
        Role::Nurse,
        Role::Police,
        Role::Agent,
        Role::Vigilante,
        Role::Inspector,
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
        let (points, reason) = role_specific_rating_adjustment(&player, role, true);
        assert!(points > 0, "{role:?} should have a positive win element");
        assert!(
            !reason.trim().is_empty(),
            "{role:?} should have a visible reason"
        );
    }
}

#[test]
fn first_dead_losing_player_loses_less_rating() {
    let game = rating_test_game();
    let roles = initial_roles(&game);
    let loser = game
        .players
        .iter()
        .find(|player| game.is_citizen_team(player))
        .cloned()
        .unwrap();
    let other_id = game
        .players
        .iter()
        .find(|player| player.user_id != loser.user_id)
        .map(|player| player.user_id)
        .unwrap();

    let mut first_dead_game = game.clone();
    first_dead_game.get_player_mut(loser.user_id).unwrap().alive = false;
    first_dead_game.death_order.push(loser.user_id);

    let mut later_dead_game = game.clone();
    later_dead_game.get_player_mut(loser.user_id).unwrap().alive = false;
    later_dead_game.death_order.push(other_id);
    later_dead_game.death_order.push(loser.user_id);

    let mut first_stats = StatsFile::default();
    let first_log = record_game_stats(
        &mut first_stats,
        &first_dead_game,
        &roles,
        120,
        Winner::Mafia,
    );
    let first_item = first_log
        .iter()
        .find(|item| item.name == loser.name)
        .unwrap();

    let mut later_stats = StatsFile::default();
    let later_log = record_game_stats(
        &mut later_stats,
        &later_dead_game,
        &roles,
        120,
        Winner::Mafia,
    );
    let later_item = later_log
        .iter()
        .find(|item| item.name == loser.name)
        .unwrap();

    assert!(first_item.delta > later_item.delta);
    assert!(first_item.delta <= 0);
    assert!(
        first_item
            .reasons
            .iter()
            .any(|reason| reason.contains("첫 사망 패배 완화"))
    );
    assert!(
        !later_item
            .reasons
            .iter()
            .any(|reason| reason.contains("첫 사망 패배 완화"))
    );
}
