// game 테스트 모듈 (src/game/mod.rs에서 분리)

use super::*;

fn basic_players() -> Vec<(u64, String)> {
    vec![
        (1, "One".to_string()),
        (2, "Two".to_string()),
        (3, "Three".to_string()),
        (4, "Four".to_string()),
        (5, "Five".to_string()),
    ]
}

fn special_mafia_player(role: Role, index: usize) -> Player {
    Player::new(900 + index as u64, format!("{role:?}"), role)
}

#[test]
fn indexes_players_by_id() {
    let game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
    assert_eq!(game.get_player(2).unwrap().name, "Two");
    assert!(game.get_player(999).is_none());
}

#[test]
fn balanced_assignment_avoids_consecutive_mafia_roles() {
    let players = (1..=6)
        .map(|user_id| (user_id, format!("P{user_id}")))
        .collect::<Vec<_>>();
    let mut history = HashMap::new();
    for user_id in 1..=6 {
        let was_mafia = user_id <= 2;
        history.insert(
            user_id,
            PlayerAssignmentHistory {
                games: 4,
                mafia_role_games: if was_mafia { 4 } else { 0 },
                role_counts: HashMap::from([(
                    if was_mafia {
                        Role::Mafia
                    } else {
                        Role::Citizen
                    },
                    4,
                )]),
                recent_roles: vec![if was_mafia {
                    Role::Mafia
                } else {
                    Role::Citizen
                }],
            },
        );
    }

    let game = MafiaGame::new_with_counts_balanced(
        players,
        GameCounts {
            mafia_count: 2,
            ..Default::default()
        },
        &history,
    )
    .unwrap();
    let mafia_ids = game
        .players
        .iter()
        .filter(|player| player.role.is_mafia_team())
        .map(|player| player.user_id)
        .collect::<HashSet<_>>();

    assert!(!mafia_ids.contains(&1));
    assert!(!mafia_ids.contains(&2));
}

#[test]
fn assignment_log_adjusts_role_probability_cost() {
    let rarely_doctor = PlayerAssignmentHistory {
        games: 12,
        role_counts: HashMap::from([(Role::Doctor, 0)]),
        ..Default::default()
    };
    let often_doctor = PlayerAssignmentHistory {
        games: 12,
        role_counts: HashMap::from([(Role::Doctor, 5)]),
        ..Default::default()
    };

    let rare_cost = role_assignment_cost(&rarely_doctor, Role::Doctor, 8, 2, 1);
    let often_cost = role_assignment_cost(&often_doctor, Role::Doctor, 8, 2, 1);

    assert!(often_cost - rare_cost > ROLE_ASSIGNMENT_RANDOM_JITTER as i64);
}

#[test]
fn assignment_history_reduces_inspector_probability() {
    let rarely_inspector = PlayerAssignmentHistory {
        games: 12,
        role_counts: HashMap::from([(Role::Inspector, 0)]),
        ..Default::default()
    };
    let often_inspector = PlayerAssignmentHistory {
        games: 12,
        role_counts: HashMap::from([(Role::Inspector, 5)]),
        ..Default::default()
    };

    let rare_cost = role_assignment_cost(&rarely_inspector, Role::Inspector, 8, 2, 1);
    let often_cost = role_assignment_cost(&often_inspector, Role::Inspector, 8, 2, 1);

    assert!(often_cost - rare_cost > ROLE_ASSIGNMENT_RANDOM_JITTER as i64);
}

#[test]
fn base_inspector_count_is_assigned() {
    let game = MafiaGame::new_with_counts(
        basic_players(),
        GameCounts {
            mafia_count: 1,
            inspector_count: 1,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        game.players
            .iter()
            .filter(|player| player.role == Role::Inspector)
            .count(),
        1
    );
}

#[test]
fn balanced_assignment_evenly_rotates_teams_and_roles() {
    let players = (1..=8)
        .map(|user_id| (user_id, format!("P{user_id}")))
        .collect::<Vec<_>>();
    let mut history = HashMap::<u64, PlayerAssignmentHistory>::new();
    let mut previous_mafia_ids = HashSet::new();

    for _ in 0..32 {
        let game = MafiaGame::new_with_counts_balanced(
            players.clone(),
            GameCounts {
                mafia_count: 2,
                doctor_count: 1,
                police_count: 1,
                ..Default::default()
            },
            &history,
        )
        .unwrap();
        let mafia_ids = game
            .players
            .iter()
            .filter(|player| player.role.is_mafia_team())
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        if !previous_mafia_ids.is_empty() {
            assert!(mafia_ids.is_disjoint(&previous_mafia_ids));
        }

        for player in &game.players {
            let entry = history.entry(player.user_id).or_default();
            entry.games += 1;
            if player.role.is_mafia_team() {
                entry.mafia_role_games += 1;
            }
            *entry.role_counts.entry(player.role).or_default() += 1;
            entry.recent_roles.insert(0, player.role);
            entry.recent_roles.truncate(3);
        }
        previous_mafia_ids = mafia_ids;
    }

    for role in [Role::Mafia, Role::Doctor, Role::Police] {
        let counts = (1..=8)
            .map(|user_id| {
                history[&user_id]
                    .role_counts
                    .get(&role)
                    .copied()
                    .unwrap_or(0)
            })
            .collect::<Vec<_>>();
        assert!(counts.iter().max().unwrap() - counts.iter().min().unwrap() <= 1);
    }
}

#[test]
fn uncontacted_mafia_specials_are_citizen_for_investigations() {
    let game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();

    for (index, role) in crate::model::MAFIA_SPECIAL_ROLES
        .iter()
        .copied()
        .enumerate()
    {
        let player = special_mafia_player(role, index);

        assert!(
            !game.is_police_detected_mafia_team(&player),
            "{role:?} should not be police-detected as mafia before contact"
        );
        assert_eq!(
            game.team_key(&player),
            "citizen",
            "{role:?} should be citizen team for relation investigations before contact"
        );
    }
}

#[test]
fn contacted_mafia_specials_are_mafia_for_investigations() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();

    for (index, role) in crate::model::MAFIA_SPECIAL_ROLES
        .iter()
        .copied()
        .enumerate()
    {
        let player = special_mafia_player(role, index);
        game.contact_mafia_team_member(&player);

        assert_eq!(
            game.team_key(&player),
            "mafia",
            "{role:?} should be mafia team for relation investigations after contact"
        );
        if role == Role::Godfather {
            assert!(
                !game.is_police_detected_mafia_team(&player),
                "Godfather should keep police concealment even after contact"
            );
        } else {
            assert!(
                game.is_police_detected_mafia_team(&player),
                "{role:?} should be police-detected as mafia after contact"
            );
        }
    }
}

#[test]
fn contractor_can_target_hidden_investigation_roles() {
    let players = (1..=8)
        .map(|user_id| (user_id, format!("P{user_id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, Vec::new()).unwrap();
    for (user_id, role) in [
        (1, Role::Contractor),
        (2, Role::Police),
        (3, Role::Agent),
        (4, Role::Vigilante),
        (5, Role::Inspector),
        (6, Role::Judge),
        (7, Role::Citizen),
        (8, Role::Mafia),
    ] {
        game.get_player_mut(user_id).unwrap().role = role;
    }
    game.publicly_revealed_ids.insert(6);
    game.phase = Phase::Night;
    game.day_number = 2;
    let contractor = game.get_player(1).unwrap().clone();

    let target_ids = game
        .contractor_contract_targets(&contractor)
        .into_iter()
        .map(|player| player.user_id)
        .collect::<HashSet<_>>();

    assert_eq!(target_ids, HashSet::from([2, 3, 4, 5, 7, 8]));
    for role in [Role::Police, Role::Agent, Role::Vigilante, Role::Inspector] {
        assert!(!crate::model::is_contractor_guess_role(role));
    }
    assert!(crate::model::is_contractor_guess_role(Role::Detective));
    assert!(
        game.submit_contractor_contract(1, 2, Role::Police, 3, Role::Citizen)
            .is_err()
    );
    assert!(
        game.submit_contractor_contract(1, 2, Role::Citizen, 3, Role::Mafia)
            .is_ok()
    );
}

#[test]
fn winning_prophet_is_exposed_for_victory_announcement() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    game.get_player_mut(2).unwrap().role = Role::Prophet;
    game.phase = Phase::Day;
    game.day_number = 4;

    assert_eq!(game.winner(), Some(Winner::Citizen));
    assert_eq!(game.winning_prophet().map(|player| player.user_id), Some(2));
}

#[test]
fn scientist_is_mafia_team_but_hidden_until_first_death() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    game.get_player_mut(2).unwrap().role = Role::Scientist;
    let scientist = game.get_player(2).unwrap().clone();

    assert!(game.is_mafia_team(&scientist));
    assert!(!game.is_citizen_team(&scientist));
    assert!(!game.is_known_mafia_team(&scientist));

    game.mark_dead(scientist.user_id).unwrap();
    let dead_scientist = game.get_player(scientist.user_id).unwrap();

    assert!(game.scientist_contacted.contains(&scientist.user_id));
    assert!(game.is_mafia_team(dead_scientist));
    assert!(!game.is_citizen_team(dead_scientist));
    assert!(game.is_known_mafia_team(dead_scientist));
}

#[test]
fn agent_directive_ignores_uncontacted_mafia_specials() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Agent),
        (3, Role::Spy),
        (4, Role::Mafia),
        (5, Role::Joker),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    let result = game.resolve_night().unwrap();

    assert!(!game.agent_discovered_ids.contains(&3));
    assert!(
        result
            .agent_results
            .get(&2)
            .is_some_and(|text| !text.contains("Three"))
    );
}

/// [성불] 결과가 제출 즉시 나오고 밤마다 한 번으로 고정되며, 결산은
/// 채널 정리 목록만 만든다.
#[test]
fn shaman_purification_returns_the_result_immediately() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Shaman]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Shaman),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.get_player_mut(3).unwrap().alive = false;
    game.get_player_mut(4).unwrap().alive = false;

    // 제출 즉시 직업이 공개되고 성불이 바로 적용된다.
    let message = game.submit_night_action(2, Some(3)).unwrap();
    assert!(
        message.contains("[성불] P3 님의 직업은 **의사**"),
        "{message}"
    );
    assert!(game.purified_dead_ids.contains(&3));

    // 밤마다 한 번뿐이라 다른 사망자로 바꿀 수 없다.
    let error = game.submit_night_action(2, Some(4)).unwrap_err();
    assert!(error.to_string().contains("한 번뿐"), "{error}");

    // 결산: 개인 메시지는 없고 채널 정리 목록만 남는다.
    let result = game.resolve_night().unwrap();
    assert!(result.shaman_results.is_empty());
    assert_eq!(result.shaman_purifications, vec![3]);
}

/// 마녀 저주는 걸린 밤과 다음 낮 동안 유지되고, 다음 밤이 시작될 때
/// (러너의 restore_frogs) 풀린다. 풀린 뒤에는 밤 행동도 정상으로 돌아온다.
#[test]
fn witch_curse_lifts_at_the_start_of_the_next_night() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 1, vec![Role::Witch]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Witch),
        (3, Role::Police),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    // 밤 시작 (러너 순서): 이전 밤 개구리가 없으니 복구 대상도 없다.
    assert!(game.restore_frogs().is_empty());

    // 경찰이 조사를 제출한 뒤 마녀 저주가 적용된다 (밤 중 타이머 이벤트).
    game.submit_night_action(3, Some(4)).unwrap();
    game.witch_targets.insert(2, 3);
    let (cursed, _) = game.apply_witch_curses(&HashSet::new());
    assert_eq!(
        cursed
            .iter()
            .map(|player| player.user_id)
            .collect::<Vec<_>>(),
        vec![3]
    );
    let police = game.get_player(3).unwrap().clone();
    assert!(game.is_frog(&police));
    // 저주가 이미 제출한 밤 행동도 지운다.
    assert!(game.police_targets.is_empty());

    // 밤 결산을 지나 다음 낮 동안에도 저주는 유지된다.
    game.resolve_night().unwrap();
    assert!(game.is_frog(game.get_player(3).unwrap()));
    // 개구리는 밤 행동 대상 목록에서 빠진다.
    game.phase = Phase::Night;
    game.day_number = 2;
    assert!(
        !game
            .night_action_actors()
            .iter()
            .any(|actor| actor.user_id == 3)
    );

    // 다음 밤 시작: restore_frogs가 저주를 풀고 복구 목록으로 알려준다.
    let restored = game.restore_frogs();
    assert_eq!(
        restored
            .iter()
            .map(|player| player.user_id)
            .collect::<Vec<_>>(),
        vec![3]
    );
    assert!(!game.is_frog(game.get_player(3).unwrap()));
    assert!(game.frog_user_ids.is_empty());

    // 풀린 뒤에는 밤 행동을 다시 쓸 수 있다.
    assert!(
        game.night_action_actors()
            .iter()
            .any(|actor| actor.user_id == 3)
    );
    game.submit_night_action(3, Some(5)).unwrap();
}

#[test]
fn agent_directive_reports_frog_instead_of_original_role() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Agent),
        (3, Role::Doctor),
        (4, Role::Mafia),
        (5, Role::Joker),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.frog_user_ids.insert(3);

    let result = game.resolve_night().unwrap();
    let directive = result.agent_results.get(&2).unwrap();

    assert!(directive.contains(Role::Frog.value()), "{directive}");
    assert!(!directive.contains(Role::Doctor.value()), "{directive}");
}

#[test]
fn agent_receives_directive_when_killed_the_same_night() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Agent),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(1, Some(2)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(
        result
            .killed_players
            .iter()
            .any(|player| player.user_id == 2)
    );
    assert!(result.agent_results.contains_key(&2));
}

