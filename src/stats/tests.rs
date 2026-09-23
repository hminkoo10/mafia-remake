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

// ---------------------------------------------------------------- 코인

#[test]
fn coin_text_groups_thousands() {
    assert_eq!(coin_text(0), "0원");
    assert_eq!(coin_text(1_234_567), "1,234,567원");
    assert_eq!(signed_coin_text(-700), "-700원");
    assert_eq!(signed_coin_text(1_400), "+1,400원");
}

#[test]
fn attendance_pays_once_per_kst_day() {
    let mut stats = StatsFile::default();
    let first = claim_attendance(&mut stats, 1, "Alpha", 10_000, "2026-09-21");
    assert_eq!(
        first,
        AttendanceOutcome::Claimed {
            amount: 10_000,
            balance: 10_000
        }
    );
    let again = claim_attendance(&mut stats, 1, "Alpha", 10_000, "2026-09-21");
    assert_eq!(again, AttendanceOutcome::AlreadyClaimed { balance: 10_000 });
    let next_day = claim_attendance(&mut stats, 1, "Alpha", 10_000, "2026-09-22");
    assert_eq!(
        next_day,
        AttendanceOutcome::Claimed {
            amount: 10_000,
            balance: 20_000
        }
    );
}

#[test]
fn bet_setting_rejects_negative_and_over_balance() {
    let mut stats = StatsFile::default();
    claim_attendance(&mut stats, 1, "Alpha", 5_000, "2026-09-21");
    assert!(set_bet_amount(&mut stats, 1, "Alpha", -1).is_err());
    assert!(set_bet_amount(&mut stats, 1, "Alpha", 5_001).is_err());
    assert_eq!(set_bet_amount(&mut stats, 1, "Alpha", 3_000), Ok(5_000));
    assert_eq!(effective_bet(stats.users.get("1")), 3_000);
    // 보유 코인이 줄면 설정값은 남되 실제 배팅은 보유 안으로 잘린다.
    stats.users.get_mut("1").unwrap().coins = 1_000;
    assert_eq!(effective_bet(stats.users.get("1")), 1_000);
    assert_eq!(effective_bet(None), 0);
}

fn coin_test_game() -> MafiaGame {
    let mut game = rating_test_game();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Citizen),
        (4, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game
}

#[test]
fn bet_settlement_pays_winners_and_charges_losers_within_bounds() {
    let game = coin_test_game();
    let initial_roles = game
        .players
        .iter()
        .map(|player| (player.user_id, player.role))
        .collect::<HashMap<_, _>>();
    let mut stats = StatsFile::default();
    for player in &game.players {
        claim_attendance(
            &mut stats,
            player.user_id,
            &player.name,
            10_000,
            "2026-09-21",
        );
    }
    let bets = HashMap::from([(1, 1_000), (2, 1_000), (3, 0)]);

    let settlements = settle_bets(&mut stats, &game, &initial_roles, Winner::Citizen, &bets);

    assert_eq!(settlements.len(), 2);
    let mafia = settlements.iter().find(|item| item.user_id == 1).unwrap();
    let police = settlements.iter().find(|item| item.user_id == 2).unwrap();
    assert!(
        !mafia.won && (-1_000..=-500).contains(&mafia.delta),
        "{mafia:?}"
    );
    assert!(
        police.won && (500..=4_000).contains(&police.delta),
        "{police:?}"
    );
    assert_eq!(stats.users["1"].coins, 10_000 + mafia.delta);
    assert_eq!(stats.users["2"].coins, 10_000 + police.delta);
    assert_eq!(stats.users["3"].coins, 10_000);
}