/// 티어 배정: 전원이 2~6티어를 받고, 4티어 이상 능력은 시작 역할의
/// 풀에서 티어에 맞는 개수(4=1, 5=2, 6=3, 풀이 작으면 풀 크기)만큼 서로
/// 다른 능력으로 나온다. 같은 능력이 여러 플레이어에게 겹칠 수는 있다.
#[test]
fn tier_abilities_follow_group_pools() {
    use crate::model::{TIER3_ABILITIES, tier4_pool};
    for _ in 0..20 {
        let players = (1..=10)
            .map(|id| (id as u64, format!("P{id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 2, 1, 1, vec![Role::Spy, Role::Madam]).unwrap();
        game.assign_tier_abilities();

        assert_eq!(game.player_tiers.len(), 10);
        for player in &game.players {
            let tier = game.player_tiers[&player.user_id];
            assert!((2..=6).contains(&tier), "{tier}");
            let abilities = game.player_tier_abilities(player.user_id);
            match tier {
                2 => assert!(abilities.is_empty(), "{:?} {abilities:?}", player.role),
                3 => {
                    assert_eq!(abilities.len(), 1, "{abilities:?}");
                    assert_eq!(abilities[0].tier(), 3, "{abilities:?}");
                }
                _ => {
                    let pool = tier4_pool(player.role);
                    let want = tier as usize - 3;
                    let expected = want.min(pool.len() + TIER3_ABILITIES.len());
                    assert_eq!(
                        abilities.len(),
                        expected,
                        "{:?} {tier} {abilities:?}",
                        player.role
                    );
                    let unique = abilities.iter().collect::<HashSet<_>>();
                    assert_eq!(unique.len(), abilities.len(), "{abilities:?}");
                    for ability in &abilities {
                        assert!(
                            pool.contains(ability) || TIER3_ABILITIES.contains(ability),
                            "{:?} {ability:?}",
                            player.role
                        );
                    }
                    // 3티어 채움은 4티어 풀을 다 쓴 뒤에만 일어난다.
                    if abilities.iter().any(|ability| !pool.contains(ability)) {
                        let tier4_count = abilities
                            .iter()
                            .filter(|ability| pool.contains(ability))
                            .count();
                        assert_eq!(tier4_count, pool.len(), "{abilities:?}");
                    }
                }
            }
        }
    }
}

/// 티어 확률(2티어 40% / 3티어 30% / 4티어 15% / 5티어 10% / 6티어 5%)이
/// 실제 분포로 나오는지 대량 표본으로 확인한다. 허용 오차 ±3%p는 표본
/// 20,000명 기준 표준편차의 8배 이상이라 사실상 플레이크가 나지 않는다.
#[test]
fn tier_probabilities_match_the_declared_distribution() {
    let mut counts = [0u32; 5];
    let mut total = 0u32;
    for _ in 0..2000 {
        let players = (1..=10)
            .map(|id| (id as u64, format!("P{id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 2, 1, 1, Vec::new()).unwrap();
        game.assign_tier_abilities();
        for tier in game.player_tiers.values() {
            counts[(*tier - 2) as usize] += 1;
            total += 1;
        }
    }
    assert_eq!(total, 20_000);
    let percent = |count: u32| count as f64 * 100.0 / total as f64;
    for (index, expected) in [40.0, 30.0, 15.0, 10.0, 5.0].into_iter().enumerate() {
        let share = percent(counts[index]);
        assert!(
            (share - expected).abs() <= 3.0,
            "{}티어 {share:.2}% (기대 {expected}%)",
            index + 2
        );
    }
}

/// 능력 배정이 풀 안에서 균등하게 나오는지 대량 표본으로 확인한다.
/// 3티어 풀과 역할별 4티어 이상 풀마다 각 능력의 비율이 균등 기대치
/// ±6%p 안이어야 한다 (다중 배정도 서로 다른 능력을 균등 추출하므로
/// 능력별 점유율 기대치는 1/풀 크기 그대로다).
#[test]
fn tier_ability_rolls_are_uniform_within_each_pool() {
    use crate::model::{TIER3_ABILITIES, tier4_pool};
    let mut tier3: HashMap<TierAbility, u32> = HashMap::new();
    let mut tier4_by_role: HashMap<Role, HashMap<TierAbility, u32>> = HashMap::new();
    for _ in 0..10_000 {
        let players = (1..=10)
            .map(|id| (id as u64, format!("P{id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 2, 1, 1, vec![Role::Spy, Role::Madam]).unwrap();
        game.assign_tier_abilities();
        for player in &game.players {
            for ability in game.player_tier_abilities(player.user_id) {
                let bucket = if ability.tier() == 3 {
                    &mut tier3
                } else {
                    tier4_by_role.entry(player.role).or_default()
                };
                *bucket.entry(ability).or_default() += 1;
            }
        }
    }
    let check = |label: &str, counts: &HashMap<TierAbility, u32>, pool: &[TierAbility]| {
        let total: u32 = counts.values().sum();
        let expected = 100.0 / pool.len() as f64;
        for ability in pool {
            let share = counts.get(ability).copied().unwrap_or(0) as f64 * 100.0 / total as f64;
            assert!(
                (share - expected).abs() <= 6.0,
                "{label} {ability:?}: {share:.2}% (기대 {expected:.2}%, 표본 {total})"
            );
        }
    };
    check("3티어", &tier3, TIER3_ABILITIES);
    for (role, counts) in &tier4_by_role {
        check(role.value(), counts, &tier4_pool(*role));
    }
}

/// [시한부] 절반 이하 + 2번째 밤 생존 시 보유자의 팀이 즉시 승리한다.
/// 포교된 보유자는 교주팀 승리가 된다.
#[test]
fn time_limit_wins_for_the_holders_team_at_half_survivors() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 2, 0, 0, vec![Role::Spy]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Mafia),
        (3, Role::Spy),
        (4, Role::CultLeader),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::TimeLimit]);

    // 아직 첫 밤: 발동하지 않는다.
    assert_eq!(game.winner(), None);

    // 2번째 밤이지만 생존자가 절반보다 많으면 발동하지 않는다.
    game.day_number = 2;
    game.phase = Phase::Night;
    for id in [5, 6, 7] {
        game.get_player_mut(id).unwrap().alive = false;
    }
    assert_eq!(game.winner(), None);

    // 절반(4명) 이하가 되면 마피아팀 승리.
    game.get_player_mut(8).unwrap().alive = false;
    assert_eq!(game.winner(), Some(Winner::Mafia));

    // 보유자가 죽으면 발동하지 않는다.
    game.get_player_mut(1).unwrap().alive = false;
    assert_ne!(game.winner(), Some(Winner::Mafia));

    // 포교된 보조 보유자는 교주팀 승리.
    game.tier_abilities.insert(3, vec![TierAbility::TimeLimit]);
    game.culted_ids.insert(3);
    assert_eq!(game.winner(), Some(Winner::Cult));
}

/// [밀정] 두 번째 낮이 되면 보유 보조가 자동으로 마피아와 접선한다.
#[test]
fn inside_man_auto_contacts_on_the_second_day() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Spy]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Spy),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::InsideMan]);

    // 첫 밤 결산: 아직 접선하지 않는다.
    let result = game.resolve_night().unwrap();
    assert!(result.tier_ability_contacts.is_empty());
    assert!(!game.spy_contacted.contains(&2));

    // 2일차 밤 결산(두 번째 낮): 자동 접선.
    game.phase = Phase::Night;
    game.day_number = 2;
    let result = game.resolve_night().unwrap();
    assert_eq!(result.tier_ability_contacts, vec![2]);
    assert!(game.spy_contacted.contains(&2));
    assert!(result.tier_ability_results[&2].contains("[밀정]"));
}

/// [부검] 사망자가 생기면 스파이 보유자가 실제 직업을 자동으로 안다.
#[test]
fn autopsy_reports_the_dead_players_real_role() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Spy]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Spy),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::Autopsy]);
    game.mafia_targets.insert(1, 3);

    let result = game.resolve_night().unwrap();
    assert!(
        result.tier_ability_results[&2].contains("[부검] P3님의 직업은 의사이었습니다."),
        "{result:?}"
    );
}

/// [자객] 마피아팀에 혼자 남은 스파이는 첩보한 대상을 처형한다.
#[test]
fn assassin_kills_the_investigated_target_when_alone() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Spy]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Spy),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::Assassin]);
    game.spy_targets.entry(2).or_default().push(3);

    // 마피아가 살아있으면 발동하지 않는다.
    let result = game.resolve_night().unwrap();
    assert!(game.get_player(3).unwrap().alive, "{result:?}");

    // 혼자 남으면 조사 대상을 처형한다.
    game.get_player_mut(1).unwrap().alive = false;
    game.phase = Phase::Night;
    game.day_number = 2;
    game.spy_targets.entry(2).or_default().push(4);
    game.resolve_night().unwrap();
    assert!(!game.get_player(4).unwrap().alive);
}

/// [미인계] 시민팀 능력의 대상이 되면 사용자의 직업이 보유자에게 알려진다.
/// 요원과 마피아팀 사용자는 발동하지 않는다.
#[test]
fn honeytrap_reveals_the_citizen_ability_user() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 1, 1, vec![Role::Spy]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Spy),
        (3, Role::Police),
        (4, Role::Doctor),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::Honeytrap]);
    game.police_targets.insert(3, 2);
    game.doctor_targets.insert(4, 2);

    let result = game.resolve_night().unwrap();
    let notice = &result.tier_ability_results[&2];
    assert!(notice.contains("P3님의 직업은 경찰입니다"), "{notice}");
    assert!(notice.contains("P4님의 직업은 의사입니다"), "{notice}");
}

/// [현혹]·[데뷔] 시민팀을 유혹하면 직업을 알아내고, 첫날엔 투표권도
/// 한 표 깎는다.
#[test]
fn allure_and_debut_trigger_on_citizen_seduction() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Madam]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Madam),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities
        .insert(2, vec![TierAbility::Allure, TierAbility::Debut]);

    let live_votes = HashMap::from([(2u64, Some(3u64))]);
    game.apply_madam_seduction(&live_votes);

    assert!(game.madam_seduced_ids.contains(&3));
    assert!(game.debut_vote_penalty_ids.contains(&3));
    assert_eq!(game.vote_weight(3), 0);
    assert_eq!(game.vote_weight(4), 1);

    // 알림은 다음 밤 결산에서 전달된다.
    game.phase = Phase::Night;
    let result = game.resolve_night().unwrap();
    let notice = &result.tier_ability_results[&2];
    assert!(
        notice.contains("[현혹] 유혹한 P3님의 직업은 의사입니다."),
        "{notice}"
    );
    assert!(notice.contains("[데뷔]"), "{notice}");
}

/// [후계자] 마피아 본대가 전멸하면 보유 도둑이 마피아가 된다.
#[test]
fn successor_thief_becomes_mafia_when_all_mafia_die() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Thief]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Thief),
        (3, Role::Vigilante),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::Successor]);
    game.vigilante_targets.insert(3, 1);
    game.vigilante_known_enemy_ids
        .entry(3)
        .or_default()
        .insert(1);

    let result = game.resolve_night().unwrap();
    assert!(!game.get_player(1).unwrap().alive, "{result:?}");
    assert_eq!(game.get_player(2).unwrap().role, Role::Mafia);
    assert!(result.tier_ability_contacts.contains(&2));
    assert!(result.tier_ability_results[&2].contains("[후계자]"));
    // 마피아가 이어졌으니 시민 승리가 아니다.
    assert_eq!(game.winner(), None);
}

/// [조문] 훔친 능력이 없는 도둑이 밤에 사망자의 직업을 도벽하고, 훔친
/// 능력은 다음 밤까지 유지된다.
#[test]
fn condolence_steals_from_the_dead_at_night() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Thief]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Thief),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::Condolence]);
    game.get_player_mut(3).unwrap().alive = false;

    let thief = game.get_player(2).unwrap().clone();
    assert!(
        game.night_action_actors()
            .iter()
            .any(|actor| actor.user_id == 2)
    );
    // 산 사람은 조문할 수 없다.
    assert!(game.submit_night_action(2, Some(4)).is_err());
    let message = game.submit_night_action(2, Some(3)).unwrap();
    assert!(message.contains("[조문]"), "{message}");
    assert_eq!(game.thief_stolen_roles.get(&2), Some(&Role::Doctor));

    // 이번 밤 결산을 지나도 훔친 능력이 남고, 다음 밤 의사 행동을 쓸 수 있다.
    game.resolve_night().unwrap();
    assert_eq!(game.thief_stolen_roles.get(&2), Some(&Role::Doctor));
    assert_eq!(game.thief_night_role(&thief), Some(Role::Doctor));

    // 그다음 밤 결산에서는 정리된다.
    game.phase = Phase::Night;
    game.day_number = 2;
    game.resolve_night().unwrap();
    assert!(game.thief_stolen_roles.get(&2).is_none());
}

/// [망각술] 저주 상태로 죽은 테러리스트의 지목 반격이 발동하지 않는다.
#[test]
fn amnesia_blocks_death_abilities_of_cursed_victims() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Witch]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Witch),
        (3, Role::Terrorist),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::Amnesia]);
    game.frog_user_ids.insert(3);
    game.terrorist_targets.insert(3, 4);
    game.mafia_targets.insert(1, 3);

    let result = game.resolve_night().unwrap();
    assert!(!game.get_player(3).unwrap().alive);
    // 지목 반격이 봉인되어 4번은 살아있다.
    assert!(game.get_player(4).unwrap().alive, "{result:?}");
    assert!(result.terrorist_retaliations.is_empty());
    assert!(result.tier_ability_results[&2].contains("[망각술]"));
}

/// [왜곡] 첫 밤 마피아의 공격을 받은 과학자 보유자는 죽지 않고 접선한다.
/// [분석]은 부활 시 공격자 정보를 전달한다.
#[test]
fn distortion_intercepts_the_first_night_attack() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Scientist]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Scientist),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::Distortion]);
    game.mafia_targets.insert(1, 2);

    let result = game.resolve_night().unwrap();
    assert!(game.get_player(2).unwrap().alive, "{result:?}");
    assert!(game.scientist_contacted.contains(&2));
    assert!(result.tier_ability_contacts.contains(&2));
    assert!(result.tier_ability_results[&2].contains("[왜곡]"));
}

/// [분석] 자해 부활 예정 과학자가 공격자 정보를 받는다.
#[test]
fn analysis_records_the_attacker_for_the_reviving_scientist() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Scientist]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Scientist),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::Analysis]);
    game.mafia_targets.insert(1, 2);

    game.resolve_night().unwrap();
    assert!(!game.get_player(2).unwrap().alive);
    assert!(game.scientist_pending_revive_ids.contains(&2));
    let notice = game.take_analysis_notice(2).unwrap();
    assert!(notice.contains("P1님의 직업은 마피아입니다"), "{notice}");
}

/// [직감] 보유 청부업자는 시작 시 시민팀 한 명의 직업 힌트를 받는다.
#[test]
fn intuition_prepares_a_citizen_role_hint() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Contractor]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Contractor),
        (3, Role::Doctor),
        (4, Role::Doctor),
        (5, Role::Doctor),
        (6, Role::Doctor),
        (7, Role::Doctor),
        (8, Role::Doctor),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::Intuition]);
    game.prepare_intuition_hints();

    let hint = game.intuition_hints.get(&2).unwrap();
    assert!(hint.contains("[직감]"), "{hint}");
    assert!(hint.contains("의사"), "{hint}");
}

/// [뒷처리] 접선한 대부는 마피아팀 처형 희생자의 직업을 알고 시민팀이면
/// 시민으로 가린다. 접선 전에는 발동하지 않는다.
#[test]
fn fixer_masks_victims_only_after_contact() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Godfather]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Godfather),
        (3, Role::Doctor),
        (4, Role::Nurse),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(2, vec![TierAbility::Fixer]);

    // 접선 전에는 발동하지 않는다.
    game.mafia_targets.insert(1, 3);
    let result = game.resolve_night().unwrap();
    assert!(!result.tier_ability_results.contains_key(&2), "{result:?}");
    assert!(!game.cleanup_masked_ids.contains(&3));

    // 접선 후에는 직업을 알아내고 시민으로 가린다.
    game.godfather_contacted.insert(2);
    game.phase = Phase::Night;
    game.day_number = 2;
    game.mafia_targets.insert(1, 4);
    let result = game.resolve_night().unwrap();
    assert!(
        result.tier_ability_results[&2].contains("[뒷처리] P4님의 직업은 간호사이었습니다."),
        "{result:?}"
    );
    assert!(game.cleanup_masked_ids.contains(&4));
    assert!(
        result
            .killed_players
            .iter()
            .any(|player| player.user_id == 4 && player.role == Role::Citizen)
    );
}

/// 사립탐정이 경찰을 추적하면 경찰의 조사 사용 여부와 대상이 보인다.
#[test]
fn detective_sees_police_investigation_activity() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 1, vec![Role::Detective]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Detective),
        (3, Role::Police),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    // 경찰이 5번을 조사하고, 사탐이 경찰을 추적한다.
    game.submit_night_action(3, Some(5)).unwrap();
    game.submit_night_action(2, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();
    assert_eq!(
        result.detective_results.get(&2).map(String::as_str),
        Some("P3 님은 밤에 P5 님에게 능력을 사용했습니다.")
    );

    // 다음 밤 경찰이 조사하지 않으면 미사용으로 나온다.
    game.phase = Phase::Night;
    game.day_number = 2;
    game.submit_night_action(2, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();
    assert_eq!(
        result.detective_results.get(&2).map(String::as_str),
        Some("P3 님은 밤에 능력을 사용하지 않았습니다.")
    );

    // 경찰이 조사를 제출한 뒤 같은 밤에 죽어도 이동은 그대로 보인다.
    game.phase = Phase::Night;
    game.day_number = 3;
    game.submit_night_action(3, Some(6)).unwrap();
    game.submit_night_action(2, Some(3)).unwrap();
    game.submit_night_action(1, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();
    assert!(!game.get_player(3).unwrap().alive);
    assert_eq!(
        result.detective_results.get(&2).map(String::as_str),
        Some("P3 님은 밤에 P6 님에게 능력을 사용했습니다.")
    );
}

/// 경찰이 그 밤에 죽어도(예: 소생으로 부활 예정) 제출한 조사 표는
/// 요약 집계에 그대로 남는다.
#[test]
fn police_recap_counts_votes_from_officers_killed_that_night() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 1, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(2, Some(3)).unwrap();
    game.submit_night_action(1, Some(2)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(!game.get_player(2).unwrap().alive);
    assert_eq!(
        result.police_target.as_ref().map(|player| player.user_id),
        Some(3),
        "{result:?}"
    );
}

/// 경찰이 1명일 때 조사 대상이 같은 밤에 죽어도 결과가 성립한다
/// ("과반 미달" 오표시 회귀 방지).
#[test]
fn single_police_result_survives_the_targets_death() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 1, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    // 경찰이 3번을 조사하고, 마피아가 같은 밤 3번을 죽인다.
    game.submit_night_action(2, Some(3)).unwrap();
    game.submit_night_action(1, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(!game.get_player(3).unwrap().alive);
    assert_eq!(
        result.police_target.as_ref().map(|player| player.user_id),
        Some(3),
        "{result:?}"
    );
    assert_eq!(result.police_target_is_mafia, Some(false));
}

/// [수배] 첫 낮이 될 때 접선하지 않은 마피아팀 명단이 보유자에게 오고,
/// 이미 접선한 보조와 둘째 밤 이후는 제외된다.
#[test]
fn wanted_lists_uncontacted_mafia_team_on_first_day() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, vec![Role::Spy, Role::Madam]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Spy),
        (3, Role::Madam),
        (4, Role::Citizen),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Wanted]);
    game.madam_contacted.insert(3);

    let result = game.resolve_night().unwrap();
    let notice = &result.tier_ability_results[&1];
    assert!(notice.contains("[수배]"), "{notice}");
    assert!(notice.contains("P2"), "{notice}");
    assert!(!notice.contains("P3"), "{notice}");

    // 둘째 밤부터는 다시 오지 않는다.
    game.phase = Phase::Night;
    game.day_number = 2;
    let result = game.resolve_night().unwrap();
    assert!(!result.tier_ability_results.contains_key(&1));
}

/// [지령] 첫 낮에 마피아·청부업자 보유자는 경찰 계열이 누군지, 보조·교주
/// 보유자는 미공개 시민팀 한 명의 직업을 안다.
#[test]
fn directive_gives_role_appropriate_intel_on_first_day() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 1, vec![Role::Spy]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Spy),
        (3, Role::Police),
        (4, Role::Doctor),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Directive]);
    game.tier_abilities.insert(2, vec![TierAbility::Directive]);
    // 정체가 공개된 시민은 지령 대상에서 빠진다. 4~8 중 4만 남기고 공개해
    // 보조 지령 결과를 결정적으로 만든다.
    for id in [3, 5, 6, 7, 8] {
        game.publicly_revealed_ids.insert(id);
    }

    let result = game.resolve_night().unwrap();
    let mafia_notice = &result.tier_ability_results[&1];
    assert_eq!(mafia_notice, "[지령] P3님은 경찰 계열 직업입니다.");
    let spy_notice = &result.tier_ability_results[&2];
    assert_eq!(spy_notice, "[지령] P4님의 직업은 의사입니다.");

    // 둘째 밤부터는 오지 않는다.
    game.phase = Phase::Night;
    game.day_number = 2;
    let result = game.resolve_night().unwrap();
    assert!(!result.tier_ability_results.contains_key(&1));
    assert!(!result.tier_ability_results.contains_key(&2));
}

/// [위선] 첫 밤 동안 조사가 의사로 판정하고, 둘째 밤부터는 원래대로다.
#[test]
fn hypocrisy_passes_first_night_investigations_as_doctor() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Citizen),
        (3, Role::Inspector),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Hypocrisy]);

    let mafia = game.get_player(1).unwrap().clone();
    // 경찰 판정: 첫 밤에는 마피아팀이 아니라고 나온다.
    assert!(!game.is_police_detected_mafia_team(&mafia));
    // 형사 판정: 같은 시민팀으로 보여 직업이 '의사'로 공개된다.
    let immediate = game.submit_night_action(3, Some(1)).unwrap();
    assert!(
        immediate.contains("[One님의 직업은 의사입니다.]"),
        "{immediate}"
    );

    // 둘째 밤부터는 원래 판정으로 돌아온다.
    game.day_number = 2;
    assert!(game.is_police_detected_mafia_team(&mafia));
    assert_eq!(game.visible_role(&mafia), Role::Mafia);
}

/// [은폐] 처형 실패(치료·방탄)가 조용한 밤으로 가려지고, 군인 방탄은
/// 소모되지만 공개 문구·정체 공개가 사라진다.
#[test]
fn concealment_hides_failed_kills_as_a_quiet_night() {
    // 치료 실패: quiet_night가 서고 보유자에게만 알림이 간다.
    let mut game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Doctor),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities
        .insert(1, vec![TierAbility::Concealment]);
    game.mafia_targets.insert(1, 3);
    game.doctor_targets.insert(2, 3);

    let result = game.resolve_night().unwrap();
    assert!(result.quiet_night);
    assert!(game.get_player(3).unwrap().alive);
    assert!(result.tier_ability_results[&1].contains("[은폐]"));

    // 군인 방탄: 방탄은 소모되지만 공개 목록과 정체 공개가 비어 있다.
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Soldier),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities
        .insert(1, vec![TierAbility::Concealment]);
    game.mafia_targets.insert(1, 2);

    let result = game.resolve_night().unwrap();
    assert!(result.quiet_night);
    assert!(result.soldier_blocks.is_empty());
    assert!(game.get_player(2).unwrap().alive);
    assert!(game.soldier_bulletproof_used.contains(&2));
    assert!(!game.publicly_revealed_ids.contains(&2));

    // 보유자가 없으면 기존대로 공개된다.
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Soldier),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.mafia_targets.insert(1, 2);
    let result = game.resolve_night().unwrap();
    assert!(!result.quiet_night);
    assert_eq!(result.soldier_blocks.len(), 1);
}

/// [저격] 전날 밤 처형이 실패하면 다음 밤은 치료·방탄을 모두 관통하고,
/// 성공한 밤 다음에는 발동하지 않는다.
#[test]
fn snipe_pierces_all_protection_after_a_failed_night() {
    let mut game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Doctor),
        (3, Role::Soldier),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Snipe]);

    // 1일차 밤: 치료에 막혀 실패 → 저격 장전.
    game.mafia_targets.insert(1, 4);
    game.doctor_targets.insert(2, 4);
    game.resolve_night().unwrap();
    assert!(game.get_player(4).unwrap().alive);
    assert!(game.snipe_armed);

    // 2일차 밤: 치료 중인 대상도 관통해 처형한다.
    game.phase = Phase::Night;
    game.day_number = 2;
    game.mafia_targets.insert(1, 4);
    game.doctor_targets.insert(2, 4);
    let result = game.resolve_night().unwrap();
    assert!(!game.get_player(4).unwrap().alive);
    assert!(result.tier_ability_results[&1].contains("[저격]"));
    // 성공했으니 장전 해제.
    assert!(!game.snipe_armed);

    // 3일차 밤: 군인 방탄도 저격이 장전됐을 때만 관통된다. 우선 실패로 장전.
    game.phase = Phase::Night;
    game.day_number = 3;
    game.mafia_targets.insert(1, 5);
    game.doctor_targets.insert(2, 5);
    game.resolve_night().unwrap();
    assert!(game.snipe_armed);
    game.phase = Phase::Night;
    game.day_number = 4;
    game.mafia_targets.insert(1, 3);
    let result = game.resolve_night().unwrap();
    assert!(!game.get_player(3).unwrap().alive, "{result:?}");
    assert!(result.soldier_blocks.is_empty());
}

/// [독살] 시민팀 처형 실패 시 중독되어 다음 밤에 죽고, 마피아팀 보조는
/// 면역이다.
#[test]
fn poison_kills_a_protected_citizen_one_day_later() {
    let mut game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Doctor),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Poison]);

    // 1일차 밤: 치료에 막혀 실패 → 중독.
    game.mafia_targets.insert(1, 3);
    game.doctor_targets.insert(2, 3);
    let result = game.resolve_night().unwrap();
    assert!(game.get_player(3).unwrap().alive);
    assert!(
        result.tier_ability_results[&1].contains("[독살]"),
        "{result:?}"
    );
    assert_eq!(game.poisoned_death_days.get(&3), Some(&2));

    // 2일차 밤 결산: 중독 사망.
    game.phase = Phase::Night;
    game.day_number = 2;
    let result = game.resolve_night().unwrap();
    assert!(!game.get_player(3).unwrap().alive);
    assert!(
        result
            .killed_players
            .iter()
            .any(|player| player.user_id == 3),
        "{:?}",
        result.killed_players
    );
    assert!(game.poisoned_death_days.is_empty());
}