#[test]
fn bet_multiplier_favors_underdogs_and_stays_bounded() {
    let game = coin_test_game();
    let mafia = game.get_player(1).unwrap().clone();
    let citizen = game.get_player(3).unwrap().clone();
    let stats = StatsFile::default();

    let (citizen_multiplier, _) = bet_win_multiplier(&stats, &game, &citizen, Role::Citizen);
    let (mafia_multiplier, _) = bet_win_multiplier(&stats, &game, &mafia, Role::Mafia);
    assert!(
        (citizen_multiplier - 1.0).abs() < 1e-9,
        "{citizen_multiplier}"
    );
    assert!((mafia_multiplier - 1.4).abs() < 1e-9, "{mafia_multiplier}");

    // 강자(승률·레이팅 높음)는 배율이 낮아진다.
    let mut strong = StatsFile::default();
    strong.users.insert(
        "3".to_string(),
        PlayerStats {
            games: 20,
            wins: 18,
            rating: 1_600,
            ..Default::default()
        },
    );
    let (strong_multiplier, _) = bet_win_multiplier(&strong, &game, &citizen, Role::Citizen);
    assert!(
        strong_multiplier < citizen_multiplier,
        "{strong_multiplier}"
    );

    // 전체 승률이 낮은 직업은 배율이 오른다.
    let mut hard = StatsFile::default();
    hard.role_outcomes.insert(
        Role::Citizen.value().to_string(),
        RoleOutcome { games: 40, wins: 8 },
    );
    let (hard_multiplier, _) = bet_win_multiplier(&hard, &game, &citizen, Role::Citizen);
    assert!(hard_multiplier > citizen_multiplier, "{hard_multiplier}");

    for multiplier in [
        citizen_multiplier,
        mafia_multiplier,
        strong_multiplier,
        hard_multiplier,
    ] {
        assert!((0.5..=4.0).contains(&multiplier));
    }
}

#[test]
fn game_record_tracks_role_outcomes() {
    let game = coin_test_game();
    let initial_roles = game
        .players
        .iter()
        .map(|player| (player.user_id, player.role))
        .collect::<HashMap<_, _>>();
    let mut stats = StatsFile::default();

    record_game_stats(&mut stats, &game, &initial_roles, 600, Winner::Citizen);

    let mafia = &stats.role_outcomes[Role::Mafia.value()];
    assert_eq!((mafia.games, mafia.wins), (1, 0));
    let police = &stats.role_outcomes[Role::Police.value()];
    assert_eq!((police.games, police.wins), (1, 1));
}

#[test]
fn star_player_prize_is_split_on_ties() {
    // 2번 2표, 4번 2표, 3번 1표.
    let votes = HashMap::from([(1, 2), (3, 2), (2, 4), (4, 3), (5, 4)]);
    let winners = tally_star_votes(&votes);
    assert_eq!(winners, vec![(2, 2), (4, 2)]);
    assert!(tally_star_votes(&HashMap::new()).is_empty());

    let mut stats = StatsFile::default();
    let awards = award_star_players(
        &mut stats,
        &[(2, "Two".to_string(), 2), (4, "Four".to_string(), 2)],
        1_000,
    );
    assert_eq!(
        awards.iter().map(|award| award.prize).collect::<Vec<_>>(),
        vec![500, 500]
    );
    assert_eq!(stats.users["2"].star_player_count, 1);
    assert_eq!(stats.users["4"].coins, 500);
}

#[test]
fn coupon_reservation_refund_and_record() {
    let mut stats = StatsFile::default();
    claim_attendance(&mut stats, 1, "Alpha", 25_000, "2026-09-21");
    assert!(reserve_coins(&mut stats, 1, "Alpha", 30_000).is_err());
    assert_eq!(reserve_coins(&mut stats, 1, "Alpha", 20_000), Ok(5_000));
    assert_eq!(refund_coins(&mut stats, 1, "Alpha", 20_000), 25_000);
    record_coupon(
        &mut stats,
        1,
        "Alpha",
        2,
        vec!["EVENT-AB12CD34".to_string()],
        "2026-09-21T10:00:00+09:00",
    );
    assert_eq!(stats.users["1"].coupon_points_exchanged, 2);
    assert_eq!(stats.users["1"].coupons.len(), 1);
    assert_eq!(stats.users["1"].coupons[0].codes, vec!["EVENT-AB12CD34"]);
}