/// [독살] 교주·광신도·마피아팀 보조는 중독되지 않는다.
#[test]
fn poison_does_not_affect_cult_or_mafia_support() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 1, 0, vec![Role::Spy]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Doctor),
        (3, Role::Spy),
        (4, Role::CultLeader),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Poison]);

    // 교주를 치료에 막혀 처형 실패 → 중독 안 됨.
    game.mafia_targets.insert(1, 4);
    game.doctor_targets.insert(2, 4);
    game.resolve_night().unwrap();
    assert!(game.poisoned_death_days.is_empty());
}

/// [승부수] 마지막 마피아의 처형은 치료·방탄을 모두 무시하고,
/// 다른 마피아가 살아있으면 발동하지 않는다.
#[test]
fn all_in_kills_unconditionally_when_last_mafia_remains() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 2, 1, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Mafia),
        (3, Role::Doctor),
        (4, Role::Soldier),
        (5, Role::Citizen),
        (6, Role::Citizen),
        (7, Role::Citizen),
        (8, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::AllIn]);

    // 동료 마피아가 살아있으면 발동하지 않는다 (치료에 막힌다).
    game.mafia_targets.insert(1, 5);
    game.doctor_targets.insert(3, 5);
    game.resolve_night().unwrap();
    assert!(game.get_player(5).unwrap().alive);

    // 혼자 남으면 치료도 방탄도 무시한다.
    game.get_player_mut(2).unwrap().alive = false;
    game.phase = Phase::Night;
    game.day_number = 2;
    game.mafia_targets.insert(1, 4);
    game.doctor_targets.insert(3, 4);
    let result = game.resolve_night().unwrap();
    assert!(!game.get_player(4).unwrap().alive, "{result:?}");
    assert!(result.soldier_blocks.is_empty());
    assert!(result.tier_ability_results[&1].contains("[승부수]"));
}

/// [퇴마] 마피아팀이 죽인 비마피아팀 희생자가 성불된다.
#[test]
fn exorcism_purifies_non_mafia_victims() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Citizen),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Exorcism]);
    game.mafia_targets.insert(1, 3);

    let result = game.resolve_night().unwrap();
    assert!(!game.get_player(3).unwrap().alive);
    assert!(game.purified_dead_ids.contains(&3));
    assert!(
        result.tier_ability_results[&1].contains("[퇴마]"),
        "{result:?}"
    );
}

/// [확성]은 밤마다 보유자 전체에서 1회 + 인당 게임 중 1회다. 먼저 쓰면
/// 나머지는 그 밤에 못 쓰고, 사용한 본인은 게임 끝까지 다시 못 쓴다.
#[test]
fn loudspeaker_is_shared_once_per_night_and_once_per_holder() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    game.tier_abilities.clear();
    game.tier_abilities
        .insert(4, vec![TierAbility::Loudspeaker]);
    game.tier_abilities
        .insert(5, vec![TierAbility::Loudspeaker]);

    let fourth = game.get_player(4).unwrap().clone();
    let fifth = game.get_player(5).unwrap().clone();
    assert!(game.is_loudspeaker_active(&fourth));
    assert!(game.is_loudspeaker_active(&fifth));

    // 4번이 먼저 사용하면 그 밤에는 5번도(그리고 4번 본인도) 못 쓴다.
    game.mark_loudspeaker_used(4);
    assert!(!game.is_loudspeaker_active(&fourth));
    assert!(!game.is_loudspeaker_active(&fifth));

    // 다음 밤: 사용한 4번은 게임 끝까지 못 쓰고, 5번은 다시 쓸 수 있다.
    game.day_number += 1;
    assert!(!game.is_loudspeaker_active(&fourth));
    assert!(game.is_loudspeaker_active(&fifth));

    // 5번도 사용하면 이제 아무도 못 쓴다.
    game.mark_loudspeaker_used(5);
    game.day_number += 1;
    assert!(!game.is_loudspeaker_active(&fourth));
    assert!(!game.is_loudspeaker_active(&fifth));
}

/// [무법] 경찰을 노린 공격은 치료를 무시한다.
#[test]
fn lawless_pierces_doctor_protection_on_police() {
    let mut game = MafiaGame::new(basic_players(), 1, 1, 1, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.player_tiers.insert(1, 4);
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Lawless]);

    game.submit_night_action(3, Some(2)).unwrap(); // 의사가 경찰 보호
    game.submit_night_action(1, Some(2)).unwrap(); // 마피아가 경찰 공격
    let result = game.resolve_night().unwrap();

    assert!(
        result
            .killed_players
            .iter()
            .any(|player| player.user_id == 2),
        "{:?}",
        result.killed_players
    );
    assert!(
        result
            .tier_ability_results
            .get(&1)
            .is_some_and(|text| text.contains("[무법]")),
        "{:?}",
        result.tier_ability_results
    );
}

/// [무법] 경찰뿐 아니라 형사 등 경찰 계열 전체를 관통해 처형한다.
#[test]
fn lawless_pierces_protection_on_any_investigation_role() {
    let mut game = MafiaGame::new(basic_players(), 1, 1, 0, vec![Role::Inspector]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Inspector),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Lawless]);

    game.submit_night_action(3, Some(2)).unwrap();
    game.submit_night_action(1, Some(2)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(
        result
            .killed_players
            .iter()
            .any(|player| player.user_id == 2),
        "{:?}",
        result.killed_players
    );
}

/// [야습] 첫날 밤 자가 치료만 무시한다. 남이 치료해 준 경우는 못 뚫는다.
#[test]
fn night_raid_pierces_only_self_heal_on_night_one() {
    let mut game = MafiaGame::new(basic_players(), 1, 1, 1, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::NightRaid]);

    // 의사 자가 치료 → 야습이 뚫고, 의사 정체가 전체 공개된다.
    game.submit_night_action(3, Some(3)).unwrap();
    game.submit_night_action(1, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();
    assert!(
        result
            .killed_players
            .iter()
            .any(|player| player.user_id == 3),
        "{:?}",
        result.killed_players
    );
    assert!(
        result
            .night_raid_reveals
            .iter()
            .any(|player| player.user_id == 3),
        "{:?}",
        result.night_raid_reveals
    );
    assert!(game.publicly_revealed_ids.contains(&3));
}

/// [수습] 마피아팀이 죽인 시민팀의 직업이 '시민'으로 바뀌고 보유자가 원 직업을 안다.
#[test]
fn cleanup_hides_the_victims_role_and_informs_the_holder() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Cleanup]);

    game.submit_night_action(1, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(
        result
            .tier_ability_results
            .get(&1)
            .is_some_and(|text| text.contains("의사")),
        "{:?}",
        result.tier_ability_results
    );
    // 발표용 사망자 목록은 '시민'으로 가려지지만 실제 직업은 유지된다
    // (역할 기반 내부 로직이 깨지지 않도록 판정만 가린다).
    assert_eq!(
        result
            .killed_players
            .iter()
            .find(|player| player.user_id == 3)
            .map(|player| player.role),
        Some(Role::Citizen)
    );
    let victim = game.get_player(3).unwrap().clone();
    assert_eq!(victim.role, Role::Doctor);
    assert_eq!(game.visible_role(&victim), Role::Citizen);
}

/// [도주] 처형 대신 도주하고, 다음날 투표 시작 때 사망한다.
#[test]
fn escape_defers_the_execution_to_the_next_vote() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(1, vec![TierAbility::Escape]);

    // 1을 지목해 찬반 가결.
    game.phase = Phase::Day;
    game.start_vote().unwrap();
    for voter in [2, 3, 4, 5] {
        game.submit_day_vote(voter, Some(1)).unwrap();
    }
    game.resolve_nomination_vote().unwrap();
    game.start_confirmation_vote().unwrap();
    for voter in [2, 3, 4, 5] {
        game.submit_confirmation_vote(voter, true).unwrap();
    }
    let confirm = game.resolve_confirmation_vote(1).unwrap();

    assert!(confirm.executed.is_none());
    assert_eq!(
        confirm.escaped.as_ref().map(|player| player.user_id),
        Some(1)
    );
    assert!(game.get_player(1).unwrap().alive);

    // 다음날 투표 시작 → 사망.
    game.phase = Phase::Day;
    let executed = game.start_vote().unwrap();
    assert_eq!(
        executed
            .iter()
            .map(|player| player.user_id)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert!(!game.get_player(1).unwrap().alive);
    // 도주는 1회뿐 — 능력은 소모됐다.
    assert!(game.tier_abilities.get(&1).is_none());
}

/// [유언] 밤에 죽으면 유언이 공개된다. 살아있으면 공개되지 않는다.
#[test]
fn last_will_is_published_only_when_the_writer_dies_at_night() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.tier_abilities.clear();
    game.tier_abilities.insert(4, vec![TierAbility::LastWill]);

    game.submit_last_will(4, "마피아는 1번입니다").unwrap();
    // 유언 능력이 없는 사람은 작성 불가.
    assert!(game.submit_last_will(5, "테스트").is_err());

    // 첫 밤: 죽지 않음 → 공개 없음.
    game.submit_night_action(1, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();
    assert!(result.published_wills.is_empty());

    // 다음 밤: 작성자가 죽음 → 공개.
    game.phase = Phase::Night;
    game.day_number += 1;
    game.submit_night_action(1, Some(4)).unwrap();
    let result = game.resolve_night().unwrap();
    assert_eq!(
        result.published_wills,
        vec![("Four".to_string(), "마피아는 1번입니다".to_string())]
    );
}

/// [불침번] 스파이가 군인을 첩보하면 정보가 막히고 군인이 스파이의 정체를 안다.
#[test]
fn soldier_watch_blocks_spy_espionage_and_reveals_the_spy() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Spy]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Spy),
        (3, Role::Soldier),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    let reply = game.submit_night_action(2, Some(3)).unwrap();
    assert!(reply.contains("불침번"), "{reply}");
    assert!(!reply.contains("군인"), "{reply}");

    let result = game.resolve_night().unwrap();
    assert_eq!(
        result.soldier_watch_results.get(&3).map(String::as_str),
        Some("[불침번] 스파이 Two님의 첩보를 막아냈습니다.")
    );
    // 밤 결산 요약에서도 직업이 새지 않는다.
    if let Some(recap) = result.spy_results.get(&2) {
        assert!(!recap.contains("군인"), "{recap}");
    }
}

/// [불침번] 도둑이 군인에게 도벽을 쓰면 훔치지 못하고 군인이 도둑의 정체를 안다.
#[test]
fn soldier_watch_blocks_the_thief_steal() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Thief]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Thief),
        (3, Role::Soldier),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.phase = Phase::Day;
    game.start_vote().unwrap();
    game.submit_day_vote(2, Some(3)).unwrap();
    let vote_result = game.resolve_nomination_vote().unwrap();

    assert!(
        vote_result
            .thief_steal_results
            .get(&2)
            .is_some_and(|text| text.contains("불침번")),
        "{:?}",
        vote_result.thief_steal_results
    );
    assert_eq!(
        vote_result.thief_steal_results.get(&3).map(String::as_str),
        Some("[불침번] 도둑 Two님의 도벽을 막아냈습니다.")
    );
    assert!(game.thief_stolen_roles.is_empty());
    // 도벽 시도 자체는 소모된다.
    assert_eq!(game.thief_used_days.get(&2), Some(&1));
}

/// [불침번] 사기꾼이 군인을 사기 대상으로 고르면 변장이 무효가 되고, 군인은
/// 게임 시작 안내에서 사기꾼의 정체를 안다.
#[test]
fn soldier_watch_blocks_the_fraudster_disguise() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Fraudster]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Fraudster),
        (3, Role::Soldier),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.fraudster_disguises.clear();
    game.fraudster_blocked_by_soldier.clear();
    // 군인이 무작위로 뽑힌 상황을 재현한다.
    game.fraudster_blocked_by_soldier.insert(2, 3);

    let fraudster = game.get_player(2).unwrap().clone();
    assert_eq!(game.visible_role(&fraudster), Role::Fraudster);
    assert!(!game.is_disguised_fraudster(&fraudster));
    assert!(game.fraudster_disguise_info(2).is_none());
}

/// [불침번] 청부 대상에 군인이 있으면 청부 전체가 무산되고 접선도 없다.
#[test]
fn soldier_watch_voids_the_contract_naming_a_soldier() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Contractor]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Contractor),
        (3, Role::Soldier),
        (4, Role::Doctor),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.phase = Phase::Night;
    game.day_number = 2;
    game.contractor_contracts
        .insert(2, ((3, Role::Soldier), (4, Role::Doctor)));

    let result = game.resolve_night().unwrap();

    assert!(
        result
            .contractor_results
            .get(&2)
            .is_some_and(|text| text.contains("불침번")),
        "{:?}",
        result.contractor_results
    );
    assert_eq!(
        result.soldier_watch_results.get(&3).map(String::as_str),
        Some("[불침번] 청부업자 Two님의 청부를 막아냈습니다.")
    );
    assert!(result.contractor_kills.is_empty());
    assert!(!game.contractor_contacted.contains(&2));
    assert!(game.get_player(4).unwrap().alive);
}

/// 스파이는 마피아를 찾아낸 밤마다 첩보를 한 번 더 쓸 수 있다 (최초 접선에만
/// 주어지던 보너스를 매 밤으로 확장).
#[test]
fn spy_gets_a_bonus_action_every_night_a_mafia_is_found() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Spy]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Spy),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    let first = game.submit_night_action(2, Some(1)).unwrap();
    assert!(first.contains("한 번 더"), "{first}");
    assert!(first.contains("[접선]"), "{first}");
    // 보너스로 두 번째 첩보 사용 가능, 세 번째는 불가.
    game.submit_night_action(2, Some(3)).unwrap();
    assert!(game.submit_night_action(2, Some(4)).is_err());
    game.resolve_night().unwrap();

    // 다음 밤에도 마피아를 찾아내면 다시 한 번 더 쓸 수 있다.
    game.phase = Phase::Night;
    game.day_number += 1;
    let next = game.submit_night_action(2, Some(1)).unwrap();
    assert!(next.contains("한 번 더"), "{next}");
    // 이미 접선한 상태라 접선 안내는 반복되지 않는다.
    assert!(!next.contains("[접선]"), "{next}");
    assert!(game.submit_night_action(2, Some(4)).is_ok());
}

/// 사기꾼 기본 배치: 1 마피아, 2 사기꾼(3=의사로 변장), 3 의사, 4 경찰, 5 시민.
fn fraudster_test_game() -> MafiaGame {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Fraudster),
        (3, Role::Doctor),
        (4, Role::Police),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.fraudster_disguises.clear();
    game.fraudster_disguises.insert(2, (3, Role::Doctor));
    game
}

#[test]
fn fraudster_disguise_changes_role_judgment_and_deceives_investigators() {
    let mut game = fraudster_test_game();
    game.get_player_mut(4).unwrap().role = Role::Inspector;

    let fraudster = game.get_player(2).unwrap().clone();
    assert_eq!(game.visible_role(&fraudster), Role::Doctor);
    assert!(!game.is_known_mafia_team(&fraudster));

    // 형사가 사기꾼을 수사하면 변장 직업이 나오고, 사기꾼은 속임 알림을 받는다.
    game.submit_night_action(4, Some(2)).unwrap();
    let result = game.resolve_night().unwrap();
    assert_eq!(
        result.inspector_results.get(&4).map(String::as_str),
        Some("[Two님의 직업은 의사입니다.]")
    );
    assert!(
        result
            .fraudster_results
            .get(&2)
            .is_some_and(|text| text.contains("[Four님을 속였습니다.]")),
        "{:?}",
        result.fraudster_results
    );
}

#[test]
fn fraudster_deceives_the_police_team_check() {
    let mut game = fraudster_test_game();

    game.submit_night_action(4, Some(2)).unwrap();
    let result = game.resolve_night().unwrap();

    assert_eq!(result.police_target_is_mafia, Some(false));
    assert!(
        result
            .fraudster_results
            .get(&2)
            .is_some_and(|text| text.contains("[Four님을 속였습니다.]")),
        "{:?}",
        result.fraudster_results
    );
}

#[test]
fn fraudster_survives_mafia_attack_and_contacts_the_team() {
    let mut game = fraudster_test_game();

    game.submit_night_action(1, Some(2)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(game.get_player(2).unwrap().alive);
    assert!(result.killed_players.is_empty());
    assert_eq!(result.fraudster_contacts, vec![2]);
    assert!(
        result
            .fraudster_results
            .get(&2)
            .is_some_and(|text| text.contains("[교섭]")),
        "{:?}",
        result.fraudster_results
    );
    let fraudster = game.get_player(2).unwrap().clone();
    assert!(game.is_known_mafia_team(&fraudster));
}

/// 사기 대상이 표적이 되면 공격 성공 여부와 무관하게 접선한다.
#[test]
fn attack_on_the_disguise_target_contacts_the_fraudster() {
    let mut game = fraudster_test_game();

    game.submit_night_action(1, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(!game.get_player(3).unwrap().alive);
    assert_eq!(result.fraudster_contacts, vec![2]);
    let fraudster = game.get_player(2).unwrap().clone();
    assert!(game.is_known_mafia_team(&fraudster));
}

#[test]
fn fraudster_gets_a_disguise_at_game_start() {
    let players = (1..=8)
        .map(|id| (id as u64, format!("P{id}")))
        .collect::<Vec<_>>();
    let game = MafiaGame::new(players, 1, 1, 1, vec![Role::Fraudster]).unwrap();
    let fraudster = game
        .players
        .iter()
        .find(|player| player.role == Role::Fraudster)
        .unwrap();

    let (target_id, disguised_role) = game.fraudster_disguises[&fraudster.user_id];
    let target = game.get_player(target_id).unwrap();
    assert!(game.is_citizen_team(target));
    assert_eq!(target.role, disguised_role);
    assert_ne!(target_id, fraudster.user_id);
}

/// 공무원/파파라치 기본 배치: 1 마피아, 2 공무원, 3 의사, 4 파파라치, 5 시민.
fn civil_servant_test_game() -> MafiaGame {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::CivilServant),
        (3, Role::Doctor),
        (4, Role::Paparazzi),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game
}

#[test]
fn civil_servant_query_reveals_the_role_holder_and_shares_with_paparazzi() {
    let mut game = civil_servant_test_game();

    let ack = game.submit_civil_servant_query(2, Role::Doctor).unwrap();
    assert_eq!(ack, "[의사를 조회합니다.]");

    let result = game.resolve_night().unwrap();

    assert_eq!(
        result.civil_servant_results.get(&2).map(String::as_str),
        Some("[Three님이 의사로 조회되었습니다.]")
    );
    assert_eq!(
        result.paparazzi_results.get(&4).map(String::as_str),
        Some("[Three님이 의사 직업이라는 정보를 공유받았습니다.]")
    );
}

/// 사망자도 조회에 걸린다.
#[test]
fn civil_servant_query_matches_dead_players() {
    let mut game = civil_servant_test_game();
    game.mark_dead(3);

    game.submit_civil_servant_query(2, Role::Doctor).unwrap();
    let result = game.resolve_night().unwrap();

    assert_eq!(
        result.civil_servant_results.get(&2).map(String::as_str),
        Some("[Three님이 의사로 조회되었습니다.]")
    );
}

/// 기자 특종도 이슈 트리거다. 공개 발표라도 하루 몫을 소모한다.
#[test]
fn reporter_scoop_triggers_the_paparazzi_issue() {
    let mut game = civil_servant_test_game();
    game.get_player_mut(2).unwrap().role = Role::Reporter;
    game.day_number = 2;

    game.reporter_targets.insert(2, 3);
    let result = game.resolve_night().unwrap();

    assert!(result.reporter_results.contains_key(&2));
    assert_eq!(
        result.paparazzi_results.get(&4).map(String::as_str),
        Some("[Three님이 의사 직업이라는 정보를 공유받았습니다.]")
    );
}

/// 기자가 자신을 특종한 경우는 "다른 사람의 직업"이 아니므로 트리거가 아니다.
#[test]
fn reporter_self_scoop_does_not_trigger_the_issue() {
    let mut game = civil_servant_test_game();
    game.get_player_mut(2).unwrap().role = Role::Reporter;
    game.day_number = 2;

    game.reporter_targets.insert(2, 2);
    let result = game.resolve_night().unwrap();

    assert!(result.paparazzi_results.is_empty());
}

#[test]
fn civil_servant_query_without_holder_consumes_the_night_use() {
    let mut game = civil_servant_test_game();

    game.submit_civil_servant_query(2, Role::Prophet).unwrap();
    // 같은 밤에는 다시 시도할 수 없다.
    assert!(game.submit_civil_servant_query(2, Role::Doctor).is_err());

    let result = game.resolve_night().unwrap();
    assert_eq!(
        result.civil_servant_results.get(&2).map(String::as_str),
        Some("[해당 직업을 보유한 플레이어가 없습니다.]")
    );
    // 알아낸 직업이 없으므로 파파라치에게도 공유되지 않는다.
    assert!(result.paparazzi_results.is_empty());

    // 다음 밤에는 다시 조회할 수 있다.
    game.phase = Phase::Night;
    game.day_number += 1;
    assert!(game.submit_civil_servant_query(2, Role::Doctor).is_ok());
}

#[test]
fn civil_servant_cannot_query_police_lineage_or_citizen() {
    let mut game = civil_servant_test_game();

    for role in [
        Role::Police,
        Role::Agent,
        Role::Vigilante,
        Role::Inspector,
        Role::Citizen,
        Role::Mafia,
        Role::CivilServant,
    ] {
        assert!(
            game.submit_civil_servant_query(2, role).is_err(),
            "{role:?} must not be queryable"
        );
    }
}

#[test]
fn paparazzi_shares_only_the_first_reveal_and_only_once_per_day() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
            (6, "Six".to_string()),
        ],
        1,
        0,
        0,
        Vec::new(),
    )
    .unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::CivilServant),
        (3, Role::Doctor),
        (4, Role::Paparazzi),
        (5, Role::Inspector),
        (6, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    // 같은 밤에 공무원 조회(의사)와 형사 수사(시민)가 함께 성공해도
    // 공유되는 것은 우선순위가 높은 조회 결과 하나뿐이다.
    game.submit_civil_servant_query(2, Role::Doctor).unwrap();
    game.submit_night_action(5, Some(6)).unwrap();
    let result = game.resolve_night().unwrap();

    let shared = result.paparazzi_results.get(&4).unwrap();
    assert!(shared.contains("Three"), "{shared}");
    assert!(shared.contains("의사"), "{shared}");
    assert!(!shared.contains("Six"), "{shared}");

    // 같은 날에는 두 번 공유되지 않았고, 다음 날에는 다시 공유된다.
    game.phase = Phase::Night;
    game.day_number += 1;
    game.submit_civil_servant_query(2, Role::Paparazzi).unwrap();
    let next_result = game.resolve_night().unwrap();
    let next_shared = next_result.paparazzi_results.get(&4).unwrap();
    assert!(next_shared.contains("파파라치"), "{next_shared}");
}

/// 실제 게임 순서 재현: 낮 1 해킹 → (해킹 결과는 밤 2 시작에 전달) → 밤 2 조회.
/// 해킹 공유의 하루 몫은 해킹이 일어난 날(1일)에서 차감돼야 하고, 밤 2의 조회
/// 공유(2일 몫)를 막으면 안 된다.
#[test]
fn day_hack_share_does_not_consume_the_next_days_issue() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
            (6, "Six".to_string()),
        ],
        1,
        0,
        0,
        Vec::new(),
    )
    .unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::CivilServant),
        (3, Role::Doctor),
        (4, Role::Paparazzi),
        (5, Role::Hacker),
        (6, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    // 밤 1: 아무 조사도 없이 지나간다.
    game.resolve_night().unwrap();
    // 낮 1: 해커가 해킹한다.
    game.submit_hacker_action(5, 6).unwrap();
    // 투표가 끝나 다음 밤으로 넘어간다 (day_number 1 → 2).
    game.start_vote().unwrap();
    game.resolve_nomination_vote().unwrap();
    assert_eq!(game.day_number, 2);

    // 밤 2 시작: 해킹 결과가 전달되고, 공유는 1일 몫으로 처리된다.
    let hacker_results = game.consume_hacker_results();
    let hack_share = hacker_results.get(&4).unwrap();
    assert!(hack_share.contains("Six"), "{hack_share}");
    assert!(game.paparazzi_shared_days.contains(&1));
    assert!(!game.paparazzi_shared_days.contains(&2));

    // 밤 2의 조회 공유는 2일 몫으로 정상 동작해야 한다.
    game.submit_civil_servant_query(2, Role::Doctor).unwrap();
    let result = game.resolve_night().unwrap();
    let night_share = result.paparazzi_results.get(&4).unwrap();
    assert!(night_share.contains("의사"), "{night_share}");
}

/// 밤 1 조사가 먼저 공유되면 같은 날(1일) 낮 해킹은 이미 몫을 쓴 뒤라 공유되지
/// 않는다 — "하루 중 가장 먼저 알아낸 정보만".
#[test]
fn night_share_beats_the_same_days_hack() {
    let mut game = civil_servant_test_game();
    game.get_player_mut(5).unwrap().role = Role::Hacker;

    game.submit_civil_servant_query(2, Role::Doctor).unwrap();
    let result = game.resolve_night().unwrap();
    assert!(result.paparazzi_results.contains_key(&4));

    game.submit_hacker_action(5, 3).unwrap();
    game.start_vote().unwrap();
    game.resolve_nomination_vote().unwrap();
    let hacker_results = game.consume_hacker_results();
    assert!(hacker_results.contains_key(&5));
    assert!(!hacker_results.contains_key(&4), "{hacker_results:?}");
}

#[test]
fn paparazzi_is_not_triggered_by_team_only_information() {
    let mut game = civil_servant_test_game();
    game.get_player_mut(2).unwrap().role = Role::Police;

    // 경찰 조사는 마피아 여부(팀)만 알아내므로 이슈가 발동하지 않는다.
    game.submit_night_action(2, Some(1)).unwrap();
    let result = game.resolve_night().unwrap();

    assert_eq!(result.police_target_is_mafia, Some(true));
    assert!(result.paparazzi_results.is_empty());
}

/// 도둑(마피아팀)이 훔친 능력으로 알아낸 정보는 "시민팀이 알아낸 정보"가
/// 아니므로 파파라치에게 공유되지 않는다.
#[test]
fn paparazzi_ignores_reveals_made_by_the_mafia_team() {
    let mut game = civil_servant_test_game();
    game.get_player_mut(2).unwrap().role = Role::Thief;
    game.thief_stolen_roles.insert(2, Role::CivilServant);

    game.submit_civil_servant_query(2, Role::Doctor).unwrap();
    let result = game.resolve_night().unwrap();

    assert_eq!(
        result.civil_servant_results.get(&2).map(String::as_str),
        Some("[Three님이 의사로 조회되었습니다.]")
    );
    assert!(result.paparazzi_results.is_empty());
}

#[test]
fn inspector_reveals_same_team_role_and_notifies_target() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Inspector),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(2, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();

    assert_eq!(
        result.inspector_results.get(&2).map(String::as_str),
        Some("[Three님의 직업은 의사입니다.]")
    );
    assert_eq!(
        result.inspector_target_notices.get(&3).map(String::as_str),
        Some("[형사 Two님이 당신을 수사했습니다.]")
    );
}