#[test]
fn admin_coin_adjustments_never_go_negative() {
    let mut stats = StatsFile::default();
    let give = adjust_coins(&mut stats, 1, "Alpha", 3_000);
    assert_eq!((give.before, give.after), (0, 3_000));
    let take = adjust_coins(&mut stats, 1, "Alpha", -5_000);
    assert_eq!((take.before, take.after), (3_000, 0));
    assert!(set_coins(&mut stats, 1, "Alpha", -1).is_err());
    let set = set_coins(&mut stats, 1, "Alpha", 7_500).unwrap();
    assert_eq!((set.before, set.after), (0, 7_500));
    assert_eq!(stats.users["1"].coins, 7_500);
}

fn coupon_rng() -> impl rand::RngCore {
    crate::system_random::rng()
}

#[test]
fn coupon_issue_uses_custom_code_with_suffixes_and_rejects_duplicates() {
    let mut stats = StatsFile::default();
    let single = issue_coupons(
        &mut stats,
        5_000,
        1,
        Some(" event2026 "),
        None,
        9,
        "2026-09-22T10:00:00+09:00",
        &mut coupon_rng(),
    )
    .unwrap();
    assert_eq!(single, vec!["EVENT2026"]);
    let many = issue_coupons(
        &mut stats,
        1_000,
        3,
        Some("GIFT"),
        Some("2026-12-31".to_string()),
        9,
        "2026-09-22T10:00:00+09:00",
        &mut coupon_rng(),
    )
    .unwrap();
    assert_eq!(many, vec!["GIFT-1", "GIFT-2", "GIFT-3"]);
    assert_eq!(
        stats.coin_coupons["GIFT-2"].expires_on.as_deref(),
        Some("2026-12-31")
    );
    // 겹치는 코드는 통째로 거부한다.
    let duplicate = issue_coupons(
        &mut stats,
        1_000,
        2,
        Some("GIFT"),
        None,
        9,
        "2026-09-22T10:00:00+09:00",
        &mut coupon_rng(),
    );
    assert!(duplicate.is_err());
    assert_eq!(stats.coin_coupons.len(), 4);
    // 잘못된 텍스트·개수·코인.
    assert!(
        issue_coupons(
            &mut stats,
            1_000,
            1,
            Some("한글코드"),
            None,
            9,
            "",
            &mut coupon_rng()
        )
        .is_err()
    );
    assert!(issue_coupons(&mut stats, 1_000, 0, None, None, 9, "", &mut coupon_rng()).is_err());
    assert!(issue_coupons(&mut stats, 0, 1, None, None, 9, "", &mut coupon_rng()).is_err());
}

#[test]
fn coupon_issue_generates_unique_random_codes() {
    let mut stats = StatsFile::default();
    let codes = issue_coupons(
        &mut stats,
        2_000,
        20,
        None,
        None,
        9,
        "2026-09-22T10:00:00+09:00",
        &mut coupon_rng(),
    )
    .unwrap();
    assert_eq!(codes.len(), 20);
    let unique = codes.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(unique.len(), 20);
    for code in &codes {
        assert!(code.starts_with("MAFIA-") && code.len() == 15, "{code}");
    }
}

#[test]
fn coupon_redeem_is_single_use_and_checks_expiry() {
    let mut stats = StatsFile::default();
    issue_coupons(
        &mut stats,
        5_000,
        1,
        Some("WELCOME"),
        Some("2026-09-30".to_string()),
        9,
        "2026-09-22T10:00:00+09:00",
        &mut coupon_rng(),
    )
    .unwrap();
    issue_coupons(
        &mut stats,
        3_000,
        1,
        Some("OLD"),
        Some("2026-09-01".to_string()),
        9,
        "2026-08-01T10:00:00+09:00",
        &mut coupon_rng(),
    )
    .unwrap();

    let redeemed = redeem_coupon(
        &mut stats,
        1,
        "Alpha",
        " welcome ",
        "2026-09-30",
        "2026-09-30T12:00:00+09:00",
    )
    .unwrap();
    assert_eq!((redeemed.coins, redeemed.balance), (5_000, 5_000));
    assert_eq!(stats.users["1"].coins, 5_000);
    assert_eq!(stats.coin_coupons["WELCOME"].redeemed_by, Some(1));

    assert!(redeem_coupon(&mut stats, 2, "Beta", "WELCOME", "2026-09-30", "").is_err());
    assert!(redeem_coupon(&mut stats, 2, "Beta", "OLD", "2026-09-22", "").is_err());
    assert!(redeem_coupon(&mut stats, 2, "Beta", "NOPE", "2026-09-22", "").is_err());
    assert!(stats.users.get("2").is_none_or(|entry| entry.coins == 0));

    assert_eq!(active_coupons(&stats, "2026-09-22").len(), 0);
    issue_coupons(
        &mut stats,
        1_000,
        2,
        Some("LIVE"),
        None,
        9,
        "",
        &mut coupon_rng(),
    )
    .unwrap();
    assert_eq!(active_coupons(&stats, "2026-09-22").len(), 2);
    assert!(parse_coupon_expiry("2026-13-01", "2026-09-22").is_err());
    assert!(parse_coupon_expiry("2026-09-21", "2026-09-22").is_err());
    assert_eq!(
        parse_coupon_expiry(" 2026-09-22 ", "2026-09-22"),
        Ok("2026-09-22".to_string())
    );
}