/// 경찰은 대상을 고른 즉시(밤이 끝나기 전) 자기 선택에 대한 결과를 볼 수 있어야
/// 하고, 대상을 바꾸면 바꾼 대상의 결과가 나와야 한다.
#[test]
fn police_result_is_available_as_soon_as_a_target_is_chosen() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    assert_eq!(game.police_result_for_actor(2), None);

    game.submit_night_action(2, Some(1)).unwrap();
    let mafia_result = game.police_result_for_actor(2).unwrap();
    assert!(mafia_result.contains("One"), "{mafia_result}");
    assert!(mafia_result.contains("마피아팀입니다"), "{mafia_result}");
}

/// 결과가 즉시 나오므로 같은 밤에 대상을 바꾸면 연속 조사가 된다. 첫 제출로
/// 고정하고, 다음 밤에는 다시 조사할 수 있다.
#[test]
fn police_investigation_locks_after_the_first_submission() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 1, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(2, Some(1)).unwrap();
    let error = game.submit_night_action(2, Some(3)).unwrap_err();
    assert!(error.to_string().contains("이미 이번 밤"), "{error}");
    // 결과는 첫 대상 그대로다.
    let result = game.police_result_for_actor(2).unwrap();
    assert!(result.contains("One"), "{result}");
    // 잠긴 행동은 변경 가능 목록에 없어야 밤 조기 종료가 막히지 않는다.
    let police = game.get_player(2).unwrap().clone();
    assert!(!game.night_action_can_be_changed(&police));

    game.resolve_night().unwrap();
    game.phase = Phase::Night;
    game.day_number += 1;
    assert!(game.submit_night_action(2, Some(3)).is_ok());
}

#[test]
fn inspector_receives_result_when_target_dies_the_same_night() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Inspector),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(2, Some(3)).unwrap();
    game.submit_night_action(1, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(
        result
            .killed_players
            .iter()
            .any(|player| player.user_id == 3)
    );
    assert_eq!(
        result.inspector_results.get(&2).map(String::as_str),
        Some("[Three님의 직업은 의사입니다.]")
    );
    assert!(!result.inspector_target_notices.contains_key(&3));
}

#[test]
fn inspector_investigation_is_single_use_per_game() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Inspector),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    // 경찰 계열 공통 규칙: 결과가 제출 즉시 나오므로 대상을 바꿀 수 없다.
    let immediate = game.submit_night_action(2, Some(3)).unwrap();
    assert!(
        immediate.contains("[Three님의 직업은 의사입니다.]"),
        "{immediate}"
    );
    assert!(game.inspector_used_ids.contains(&2));
    let error = game.submit_night_action(2, Some(4)).unwrap_err();
    assert!(error.to_string().contains("한 번만"), "{error}");
    // 밤 종료 시에도 결과 기록(리플레이/대상 알림)은 그대로 남는다.
    assert!(
        game.resolve_night()
            .unwrap()
            .inspector_results
            .contains_key(&2)
    );

    game.phase = Phase::Night;
    assert!(
        !game
            .night_action_actors()
            .iter()
            .any(|actor| actor.user_id == 2)
    );
    assert!(game.submit_night_action(2, Some(3)).is_err());
}

/// 다른 팀 수사는 즉시 "시민팀이 아닙니다"만 나오고 1회용은 소모된다.
#[test]
fn inspector_gets_an_immediate_no_result_for_another_team() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Inspector),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    let immediate = game.submit_night_action(2, Some(1)).unwrap();
    assert!(
        immediate.contains("[One님은 시민팀이 아닙니다.]"),
        "{immediate}"
    );
    assert!(game.inspector_used_ids.contains(&2));
}

/// 형사는 접선 여부와 무관하게 실제 소속으로 판정한다: 접선 전 마피아 보조나
/// 교주팀도 "시민팀이 아닙니다"가 나오고, 대상에게 알림이 가지 않는다.
#[test]
fn inspector_judges_by_real_team_without_notifying_the_target() {
    for enemy_role in [Role::Spy, Role::CultLeader] {
        let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
        for (id, role) in [
            (1, Role::Mafia),
            (2, Role::Inspector),
            (3, enemy_role),
            (4, Role::Citizen),
            (5, Role::Citizen),
        ] {
            game.get_player_mut(id).unwrap().role = role;
        }
        assert!(!game.is_known_mafia_team(game.get_player(3).unwrap()));

        let immediate = game.submit_night_action(2, Some(3)).unwrap();
        assert!(
            immediate.contains("[Three님은 시민팀이 아닙니다.]"),
            "{enemy_role:?}: {immediate}"
        );

        let result = game.resolve_night().unwrap();
        assert!(!result.inspector_results.contains_key(&2));
        assert!(!result.inspector_target_notices.contains_key(&3));
    }
}

/// 자경단원 숙청 조사도 제출 즉시 결과가 나온다.
#[test]
fn vigilante_investigation_returns_the_result_immediately() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Vigilante]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Vigilante),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.phase = Phase::Day;

    let citizen_result = game.submit_vigilante_investigation(2, 3).unwrap();
    assert!(
        citizen_result.contains("[숙청] Three 님은 **마피아팀이 아닙니다**."),
        "{citizen_result}"
    );
    // 게임 중 1회라 재조사는 막힌다.
    assert!(game.submit_vigilante_investigation(2, 1).is_err());
}

/// 다른 팀을 수사하면 결과가 없지만 1회용은 그대로 소모된다.
#[test]
fn inspector_single_use_is_consumed_even_without_a_result() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Inspector),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(2, Some(1)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(!result.inspector_results.contains_key(&2));
    assert!(game.inspector_used_ids.contains(&2));
}

#[test]
fn inspector_does_not_reveal_or_notify_other_team() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Inspector]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Inspector),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(2, Some(1)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(!result.inspector_results.contains_key(&2));
    assert!(!result.inspector_target_notices.contains_key(&1));
}

#[test]
fn public_status_lists_alive_and_dead_players() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    game.get_player_mut(2).unwrap().alive = false;
    let status = game.public_status();
    assert!(status.contains("1일차 / 현재 단계: 밤"));
    assert!(status.contains("생존자(4명)"));
    assert!(status.contains("사망자: Two"));
}

#[test]
fn stolen_terrorist_retaliates_against_citizen_team_when_thief_dies_at_night() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Thief),
        (3, Role::Citizen),
        (4, Role::CultLeader),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.thief_stolen_roles.insert(2, Role::Terrorist);
    game.terrorist_targets.insert(2, 3);
    game.mafia_targets.insert(1, 2);

    let result = game.resolve_night().unwrap();

    assert!(!game.get_player(2).unwrap().alive);
    assert!(!game.get_player(3).unwrap().alive);
    assert!(
        result
            .terrorist_retaliations
            .iter()
            .any(|(terrorist, target)| terrorist.user_id == 2 && target.user_id == 3)
    );
}

#[test]
fn terrorist_retaliates_against_cult_team_when_citizen_team_terrorist_dies() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Terrorist),
        (3, Role::CultLeader),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.terrorist_targets.insert(2, 3);
    game.mafia_targets.insert(1, 2);

    let result = game.resolve_night().unwrap();

    assert!(!game.get_player(2).unwrap().alive);
    assert!(!game.get_player(3).unwrap().alive);
    assert!(
        result
            .terrorist_retaliations
            .iter()
            .any(|(terrorist, target)| terrorist.user_id == 2 && target.user_id == 3)
    );
}

#[test]
fn terrorist_does_not_retaliate_against_same_team_target() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Terrorist),
        (3, Role::Citizen),
        (4, Role::CultLeader),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.terrorist_targets.insert(2, 3);
    game.mafia_targets.insert(1, 2);

    let result = game.resolve_night().unwrap();

    assert!(!game.get_player(2).unwrap().alive);
    assert!(game.get_player(3).unwrap().alive);
    assert!(result.terrorist_retaliations.is_empty());
}

#[test]
fn stolen_terrorist_retaliates_when_thief_is_executed() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Thief),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.phase = Phase::FinalDefense;
    game.thief_stolen_roles.insert(2, Role::Terrorist);
    game.begin_terrorist_final_defense(2);
    game.submit_terrorist_final_defense_target(2, 3).unwrap();
    game.start_confirmation_vote().unwrap();
    game.confirm_votes.insert(1, true);

    let result = game.resolve_confirmation_vote(2).unwrap();

    assert_eq!(
        result.executed.as_ref().map(|player| player.user_id),
        Some(2)
    );
    assert!(result.extra_killed.iter().any(|player| player.user_id == 3));
    assert!(!game.get_player(2).unwrap().alive);
    assert!(!game.get_player(3).unwrap().alive);
}

#[test]
fn terrorist_night_target_is_not_reused_when_executed_by_vote() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Terrorist),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.phase = Phase::ConfirmVote;
    game.terrorist_targets.insert(2, 1);
    game.confirm_votes.insert(3, true);

    let result = game.resolve_confirmation_vote(2).unwrap();

    assert_eq!(
        result.executed.as_ref().map(|player| player.user_id),
        Some(2)
    );
    assert!(result.extra_killed.is_empty());
    assert!(game.get_player(1).unwrap().alive);
}

#[test]
fn terrorist_attacks_mafia_selected_during_final_defense() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Terrorist),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.phase = Phase::FinalDefense;

    let targets = game.begin_terrorist_final_defense(2);
    assert!(targets.iter().any(|player| player.user_id == 1));
    assert_eq!(
        game.submit_terrorist_final_defense_target(2, 1).unwrap(),
        "습격 대상: One"
    );
    game.start_confirmation_vote().unwrap();
    game.confirm_votes.insert(3, true);

    let result = game.resolve_confirmation_vote(2).unwrap();

    assert!(result.extra_killed.iter().any(|player| player.user_id == 1));
    assert!(!game.get_player(1).unwrap().alive);
}

#[test]
fn terrorist_attacks_only_contacted_mafia_support_during_execution() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Terrorist),
        (3, Role::Spy),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.spy_contacted.insert(3);
    game.phase = Phase::FinalDefense;
    game.begin_terrorist_final_defense(2);
    game.submit_terrorist_final_defense_target(2, 3).unwrap();
    game.start_confirmation_vote().unwrap();
    game.confirm_votes.insert(4, true);

    let result = game.resolve_confirmation_vote(2).unwrap();

    assert!(result.extra_killed.iter().any(|player| player.user_id == 3));
    assert!(!game.get_player(3).unwrap().alive);
}

#[test]
fn terrorist_does_not_attack_uncontacted_mafia_support_during_execution() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Terrorist),
        (3, Role::Spy),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.phase = Phase::FinalDefense;
    game.begin_terrorist_final_defense(2);
    game.submit_terrorist_final_defense_target(2, 3).unwrap();
    game.start_confirmation_vote().unwrap();
    game.confirm_votes.insert(4, true);

    let result = game.resolve_confirmation_vote(2).unwrap();

    assert!(result.extra_killed.is_empty());
    assert!(game.get_player(3).unwrap().alive);
}

#[test]
fn mark_dead_reports_a_player_once() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();

    assert_eq!(game.mark_dead(1).map(|player| player.user_id), Some(1));
    assert!(game.mark_dead(1).is_none());
    assert_eq!(game.death_order, vec![1]);
}

#[test]
fn mark_dead_removes_stale_vote_state() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    game.phase = Phase::Day;
    game.start_vote().unwrap();
    game.day_votes.insert(1, Some(2));
    game.day_votes.insert(3, Some(2));
    game.day_votes.insert(4, None);
    game.confirm_votes.insert(1, true);
    game.confirm_votes.insert(4, false);

    game.mark_dead(1).unwrap();
    game.mark_dead(2).unwrap();

    assert!(!game.day_votes.contains_key(&1));
    assert!(!game.day_votes.values().any(|target| *target == Some(2)));
    assert_eq!(game.current_vote_counts().get(&2), None);
    assert_eq!(game.current_skip_vote_count(), 1);
    assert_eq!(game.current_confirm_counts(), (0, 1));
}

#[test]
fn confirmation_vote_executes_at_half_or_more_yes() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    game.phase = Phase::FinalDefense;
    game.start_confirmation_vote().unwrap();

    for voter_id in [1, 2, 3] {
        game.submit_confirmation_vote(voter_id, true).unwrap();
    }
    for voter_id in [4, 5] {
        game.submit_confirmation_vote(voter_id, false).unwrap();
    }

    let result = game.resolve_confirmation_vote(5).unwrap();

    assert!(result.approved);
    assert_eq!(result.executed.unwrap().user_id, 5);
}

#[test]
fn gangster_vote_block_does_not_change_confirmation_majority() {
    let players = (1..=7)
        .map(|id| (id, format!("Player {id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 0, 0, Vec::new()).unwrap();
    game.get_player_mut(7).unwrap().role = Role::Citizen;
    game.phase = Phase::ConfirmVote;
    game.gangster_blocked_vote_days.insert(6, game.day_number);

    for voter_id in [1, 2, 3] {
        game.submit_confirmation_vote(voter_id, true).unwrap();
    }
    for voter_id in [4, 5, 6] {
        game.submit_confirmation_vote(voter_id, false).unwrap();
    }

    let result = game.resolve_confirmation_vote(7).unwrap();

    assert!(!result.approved);
    assert!(result.tied);
    assert!(result.executed.is_none());
    assert_eq!(result.vote_counts.get(&true).copied(), Some(3));
    assert_eq!(result.vote_counts.get(&false).copied(), Some(3));
    assert_eq!(result.weighted_vote_counts.get(&true).copied(), Some(3));
    assert_eq!(result.weighted_vote_counts.get(&false).copied(), Some(3));
}

#[test]
fn politician_vote_displays_one_but_counts_as_two_for_nomination() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    game.get_player_mut(1).unwrap().role = Role::Politician;
    game.phase = Phase::Vote;

    game.submit_day_vote(1, Some(2)).unwrap();
    game.submit_day_vote(3, Some(4)).unwrap();

    let result = game.resolve_nomination_vote().unwrap();

    assert_eq!(
        result.executed.as_ref().map(|player| player.user_id),
        Some(2)
    );
    assert_eq!(result.vote_counts.get(&Some(2)).copied(), Some(1));
    assert_eq!(result.weighted_vote_counts.get(&Some(2)).copied(), Some(2));
}

#[test]
fn politician_does_not_weight_confirmation_vote() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    game.get_player_mut(1).unwrap().role = Role::Politician;
    game.get_player_mut(2).unwrap().role = Role::Citizen;
    game.phase = Phase::ConfirmVote;

    game.submit_confirmation_vote(1, true).unwrap();
    game.submit_confirmation_vote(3, false).unwrap();

    let result = game.resolve_confirmation_vote(2).unwrap();

    assert!(!result.approved);
    assert!(result.tied);
    assert!(result.executed.is_none());
    assert_eq!(result.vote_counts.get(&true).copied(), Some(1));
    assert_eq!(result.vote_counts.get(&false).copied(), Some(1));
    assert_eq!(result.weighted_vote_counts.get(&true).copied(), Some(1));
    assert_eq!(result.weighted_vote_counts.get(&false).copied(), Some(1));
}

fn mercenary_test_game() -> MafiaGame {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Mercenary),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.mercenary_client_ids.clear();
    game.assign_mercenary_clients();
    game
}

#[test]
fn mercenary_client_is_citizen_team_player() {
    let game = mercenary_test_game();
    let client = game.mercenary_client(2).unwrap();

    assert_ne!(client.user_id, 2);
    assert!(game.is_citizen_team(client));
}

#[test]
fn mercenary_arms_when_client_dies_first_night() {
    let mut game = mercenary_test_game();
    let mafia_id = 1;
    let client = game.mercenary_client(2).unwrap().clone();
    let client_id = client.user_id;

    game.submit_night_action(mafia_id, Some(client_id)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(result.killed_players.iter().any(|p| p.user_id == client_id));
    assert!(game.mercenary_armed_ids.contains(&2));
    assert!(game.mercenary_contract_received_ids.contains(&2));
    assert_eq!(
        result.mercenary_results.get(&2).map(String::as_str),
        Some("[의뢰] 의뢰인이 사망했습니다. 이제 밤마다 플레이어 한 명을 처형할 수 있습니다.")
    );
    assert!(!result.mercenary_results[&2].contains(&client.name));
}

#[test]
fn mercenary_arms_after_contracted_client_dies_at_night() {
    let mut game = mercenary_test_game();
    let mafia_id = 1;
    let client = game.mercenary_client(2).unwrap().clone();
    let client_id = client.user_id;
    assert_eq!(game.receive_mercenary_contracts().len(), 1);
    game.phase = Phase::Night;
    game.day_number = 2;

    game.submit_night_action(mafia_id, Some(client_id)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(result.killed_players.iter().any(|p| p.user_id == client_id));
    assert!(game.mercenary_armed_ids.contains(&2));
    assert_eq!(
        result.mercenary_results.get(&2).map(String::as_str),
        Some("[의뢰] 의뢰인이 사망했습니다. 이제 밤마다 플레이어 한 명을 처형할 수 있습니다.")
    );
    assert!(!result.mercenary_results[&2].contains(&client.name));
}

#[test]
fn armed_mercenary_blocks_mafia_majority_win() {
    let mut game = mercenary_test_game();
    for id in [3, 4, 5] {
        game.get_player_mut(id).unwrap().alive = false;
    }

    assert_eq!(game.winner(), Some(Winner::Mafia));
    game.mercenary_armed_ids.insert(2);
    assert_eq!(game.winner(), None);
}

#[test]
fn mercenary_executes_independently_at_night() {
    let mut game = mercenary_test_game();
    game.mercenary_armed_ids.insert(2);

    game.submit_night_action(2, Some(1)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(result.mercenary_kills.iter().any(|p| p.user_id == 1));
    assert!(result.killed_players.iter().any(|p| p.user_id == 1));
}

#[test]
fn mercenary_kill_is_canceled_when_mercenary_dies_same_night() {
    let mut game = mercenary_test_game();
    game.mercenary_armed_ids.insert(2);

    game.submit_night_action(1, Some(2)).unwrap();
    game.submit_night_action(2, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(result.killed_players.iter().any(|p| p.user_id == 2));
    assert!(!result.killed_players.iter().any(|p| p.user_id == 3));
    assert!(result.mercenary_kills.is_empty());
    assert!(!result.mercenary_results.contains_key(&2));
    assert!(game.get_player(3).unwrap().alive);
}

/// 조사는 제출 즉시 성립하므로, 경찰이 같은 밤에 죽어도 결과는 남는다.
#[test]
fn police_result_stands_when_police_dies_same_night() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Police),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(1, Some(2)).unwrap();
    game.submit_night_action(2, Some(1)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(result.killed_players.iter().any(|p| p.user_id == 2));
    assert_eq!(
        result.police_target.as_ref().map(|player| player.user_id),
        Some(1)
    );
    assert_eq!(result.police_target_is_mafia, Some(true));
}

#[test]
fn doctor_protection_is_canceled_when_doctor_dies_same_night() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Doctor),
        (3, Role::Citizen),
        (4, Role::Godfather),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game.godfather_contacted.insert(4);

    game.submit_night_action(1, Some(2)).unwrap();
    game.submit_night_action(2, Some(3)).unwrap();
    game.submit_night_action(4, Some(3)).unwrap();
    let result = game.resolve_night().unwrap();

    assert!(result.killed_players.iter().any(|p| p.user_id == 2));
    assert!(result.killed_players.iter().any(|p| p.user_id == 3));
    assert!(result.protected.is_none());
}

#[test]
fn doctor_can_change_night_target_before_morning() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Doctor),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(2, Some(3)).unwrap();
    game.submit_night_action(2, Some(4)).unwrap();
    game.submit_night_action(1, Some(3)).unwrap();

    assert_eq!(game.doctor_targets.get(&2), Some(&4));
    assert!(!game.should_finish_night_early());

    let result = game.resolve_night().unwrap();

    assert_eq!(result.protected.unwrap().user_id, 4);
    assert!(
        result
            .killed_players
            .iter()
            .any(|player| player.user_id == 3)
    );
}

#[test]
fn vigilante_can_change_execution_target_before_morning() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Vigilante),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(2, Some(3)).unwrap();
    game.submit_night_action(2, Some(1)).unwrap();

    assert_eq!(game.vigilante_targets.get(&2), Some(&1));
    assert!(!game.vigilante_execution_used_ids.contains(&2));

    let result = game.resolve_night().unwrap();

    assert!(
        result
            .vigilante_kills
            .iter()
            .any(|player| player.user_id == 1)
    );
    assert!(game.vigilante_execution_used_ids.contains(&2));
}

#[test]
fn cult_leader_change_does_not_convert_previous_target() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::CultLeader),
        (3, Role::Citizen),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.submit_night_action(2, Some(3)).unwrap();
    game.submit_night_action(2, Some(4)).unwrap();

    assert!(!game.culted_ids.contains(&3));
    assert!(!game.culted_ids.contains(&4));

    let result = game.resolve_night().unwrap();

    assert!(!game.culted_ids.contains(&3));
    assert!(game.culted_ids.contains(&4));
    assert_eq!(result.cult_bells, 1);
}

fn hypnotist_test_game() -> MafiaGame {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Hypnotist),
        (3, Role::Doctor),
        (4, Role::CultLeader),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }
    game
}

#[test]
fn hypnotist_accumulates_targets_until_wake() {
    let mut game = hypnotist_test_game();

    game.submit_night_action(2, Some(1)).unwrap();
    game.resolve_night().unwrap();
    assert!(
        game.hypnotized_targets
            .get(&2)
            .is_some_and(|targets| targets.contains(&1))
    );

    game.advance_to_next_night();
    game.submit_night_action(2, Some(3)).unwrap();
    game.resolve_night().unwrap();

    let result = game.submit_hypnotist_wake(2).unwrap();
    assert!(result.contains("One님 : 마피아"));
    assert!(result.contains("Three님 : 시민팀"));
    assert!(!game.hypnotized_targets.contains_key(&2));
}

#[test]
fn hypnotist_wake_blocks_next_night_action() {
    let mut game = hypnotist_test_game();

    game.submit_night_action(2, Some(4)).unwrap();
    game.resolve_night().unwrap();
    let result = game.submit_hypnotist_wake(2).unwrap();

    assert!(result.contains("Four님 : 교주"));
    game.advance_to_next_night();
    assert!(
        !game
            .night_action_actors()
            .iter()
            .any(|player| player.user_id == 2)
    );
}

#[test]
fn stolen_police_result_is_independent_from_police_vote() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
            (6, "Six".to_string()),
        ],
        1,
        0,
        1,
        vec![Role::Thief],
    )
    .unwrap();
    let thief_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Thief)
        .unwrap()
        .user_id;
    let police_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Police)
        .unwrap()
        .user_id;
    let targets = game
        .players
        .iter()
        .filter(|player| player.user_id != thief_id && player.user_id != police_id)
        .take(2)
        .map(|player| (player.user_id, player.name.clone()))
        .collect::<Vec<_>>();
    let (police_target_id, police_target_name) = targets[0].clone();
    let (thief_target_id, thief_target_name) = targets[1].clone();

    game.phase = Phase::Day;
    game.start_vote().unwrap();
    let vote_message = game.submit_day_vote(thief_id, Some(police_id)).unwrap();
    assert!(vote_message.contains("투표 대상"));
    assert!(vote_message.contains("[도벽]"));
    // 훔친 직업은 투표 응답이 아니라 투표 결산 후에야 알려준다.
    assert!(!vote_message.contains("경찰"), "{vote_message}");
    let vote_result = game.resolve_nomination_vote().unwrap();
    assert!(
        vote_result
            .thief_steal_results
            .get(&thief_id)
            .is_some_and(|text| text.contains("경찰")),
        "{:?}",
        vote_result.thief_steal_results
    );
    game.phase = Phase::Night;
    game.submit_night_action(police_id, Some(police_target_id))
        .unwrap();
    game.submit_night_action(thief_id, Some(thief_target_id))
        .unwrap();

    assert_eq!(game.police_targets.get(&police_id), Some(&police_target_id));
    assert!(!game.police_targets.contains_key(&thief_id));
    assert_eq!(
        game.thief_police_targets.get(&thief_id),
        Some(&thief_target_id)
    );
    assert_eq!(
        game.get_night_action_target(police_id),
        Some(police_target_id)
    );
    assert_eq!(
        game.get_night_action_target(thief_id),
        Some(thief_target_id)
    );
    assert!(
        game.police_result_for_actor(thief_id)
            .unwrap()
            .contains(&thief_target_name)
    );
    assert!(
        !game
            .police_result_for_actor(thief_id)
            .unwrap()
            .contains(&police_target_name)
    );

    let result = game.resolve_night().unwrap();

    assert_eq!(result.police_target.unwrap().user_id, police_target_id);
    let thief_result = result.thief_police_results.get(&thief_id).unwrap();
    assert!(thief_result.contains(&thief_target_name));
    assert!(!thief_result.contains(&police_target_name));
}

#[test]
fn thief_stealing_vigilante_can_act_at_night() {
    let mut game = MafiaGame::new_with_counts(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ],
        GameCounts {
            mafia_count: 1,
            vigilante_count: 1,
            special_roles: vec![Role::Thief],
            ..Default::default()
        },
    )
    .unwrap();
    let thief_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Thief)
        .unwrap()
        .user_id;
    let vigilante_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Vigilante)
        .unwrap()
        .user_id;
    let target_id = game
        .players
        .iter()
        .find(|player| player.user_id != thief_id && player.user_id != vigilante_id)
        .unwrap()
        .user_id;

    game.phase = Phase::Day;
    game.start_vote().unwrap();
    let vote_message = game.submit_day_vote(thief_id, Some(vigilante_id)).unwrap();
    assert!(!vote_message.contains("자경단원"), "{vote_message}");
    let vote_result = game.resolve_nomination_vote().unwrap();
    assert!(
        vote_result
            .thief_steal_results
            .get(&thief_id)
            .is_some_and(|text| text.contains("자경단원")),
        "{:?}",
        vote_result.thief_steal_results
    );

    game.phase = Phase::Night;
    assert!(
        game.night_action_actors()
            .iter()
            .any(|player| player.user_id == thief_id)
    );
    let action_message = game.submit_night_action(thief_id, Some(target_id)).unwrap();

    assert!(action_message.contains("[도벽: 자경단원]"));
    assert_eq!(game.vigilante_targets.get(&thief_id), Some(&target_id));
}