#[test]
fn coin_gifts_move_coins_between_two_players_at_once() {
    let mut stats = StatsFile::default();
    claim_attendance(&mut stats, 1, "Alpha", 10_000, "2026-09-23");
    let gift = gift_coins(&mut stats, 1, "Alpha", 2, "Beta", 3_000, 0).unwrap();
    assert_eq!(
        gift,
        CoinGift {
            amount: 3_000,
            sender_balance: 7_000,
            receiver_balance: 3_000
        }
    );
    assert_eq!(stats.users["1"].coins, 7_000);
    assert_eq!(stats.users["2"].coins, 3_000);
    assert_eq!(stats.users["2"].name, "Beta");
    // 받은 코인을 다시 보낼 수 있고, 전부 보내면 0원이 된다.
    gift_coins(&mut stats, 2, "Beta", 1, "Alpha", 3_000, 0).unwrap();
    assert_eq!(
        (stats.users["1"].coins, stats.users["2"].coins),
        (10_000, 0)
    );
}

#[test]
fn coin_gifts_reject_bad_requests_without_touching_anyone() {
    let mut stats = StatsFile::default();
    claim_attendance(&mut stats, 1, "Alpha", 10_000, "2026-09-23");
    let before = stats.clone();
    for (to, amount, text) in [
        (2, 0, "1원 이상"),
        (2, -500, "1원 이상"),
        (1, 1_000, "자기 자신"),
        (2, 10_001, "보유 코인이 부족"),
    ] {
        let error = gift_coins(&mut stats, 1, "Alpha", to, "Beta", amount, 0).unwrap_err();
        assert!(error.contains(text), "{error}");
    }
    // 코인이 없는 사람은 보낼 수 없고, 기록도 새로 생기지 않는다.
    assert!(gift_coins(&mut stats, 3, "Gamma", 1, "Alpha", 100, 0).is_err());
    assert_eq!(
        serde_json::to_value(&stats).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    assert!(!stats.users.contains_key("2") && !stats.users.contains_key("3"));
}

#[test]
fn coin_gifts_keep_the_unsettled_game_bet() {
    let mut stats = StatsFile::default();
    claim_attendance(&mut stats, 1, "Alpha", 10_000, "2026-09-23");
    // 진행 중인 게임에 4,000원이 걸려 있으면 6,000원까지만 보낼 수 있다.
    let error = gift_coins(&mut stats, 1, "Alpha", 2, "Beta", 6_001, 4_000).unwrap_err();
    assert!(
        error.contains("배팅 4,000원") && error.contains("6,000원까지만"),
        "{error}"
    );
    assert_eq!(stats.users["1"].coins, 10_000);
    let gift = gift_coins(&mut stats, 1, "Alpha", 2, "Beta", 6_000, 4_000).unwrap();
    assert_eq!(gift.sender_balance, 4_000, "배팅액은 정산까지 남는다");
    // 받는 사람 코인이 넘치면 거부한다.
    stats.users.get_mut("2").unwrap().coins = i64::MAX;
    assert!(gift_coins(&mut stats, 1, "Alpha", 2, "Beta", 1, 0).is_err());
    assert_eq!(stats.users["1"].coins, 4_000);
}