#[test]
fn thief_stealing_mafia_contacts_and_can_attack() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ],
        1,
        0,
        0,
        vec![Role::Thief],
    )
    .unwrap();
    let mafia_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Mafia)
        .unwrap()
        .user_id;
    let thief_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Thief)
        .unwrap()
        .user_id;
    let target_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Citizen)
        .unwrap()
        .user_id;

    game.phase = Phase::Day;
    game.start_vote().unwrap();
    let vote_message = game.submit_day_vote(thief_id, Some(mafia_id)).unwrap();
    // 접선도 투표 결산 시점에 이뤄진다.
    assert!(!vote_message.contains("접선"), "{vote_message}");
    assert!(!game.thief_contacted.contains(&thief_id));
    let vote_result = game.resolve_nomination_vote().unwrap();
    let thief = game.get_player(thief_id).unwrap().clone();

    assert!(
        vote_result
            .thief_steal_results
            .get(&thief_id)
            .is_some_and(|text| text.contains("마피아팀과 접선했습니다")),
        "{:?}",
        vote_result.thief_steal_results
    );
    assert_eq!(
        vote_result
            .thief_newly_contacted
            .iter()
            .map(|player| player.user_id)
            .collect::<Vec<_>>(),
        vec![thief_id]
    );
    assert!(game.thief_contacted.contains(&thief_id));
    assert!(game.is_known_mafia_team(&thief));
    assert_eq!(game.thief_night_role(&thief), Some(Role::Mafia));

    game.phase = Phase::Night;
    assert!(
        game.night_action_actors()
            .iter()
            .any(|player| player.user_id == thief_id)
    );
    assert!(game.submit_night_action(thief_id, Some(target_id)).is_ok());
    assert_eq!(game.mafia_targets.get(&thief_id), Some(&target_id));
}

/// 투표를 바꿔가며 여러 명의 직업을 알아내는 것을 막는다: 훔치는 대상은 마지막
/// 지목 하나뿐이고, 결과도 결산 때 한 번만 나온다.
#[test]
fn thief_steal_follows_only_the_final_vote_target() {
    let mut game = MafiaGame::new(basic_players(), 1, 1, 0, vec![Role::Thief]).unwrap();
    for (id, role) in [
        (1, Role::Mafia),
        (2, Role::Thief),
        (3, Role::Doctor),
        (4, Role::Citizen),
        (5, Role::Citizen),
    ] {
        game.get_player_mut(id).unwrap().role = role;
    }

    game.phase = Phase::Day;
    game.start_vote().unwrap();
    let first = game.submit_day_vote(2, Some(3)).unwrap();
    assert!(!first.contains("의사"), "{first}");
    let second = game.submit_day_vote(2, Some(1)).unwrap();
    assert!(!second.contains("마피아"), "{second}");

    let vote_result = game.resolve_nomination_vote().unwrap();

    // 마지막 지목(마피아)만 훔쳤고, 결과도 하나뿐이다.
    assert_eq!(vote_result.thief_steal_results.len(), 1);
    assert!(
        vote_result
            .thief_steal_results
            .get(&2)
            .is_some_and(|text| text.contains("One") && !text.contains("의사")),
        "{:?}",
        vote_result.thief_steal_results
    );
    let thief = game.get_player(2).unwrap().clone();
    assert_eq!(game.thief_night_role(&thief), Some(Role::Mafia));
}

#[test]
fn police_does_not_detect_uncontacted_spy_as_mafia_team() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ],
        1,
        0,
        1,
        vec![Role::Spy],
    )
    .unwrap();
    let police_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Police)
        .unwrap()
        .user_id;
    let spy_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Spy)
        .unwrap()
        .user_id;

    game.submit_night_action(police_id, Some(spy_id)).unwrap();

    assert!(game.police_result_ready());
    assert_eq!(game.current_police_result().1, Some(false));
    assert_eq!(
        game.resolve_night().unwrap().police_target_is_mafia,
        Some(false)
    );
}

#[test]
fn police_detects_contacted_spy_as_mafia_team() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ],
        1,
        0,
        1,
        vec![Role::Spy],
    )
    .unwrap();
    let police_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Police)
        .unwrap()
        .user_id;
    let spy_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Spy)
        .unwrap()
        .user_id;
    game.spy_contacted.insert(spy_id);

    game.submit_night_action(police_id, Some(spy_id)).unwrap();

    assert!(game.police_result_ready());
    assert_eq!(game.current_police_result().1, Some(true));
    assert_eq!(
        game.resolve_night().unwrap().police_target_is_mafia,
        Some(true)
    );
}

#[test]
fn psychologist_treats_uncontacted_spy_and_citizen_as_same_team() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ],
        1,
        0,
        0,
        vec![Role::Psychologist, Role::Spy],
    )
    .unwrap();
    game.phase = Phase::Day;
    let psychologist_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Psychologist)
        .unwrap()
        .user_id;
    let spy = game
        .players
        .iter()
        .find(|player| player.role == Role::Spy)
        .unwrap()
        .clone();
    let citizen = game
        .players
        .iter()
        .find(|player| player.role == Role::Citizen)
        .unwrap()
        .clone();

    assert_eq!(game.team_key(&spy), game.team_key(&citizen));
    assert!(
        game.submit_psychologist_observation(psychologist_id, spy.user_id, citizen.user_id)
            .is_ok()
    );
}

#[test]
fn psychologist_treats_contacted_spy_and_citizen_as_different_team() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ],
        1,
        0,
        0,
        vec![Role::Psychologist, Role::Spy],
    )
    .unwrap();
    let spy = game
        .players
        .iter()
        .find(|player| player.role == Role::Spy)
        .unwrap()
        .clone();
    let citizen = game
        .players
        .iter()
        .find(|player| player.role == Role::Citizen)
        .unwrap()
        .clone();
    game.spy_contacted.insert(spy.user_id);

    assert_ne!(game.team_key(&spy), game.team_key(&citizen));
}

#[test]
fn vigilante_does_not_execute_uncontacted_spy() {
    let mut game = MafiaGame::new_with_counts(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ],
        GameCounts {
            mafia_count: 1,
            vigilante_count: 1,
            special_roles: vec![Role::Spy],
            ..Default::default()
        },
    )
    .unwrap();
    let vigilante_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Vigilante)
        .unwrap()
        .user_id;
    let spy_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Spy)
        .unwrap()
        .user_id;

    game.submit_night_action(vigilante_id, Some(spy_id))
        .unwrap();
    let result = game.resolve_night().unwrap();

    assert!(result.vigilante_kills.is_empty());
    assert!(game.get_player(spy_id).unwrap().alive);
}

#[test]
fn vigilante_executes_contacted_spy() {
    let mut game = MafiaGame::new_with_counts(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ],
        GameCounts {
            mafia_count: 1,
            vigilante_count: 1,
            special_roles: vec![Role::Spy],
            ..Default::default()
        },
    )
    .unwrap();
    let vigilante_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Vigilante)
        .unwrap()
        .user_id;
    let spy_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Spy)
        .unwrap()
        .user_id;
    game.spy_contacted.insert(spy_id);

    game.submit_night_action(vigilante_id, Some(spy_id))
        .unwrap();
    let result = game.resolve_night().unwrap();

    assert_eq!(
        result
            .vigilante_kills
            .iter()
            .map(|player| player.user_id)
            .collect::<Vec<_>>(),
        vec![spy_id]
    );
    assert!(!game.get_player(spy_id).unwrap().alive);
}

#[test]
fn police_does_not_detect_uncontacted_witch_as_mafia_team() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ],
        1,
        0,
        1,
        vec![Role::Witch],
    )
    .unwrap();
    let police_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Police)
        .unwrap()
        .user_id;
    let witch_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Witch)
        .unwrap()
        .user_id;

    game.submit_night_action(police_id, Some(witch_id)).unwrap();

    assert!(game.police_result_ready());
    assert_eq!(game.current_police_result().1, Some(false));
    assert_eq!(
        game.resolve_night().unwrap().police_target_is_mafia,
        Some(false)
    );
}

#[test]
fn police_detects_contacted_witch_as_mafia_team() {
    let mut game = MafiaGame::new(
        vec![
            (1, "One".to_string()),
            (2, "Two".to_string()),
            (3, "Three".to_string()),
            (4, "Four".to_string()),
            (5, "Five".to_string()),
        ],
        1,
        0,
        1,
        vec![Role::Witch],
    )
    .unwrap();
    let police_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Police)
        .unwrap()
        .user_id;
    let witch_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Witch)
        .unwrap()
        .user_id;
    game.witch_contacted.insert(witch_id);

    game.submit_night_action(police_id, Some(witch_id)).unwrap();

    assert!(game.police_result_ready());
    assert_eq!(game.current_police_result().1, Some(true));
    assert_eq!(
        game.resolve_night().unwrap().police_target_is_mafia,
        Some(true)
    );
}

#[test]
fn citizen_wins_when_known_mafia_dead() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, Vec::new()).unwrap();
    let mafia_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Mafia)
        .unwrap()
        .user_id;
    game.get_player_mut(mafia_id).unwrap().alive = false;
    assert_eq!(game.winner(), Some(Winner::Citizen));
}

#[test]
fn doctor_blocks_mafia_majority_attack() {
    let mut game = MafiaGame::new(basic_players(), 1, 1, 0, Vec::new()).unwrap();
    let mafia = game
        .players
        .iter()
        .find(|p| p.role == Role::Mafia)
        .unwrap()
        .user_id;
    let doctor = game
        .players
        .iter()
        .find(|p| p.role == Role::Doctor)
        .unwrap()
        .user_id;
    let target = game
        .players
        .iter()
        .find(|p| p.role == Role::Citizen)
        .unwrap()
        .user_id;
    game.submit_night_action(mafia, Some(target)).unwrap();
    game.submit_night_action(doctor, Some(target)).unwrap();
    let result = game.resolve_night().unwrap();
    assert!(result.killed.is_none());
    assert_eq!(result.protected.unwrap().user_id, target);
    let events = game.rating_events.get(&doctor).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.points == 5 && event.reason.contains("치료 성공"))
    );
}

#[test]
fn single_submitted_mafia_attack_resolves_even_if_other_mafia_waits() {
    let mut game = MafiaGame::new(basic_players(), 2, 0, 0, Vec::new()).unwrap();
    let mafia = game
        .players
        .iter()
        .filter(|player| player.role == Role::Mafia)
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    let target = game
        .players
        .iter()
        .find(|player| player.role == Role::Citizen)
        .unwrap()
        .user_id;

    game.submit_night_action(mafia[0], Some(target)).unwrap();
    let result = game.resolve_night().unwrap();

    assert_eq!(result.killed.unwrap().user_id, target);
}

#[test]
fn split_submitted_mafia_attacks_do_not_resolve() {
    let mut game = MafiaGame::new(basic_players(), 2, 0, 0, Vec::new()).unwrap();
    let mafia = game
        .players
        .iter()
        .filter(|player| player.role == Role::Mafia)
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    let targets = game
        .players
        .iter()
        .filter(|player| player.role == Role::Citizen)
        .map(|player| player.user_id)
        .take(2)
        .collect::<Vec<_>>();

    game.submit_night_action(mafia[0], Some(targets[0]))
        .unwrap();
    game.submit_night_action(mafia[1], Some(targets[1]))
        .unwrap();
    let result = game.resolve_night().unwrap();

    assert!(result.killed.is_none());
}

#[test]
fn madam_seduction_lasts_until_following_vote_ends() {
    let mut game = MafiaGame::new(basic_players(), 1, 1, 0, vec![Role::Madam]).unwrap();
    let madam_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Madam)
        .unwrap()
        .user_id;
    let doctor_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Doctor)
        .unwrap()
        .user_id;

    game.phase = Phase::Day;
    game.start_vote().unwrap();
    game.submit_day_vote(madam_id, Some(doctor_id)).unwrap();
    let other_voter_ids = game
        .alive_players()
        .into_iter()
        .filter(|player| player.user_id != madam_id)
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    for voter_id in other_voter_ids {
        game.submit_day_vote(voter_id, None).unwrap();
    }
    game.resolve_nomination_vote().unwrap();
    assert!(game.madam_seduced_ids.contains(&doctor_id));
    assert!(
        !game
            .night_action_actors()
            .iter()
            .any(|player| player.user_id == doctor_id)
    );

    game.resolve_night().unwrap();
    assert!(game.madam_seduced_ids.contains(&doctor_id));

    game.start_vote().unwrap();
    let voter_ids = game
        .alive_players()
        .into_iter()
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    for voter_id in voter_ids {
        game.submit_day_vote(voter_id, None).unwrap();
    }
    game.resolve_nomination_vote().unwrap();
    assert!(!game.madam_seduced_ids.contains(&doctor_id));
    assert!(!game.madam_seduction_release_days.contains_key(&doctor_id));
    assert!(
        game.night_action_actors()
            .iter()
            .any(|player| player.user_id == doctor_id)
    );
    assert!(game.submit_night_action(doctor_id, Some(madam_id)).is_ok());
}

#[test]
fn dead_madam_vote_does_not_seduce() {
    let mut game = MafiaGame::new(basic_players(), 1, 1, 0, vec![Role::Madam]).unwrap();
    let madam_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Madam)
        .unwrap()
        .user_id;
    let doctor_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Doctor)
        .unwrap()
        .user_id;

    game.phase = Phase::Day;
    game.start_vote().unwrap();
    game.submit_day_vote(madam_id, Some(doctor_id)).unwrap();
    game.mark_dead(madam_id).unwrap();
    for voter_id in game
        .alive_players()
        .into_iter()
        .map(|player| player.user_id)
        .collect::<Vec<_>>()
    {
        game.submit_day_vote(voter_id, None).unwrap();
    }

    let result = game.resolve_nomination_vote().unwrap();

    assert!(result.madam_seduced.is_empty());
    assert!(!game.madam_seduced_ids.contains(&doctor_id));
}

#[test]
fn madam_cannot_vote_for_herself() {
    let mut game = MafiaGame::new(basic_players(), 1, 0, 0, vec![Role::Madam]).unwrap();
    let madam_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Madam)
        .unwrap()
        .user_id;

    game.phase = Phase::Day;
    game.start_vote().unwrap();
    let error = game.submit_day_vote(madam_id, Some(madam_id)).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("마담은 자기 자신에게 투표할 수 없습니다.")
    );
    assert!(!game.day_votes.contains_key(&madam_id));
}
