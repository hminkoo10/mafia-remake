// casino 엔진 테스트 (noir-casino의 규칙 테스트를 옮긴 것)

use super::*;
use rand::RngCore;

fn cards(list: &[&str]) -> Vec<String> {
    list.iter().map(|card| card.to_string()).collect()
}

/// `draw`는 끝에서 뽑으므로, 나열한 순서대로 나오게 뒤집어 둔다.
fn deck_from_top(list: &[&str]) -> Vec<String> {
    let mut deck = cards(list);
    deck.reverse();
    deck
}

fn holdem_table() -> CasinoTable {
    CasinoTable::new("t-holdem", GameKind::Holdem, "Test Hold'em", 1, 0)
}

fn blackjack_table() -> CasinoTable {
    CasinoTable::new("t-bj", GameKind::Blackjack, "Test Blackjack", 1, 0)
}

fn sit(table: &mut CasinoTable, user: u64, seat: usize, amount: i64, now: i64) -> Vec<CasinoEvent> {
    table
        .apply_command(
            user,
            &format!("P{user}"),
            &CasinoCommand::Join {
                seat,
                amount,
                name: format!("P{user}"),
            },
            None,
            now,
        )
        .unwrap()
}

fn act(table: &mut CasinoTable, user: u64, command: CasinoCommand, now: i64) -> Vec<CasinoEvent> {
    let version = table.version;
    table
        .apply_command(user, &format!("P{user}"), &command, Some(version), now)
        .unwrap_or_else(|error| panic!("{command:?} by {user} failed: {error}"))
}

fn stacks(table: &CasinoTable) -> i64 {
    table.seats.iter().flatten().map(|seat| seat.stack).sum()
}

fn committed(table: &CasinoTable) -> i64 {
    table.seats.iter().flatten().map(|seat| seat.total).sum()
}

#[test]
fn poker_rank_orders_categories_and_names() {
    let royal = poker_rank(&cards(&["As", "Ks", "Qs", "Js", "Ts", "2d", "3c"])).unwrap();
    assert_eq!(royal.name, "로열 플러시");
    let wheel = poker_rank(&cards(&["Ah", "2d", "3c", "4s", "5h"])).unwrap();
    assert_eq!(wheel.name, "스트레이트");
    let six_high = poker_rank(&cards(&["2h", "3d", "4c", "5s", "6h"])).unwrap();
    assert!(six_high.score > wheel.score);
    let full_house = poker_rank(&cards(&["Kh", "Kd", "Kc", "2s", "2h"])).unwrap();
    let flush = poker_rank(&cards(&["Ah", "9h", "7h", "4h", "2h"])).unwrap();
    assert_eq!(full_house.name, "풀 하우스");
    assert_eq!(flush.name, "플러시");
    assert!(full_house.score > flush.score);
    let quads = poker_rank(&cards(&["7h", "7d", "7c", "7s", "2h"])).unwrap();
    assert!(quads.score > full_house.score);
    let pair_high_kicker = poker_rank(&cards(&["Ah", "Ad", "9c", "7s", "2h"])).unwrap();
    let pair_low_kicker = poker_rank(&cards(&["Ah", "Ad", "8c", "7s", "2h"])).unwrap();
    assert!(pair_high_kicker.score > pair_low_kicker.score);
    let two_pair = poker_rank(&cards(&["Ah", "Ad", "8c", "8s", "2h"])).unwrap();
    assert_eq!(two_pair.name, "투 페어");
    assert!(poker_rank(&cards(&["Ah", "Ad"])).is_err());
}

#[test]
fn best_hand_reports_name_and_core_cards() {
    let pair = best_hand(&cards(&["Ah", "Ad"])).unwrap();
    assert_eq!(pair.name, "원 페어");
    assert_eq!(pair.cards.len(), 2);
    let high = best_hand(&cards(&["7d", "Ah"])).unwrap();
    assert_eq!(high.name, "하이 카드");
    assert_eq!(high.cards, cards(&["Ah"]));
    let trips = best_hand(&cards(&["9h", "9d", "9c", "2s", "Kd", "4h", "7c"])).unwrap();
    assert_eq!(trips.name, "트리플");
    let mut core = trips.cards.clone();
    core.sort();
    let mut expected = cards(&["9h", "9d", "9c"]);
    expected.sort();
    assert_eq!(core, expected);
    let straight = best_hand(&cards(&["5h", "6d", "7c", "8s", "9d", "Kh", "2c"])).unwrap();
    assert_eq!(straight.name, "스트레이트");
    assert_eq!(straight.cards.len(), 5);
    assert!(!straight.cards.contains(&"Kh".to_string()));
    let two_pair = best_hand(&cards(&["Qh", "Qd", "3c", "3s", "Ad"])).unwrap();
    assert_eq!(two_pair.name, "투 페어");
    assert_eq!(two_pair.cards.len(), 4);
    assert!(!two_pair.cards.contains(&"Ad".to_string()));
    assert!(best_hand(&cards(&["Ah"])).is_none());
}

#[test]
fn dealer_rotation_only_uses_dealers_with_portraits() {
    let mut table = blackjack_table();
    sit(&mut table, 70, 0, 10_000, 0);
    let first = table.dealer.clone();
    assert_eq!(table.dealer_profile().name, "소피아");
    for hand in 0..(DEALER_SHIFT_HANDS * 2) {
        let now = hand as i64 * 100_000;
        act(&mut table, 70, CasinoCommand::Start, now);
        table.tick(now + BET_WINDOW_MS + 1).unwrap();
        assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    }
    let with_portrait = DEALERS.iter().filter(|dealer| dealer.has_portrait).count();
    if with_portrait < 2 {
        assert_eq!(table.dealer, first, "초상이 하나뿐이면 교대하지 않는다");
    } else {
        assert_ne!(
            table.dealer, first,
            "정해진 판 수 뒤에는 다른 딜러로 바뀐다"
        );
        assert!(dealer_profile(&table.dealer).has_portrait);
    }
    let dealer_names = DEALERS.iter().map(|dealer| dealer.name).collect::<Vec<_>>();
    assert!(
        table
            .messages
            .iter()
            .filter(|message| message.dealer)
            .all(|message| dealer_names.contains(&message.name.as_str())),
        "딜러 안내는 항상 명단에 있는 딜러 이름으로 남는다"
    );
}

#[test]
fn side_bet_tables_follow_evolution_payouts() {
    assert_eq!(perfect_pairs("8h", "8d"), Some(("컬러 페어", 12)));
    assert_eq!(perfect_pairs("8h", "8s"), Some(("믹스 페어", 6)));
    assert_eq!(perfect_pairs("8h", "8h"), Some(("퍼펙트 페어", 25)));
    assert_eq!(perfect_pairs("8h", "9h"), None);
    assert_eq!(
        twenty_one_plus_three("8h", "8d", "8s"),
        Some(("트리플", 30))
    );
    assert_eq!(
        twenty_one_plus_three("8h", "8h", "8h"),
        Some(("수티드 트립스", 100))
    );
    assert_eq!(
        twenty_one_plus_three("5h", "6h", "7h"),
        Some(("스트레이트 플러시", 40))
    );
    assert_eq!(
        twenty_one_plus_three("5h", "6d", "7h"),
        Some(("스트레이트", 10))
    );
    assert_eq!(
        twenty_one_plus_three("Ah", "2d", "3c"),
        Some(("스트레이트", 10))
    );
    assert_eq!(
        twenty_one_plus_three("Qh", "Kd", "Ac"),
        Some(("스트레이트", 10))
    );
    assert_eq!(twenty_one_plus_three("2h", "9h", "Kh"), Some(("플러시", 5)));
    assert_eq!(twenty_one_plus_three("2h", "9d", "Kc"), None);
}

#[test]
fn blackjack_side_bets_are_settled_on_the_deal() {
    let mut table = blackjack_table();
    sit(&mut table, 80, 0, 10_000, 0);
    // 딜 순서: 내 첫 장, 딜러 첫 장, 내 둘째 장, 딜러 둘째 장, 그다음 딜러 드로우.
    table
        .start_with_deck(80, deck_from_top(&["8h", "8s", "8d", "5c", "Tc"]), 0)
        .unwrap();
    act(
        &mut table,
        80,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 100,
            plus3: 100,
        },
        100,
    );
    // 컬러 페어 12:1 (+1,200), 21+3 트리플 30:1 (+3,000)이 딜 직후 스택에 얹힌다.
    let seat = table.seat(0).unwrap();
    assert_eq!(seat.stack, 10_000 - 700 + 1_300 + 3_100);
    assert_eq!(seat.side_net, 4_200);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Playing);
    act(&mut table, 80, CasinoCommand::Stand, 200);
    let result = &table.history[0].results[0];
    assert_eq!(result.wagered, 700);
    // 딜러 13 → Tc로 23 버스트: 메인 +500.
    assert_eq!(result.net, 500 + 4_200);
    assert!(
        result
            .notes
            .iter()
            .any(|note| note.contains("컬러 페어 12:1"))
    );
    assert!(result.notes.iter().any(|note| note.contains("트리플 30:1")));
    assert_eq!(table.seat(0).unwrap().stack, 10_000 + 500 + 4_200);
}

#[test]
fn insurance_pays_two_to_one_against_dealer_blackjack() {
    let mut table = blackjack_table();
    sit(&mut table, 81, 0, 10_000, 0);
    table
        .start_with_deck(81, deck_from_top(&["9h", "As", "7d", "Kc"]), 0)
        .unwrap();
    act(
        &mut table,
        81,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 0,
            plus3: 0,
        },
        100,
    );
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Insurance);
    let stale = table.apply_command(81, "P81", &CasinoCommand::Hit, None, 150);
    assert!(stale.is_err(), "인슈어런스 중에는 플레이 액션이 막힌다");
    act(&mut table, 81, CasinoCommand::Insure { accept: true }, 200);
    // 혼자라서 바로 확인: 딜러 블랙잭 → 메인 -500, 인슈어런스 +500 = 0.
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    let result = &table.history[0].results[0];
    assert_eq!(result.wagered, 750);
    assert_eq!(result.net, 0);
    assert!(
        result
            .notes
            .iter()
            .any(|note| note.contains("인슈어런스 2:1"))
    );
    assert_eq!(table.seat(0).unwrap().stack, 10_000);
}

#[test]
fn declined_or_timed_out_insurance_costs_nothing_and_play_continues() {
    let mut table = blackjack_table();
    sit(&mut table, 82, 0, 10_000, 0);
    sit(&mut table, 83, 1, 10_000, 0);
    // 82: 9h 7d, 83: 6c 5s, 딜러: As 5c (블랙잭 아님), 그다음 드로우 Tc.
    table
        .start_with_deck(
            82,
            deck_from_top(&["9h", "6c", "As", "7d", "5s", "5c", "Tc", "9d"]),
            0,
        )
        .unwrap();
    act(
        &mut table,
        82,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 0,
            plus3: 0,
        },
        100,
    );
    act(
        &mut table,
        83,
        CasinoCommand::Bet {
            amount: 300,
            pairs: 0,
            plus3: 0,
        },
        200,
    );
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Insurance);
    act(&mut table, 82, CasinoCommand::Insure { accept: true }, 300);
    // 83은 정하지 않는다 → 시간이 지나면 거절로 처리하고 플레이가 이어진다.
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Insurance);
    table.tick(300 + INSURANCE_MS + 1).unwrap();
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Playing);
    assert_eq!(table.seat(0).unwrap().insurance, 250);
    assert_eq!(table.seat(1).unwrap().insurance, 0);
    assert!(table.seat(1).unwrap().insurance_decided);
    let stand_at = 300 + INSURANCE_MS + 2;
    act(&mut table, 82, CasinoCommand::Stand, stand_at);
    act(&mut table, 83, CasinoCommand::Stand, stand_at + 1);
    // 딜러 16 → Tc로 26 버스트: 둘 다 메인 승리. 82는 인슈어런스 250을 잃는다.
    let results = &table.history[0].results;
    let first = results.iter().find(|entry| entry.seat == 0).unwrap();
    let second = results.iter().find(|entry| entry.seat == 1).unwrap();
    assert_eq!(first.net, 500 - 250);
    assert_eq!(first.wagered, 750);
    assert_eq!(second.net, 300);
    assert!(
        first
            .notes
            .iter()
            .any(|note| note.contains("인슈어런스 -250"))
    );
}

#[test]
fn blackjack_value_handles_soft_aces() {
    assert_eq!(blackjack_value(&cards(&["Ah", "6d"])), (17, true));
    assert_eq!(blackjack_value(&cards(&["Ah", "6d", "Tc"])), (17, false));
    assert_eq!(blackjack_value(&cards(&["Ah", "Ad", "9c"])), (21, true));
    assert_eq!(blackjack_value(&cards(&["Th", "Jd", "Qc"])), (30, false));
    assert_eq!(card_value("Ah"), 11);
    assert_eq!(card_value("Kh"), 10);
    assert_eq!(card_value("7h"), 7);
}

#[test]
fn shuffled_deck_has_every_card_once() {
    let deck = shuffled_deck(1);
    assert_eq!(deck.len(), 52);
    let unique = deck.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(unique.len(), 52);
    assert_eq!(shuffled_deck(6).len(), 312);
}

#[test]
fn holdem_three_players_play_a_full_hand_to_showdown() {
    let mut table = holdem_table();
    for (user, seat) in [(10, 0), (11, 1), (12, 2)] {
        sit(&mut table, user, seat, 10_000, 0);
    }
    // 딜 순서: 버튼(0) 다음부터 → 1, 2, 0, 1, 2, 0. 그다음 번·플롭·번·턴·번·리버.
    let deck = deck_from_top(&[
        "Kh", "2c", "Ah", "Kd", "7d", "Ad", "8c", "As", "Kc", "5h", "8d", "9s", "8h", "3d",
    ]);
    table.start_with_deck(10, deck, 1_000).unwrap();
    assert_eq!(table.button, 0);
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.phase, Phase::Preflop);
    assert_eq!(round.turn, 0, "UTG는 버튼(3인)");
    assert_eq!(table.seat(1).unwrap().bet, 50);
    assert_eq!(table.seat(2).unwrap().bet, 100);
    assert_eq!(table.seat(0).unwrap().cards, cards(&["Ah", "Ad"]));

    // 오래된 버전으로는 거부한다.
    let stale = table.apply_command(10, "P10", &CasinoCommand::Call, Some(0), 1_000);
    assert_eq!(stale.unwrap_err().code, "STALE_STATE");

    let legal = table.poker_legal_for(0).unwrap();
    assert_eq!(legal.to_call, 100);
    assert!(!legal.can_check && legal.can_raise);
    act(&mut table, 10, CasinoCommand::Call, 1_100);
    assert_eq!(table.poker_legal_for(1).unwrap().to_call, 50);
    act(&mut table, 11, CasinoCommand::Call, 1_200);
    assert!(table.poker_legal_for(2).unwrap().can_check);
    act(&mut table, 12, CasinoCommand::Check, 1_300);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Flop);
    assert_eq!(
        table.round.as_ref().unwrap().board,
        cards(&["As", "Kc", "5h"])
    );
    assert_eq!(committed(&table), 300);

    for (step, phase) in [Phase::Turn, Phase::River].into_iter().enumerate() {
        let at = 2_000 + step as i64 * 1_000;
        // 플롭부터는 버튼 다음(1)부터 시작한다.
        assert_eq!(table.round.as_ref().unwrap().turn, 1);
        act(&mut table, 11, CasinoCommand::Check, at);
        act(&mut table, 12, CasinoCommand::Check, at + 100);
        act(&mut table, 10, CasinoCommand::Check, at + 200);
        assert_eq!(table.round.as_ref().unwrap().phase, phase);
    }
    act(&mut table, 11, CasinoCommand::Check, 4_000);
    act(&mut table, 12, CasinoCommand::Check, 4_100);
    let events = act(&mut table, 10, CasinoCommand::Check, 4_200);

    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert_eq!(table.seat(0).unwrap().stack, 10_200);
    assert_eq!(table.seat(1).unwrap().stack, 9_900);
    assert_eq!(table.seat(2).unwrap().stack, 9_900);
    assert_eq!(stacks(&table), 30_000);
    let result = &table.history[0];
    assert_eq!(result.payouts.len(), 1);
    assert_eq!(result.payouts[0].name, "P10");
    assert_eq!(result.payouts[0].amount, 300);
    assert_eq!(result.payouts[0].label, "트리플");
    // 손익: 승자는 팟에서 자기 몫을 뺀 만큼 따고, 나머지는 건 만큼 잃는다.
    let net: i64 = result.results.iter().map(|entry| entry.net).sum();
    assert_eq!(net, 0, "손익 합은 0이어야 한다: {:?}", result.results);
    let winner = result
        .results
        .iter()
        .find(|entry| entry.name == "P10")
        .unwrap();
    assert_eq!(winner.net, 300 - winner.wagered);
    assert_eq!(winner.label, "트리플");
    assert!(
        result
            .results
            .iter()
            .filter(|entry| entry.name != "P10")
            .all(|entry| entry.net == -entry.wagered)
    );
    assert!(matches!(
        events.as_slice(),
        [CasinoEvent::RoundSettled { house_delta: 0, .. }]
    ));
    // 쇼다운이면 폴드하지 않은 카드가 공개된다.
    let spectator = table_view(&table, None, i64::MAX);
    assert_eq!(
        spectator.seats[1].as_ref().unwrap().cards,
        cards(&["Kh", "Kd"])
    );
}

#[test]
fn holdem_side_pot_pays_short_stack_from_main_pot_only() {
    let mut table = holdem_table();
    for (user, seat) in [(20, 0), (21, 1), (22, 2)] {
        sit(&mut table, user, seat, 10_000, 0);
    }
    table.seats[0].as_mut().unwrap().stack = 300;
    // A(0): 에이스 페어, B(1): 퀸 페어, C(2): 킹 페어. 보드는 아무도 돕지 않는다.
    let deck = deck_from_top(&[
        "Qh", "Kh", "Ah", "Qd", "Kd", "Ad", "8c", "9s", "5h", "2d", "8d", "7c", "8h", "3s",
    ]);
    table.start_with_deck(20, deck, 0).unwrap();
    let legal = table.poker_legal_for(0).unwrap();
    assert_eq!((legal.min_raise_to, legal.max_raise_to), (200, 300));
    act(&mut table, 20, CasinoCommand::Raise { amount: 300 }, 100);
    assert_eq!(table.seat(0).unwrap().stack, 0);
    act(&mut table, 21, CasinoCommand::Call, 200);
    act(&mut table, 22, CasinoCommand::Call, 300);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Flop);
    assert_eq!(table.round.as_ref().unwrap().turn, 1);
    act(&mut table, 21, CasinoCommand::Raise { amount: 500 }, 400);
    act(&mut table, 22, CasinoCommand::Call, 500);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Turn);
    act(&mut table, 21, CasinoCommand::Check, 600);
    act(&mut table, 22, CasinoCommand::Check, 700);
    act(&mut table, 21, CasinoCommand::Check, 800);
    act(&mut table, 22, CasinoCommand::Check, 900);

    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert_eq!(table.seat(0).unwrap().stack, 900, "메인팟 900만 받는다");
    assert_eq!(table.seat(1).unwrap().stack, 9_200);
    assert_eq!(
        table.seat(2).unwrap().stack,
        10_200,
        "사이드팟 1,000은 C의 몫"
    );
    assert_eq!(stacks(&table), 20_300);
    let payouts = &table.history[0].payouts;
    assert!(
        payouts
            .iter()
            .any(|payout| payout.name == "P20" && payout.amount == 900)
    );
    assert!(
        payouts
            .iter()
            .any(|payout| payout.name == "P22" && payout.amount == 1_000)
    );
}

#[test]
fn holdem_random_rounds_conserve_chips() {
    let mut table = holdem_table();
    for (user, seat) in [(30, 0), (31, 1), (32, 3), (33, 5)] {
        sit(&mut table, user, seat, 10_000, 0);
    }
    let mut rng = crate::system_random::rng();
    let mut now = 0;
    let mut expected = 40_000;
    let mut rounds = 0;
    while rounds < 120 {
        now += 1_000;
        if !table.playing() {
            // 파산자는 다시 채워 넣고 기대 총액을 맞춘다.
            for seat in table.seats.iter_mut().flatten() {
                if seat.stack == 0 {
                    seat.stack = 10_000;
                    expected += 10_000;
                }
                seat.sit_out = false;
                seat.missed = 0;
            }
            let starter = table.seats.iter().flatten().next().unwrap().user_id;
            act(&mut table, starter, CasinoCommand::Start, now);
            rounds += 1;
        }
        let mut guard = 0;
        while table.playing() {
            guard += 1;
            assert!(guard < 500, "라운드가 끝나지 않습니다");
            now += 1_000;
            if rng.next_u64() % 10 == 0 {
                // 가끔 시간 초과로 처리한다.
                let deadline = table.round.as_ref().unwrap().deadline;
                now = now.max(deadline + 1);
                table.tick(now).unwrap();
            } else {
                let turn = table.round.as_ref().unwrap().turn;
                let actor = table.seat(turn).unwrap().user_id;
                let legal = table.poker_legal_for(turn).unwrap();
                let roll = rng.next_u64() % 10;
                let command = if roll < 2 {
                    CasinoCommand::Fold
                } else if roll < 7 || !legal.can_raise {
                    if legal.can_check {
                        CasinoCommand::Check
                    } else {
                        CasinoCommand::Call
                    }
                } else {
                    let low = legal.min_raise_to.min(legal.max_raise_to);
                    let span = (legal.max_raise_to - low).max(0) as u64 + 1;
                    let amount = low + (rng.next_u64() % span) as i64;
                    let amount = if amount >= legal.min_raise_to {
                        amount
                    } else {
                        legal.max_raise_to
                    };
                    CasinoCommand::Raise { amount }
                };
                act(&mut table, actor, command, now);
            }
            assert_eq!(
                stacks(&table)
                    + if table.playing() {
                        committed(&table)
                    } else {
                        0
                    },
                expected,
                "칩 보존"
            );
        }
        assert_eq!(stacks(&table), expected);
    }
}

fn deal_blackjack_with(
    table: &mut CasinoTable,
    starter: u64,
    bets: &[(u64, i64)],
    deck: &[&str],
    now: i64,
) -> i64 {
    table
        .start_with_deck(starter, deck_from_top(deck), now)
        .unwrap();
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Betting);
    for (user, amount) in bets {
        act(
            table,
            *user,
            CasinoCommand::Bet {
                amount: *amount,
                pairs: 0,
                plus3: 0,
            },
            now + 100,
        );
    }
    let deal_at = now + BET_WINDOW_MS + 1;
    table.tick(deal_at).unwrap();
    deal_at
}

#[test]
fn blackjack_pays_natural_three_to_two_and_wins_against_bust() {
    let mut table = blackjack_table();
    sit(&mut table, 40, 0, 10_000, 0);
    sit(&mut table, 41, 1, 10_000, 0);
    // 딜 순서: P0, P1, 딜러, P0, P1, 딜러. 그다음 P1 히트, 딜러 드로.
    let now = deal_blackjack_with(
        &mut table,
        40,
        &[(40, 100), (41, 500)],
        &["Ah", "9c", "Th", "Kd", "7d", "6s", "5c", "Td"],
        0,
    );
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.phase, Phase::Playing);
    assert_eq!(round.turn, 1, "내추럴은 자동 스탠드라 P1 차례");
    assert_eq!(table.seat(0).unwrap().hands[0].status, HandStatus::Stand);
    // 딜러 홀 카드는 공개 전에는 가려진다.
    let view = table_view(&table, Some(41), i64::MAX);
    assert_eq!(view.round.as_ref().unwrap().dealer, cards(&["Th", "??"]));
    assert!(view.legal.blackjack.is_some());

    let events = act(&mut table, 41, CasinoCommand::Hit, now + 1_000);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert_eq!(
        table.round.as_ref().unwrap().dealer,
        cards(&["Th", "6s", "Td"])
    );
    assert_eq!(table.seat(0).unwrap().stack, 10_150);
    assert_eq!(table.seat(1).unwrap().stack, 10_500);
    assert_eq!(
        table.seat(0).unwrap().hands[0].result.as_deref(),
        Some("블랙잭 3:2")
    );
    assert_eq!(
        table.seat(1).unwrap().hands[0].result.as_deref(),
        Some("승리")
    );
    assert!(matches!(
        events.as_slice(),
        [CasinoEvent::RoundSettled {
            house_delta: -650,
            ..
        }]
    ));
    assert!(table.history[0].summary.starts_with("딜러 버스트"));
}

#[test]
fn blackjack_split_then_double_each_hand_settles_separately() {
    let mut table = blackjack_table();
    sit(&mut table, 50, 0, 10_000, 0);
    // 딜: P0 8h, 딜러 5c, P0 8d, 딜러 9s. 스플릿: 3h, Tc. 더블: 9d. 딜러 드로: Kc.
    let now = deal_blackjack_with(
        &mut table,
        50,
        &[(50, 200)],
        &["8h", "5c", "8d", "9s", "3h", "Tc", "9d", "Kc"],
        0,
    );
    let legal = table.blackjack_legal_for(0).unwrap();
    assert!(legal.can_split && legal.can_double && !legal.can_surrender);
    act(&mut table, 50, CasinoCommand::Split, now + 100);
    let seat = table.seat(0).unwrap();
    assert_eq!(seat.hands.len(), 2);
    assert_eq!(seat.hands[0].cards, cards(&["8h", "3h"]));
    assert_eq!(seat.hands[1].cards, cards(&["8d", "Tc"]));
    assert_eq!(seat.stack, 10_000 - 400);
    let legal = table.blackjack_legal_for(0).unwrap();
    assert!(
        legal.can_double && !legal.can_surrender,
        "스플릿 후 서렌더 불가"
    );
    act(&mut table, 50, CasinoCommand::Double, now + 200);
    let seat = table.seat(0).unwrap();
    assert_eq!(seat.hands[0].bet, 400);
    assert_eq!(seat.hands[0].status, HandStatus::Stand);
    assert_eq!(table.round.as_ref().unwrap().hand, 1);
    let events = act(&mut table, 50, CasinoCommand::Stand, now + 300);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert_eq!(table.seat(0).unwrap().stack, 10_600);
    assert!(matches!(
        events.as_slice(),
        [CasinoEvent::RoundSettled {
            house_delta: -600,
            ..
        }]
    ));
}

#[test]
fn blackjack_deals_immediately_once_everyone_has_bet() {
    let mut table = blackjack_table();
    sit(&mut table, 61, 0, 10_000, 0);
    sit(&mut table, 62, 1, 10_000, 0);
    act(&mut table, 61, CasinoCommand::Start, 0);
    act(
        &mut table,
        61,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 0,
            plus3: 0,
        },
        100,
    );
    // 한 명이 아직 베팅 전이면 베팅창을 유지한다.
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Betting);
    act(
        &mut table,
        62,
        CasinoCommand::Bet {
            amount: 300,
            pairs: 0,
            plus3: 0,
        },
        200,
    );
    // 모두 베팅했으니 15초를 기다리지 않고 바로 카드를 나눈다.
    let round = table.round.as_ref().unwrap();
    assert_ne!(round.phase, Phase::Betting);
    assert_eq!(round.dealer.len(), 2);
    assert_eq!(table.seat(0).unwrap().hands[0].cards.len(), 2);
    assert_eq!(table.seat(1).unwrap().hands[0].cards.len(), 2);
    assert_eq!(
        table.seat(0).unwrap().stack + table.seat(1).unwrap().stack,
        19_200
    );
}

#[test]
fn blackjack_waits_for_a_seat_that_can_still_bet() {
    let mut table = blackjack_table();
    sit(&mut table, 63, 0, 10_000, 0);
    sit(&mut table, 64, 1, 10_000, 0);
    act(&mut table, 63, CasinoCommand::Start, 0);
    act(
        &mut table,
        63,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 0,
            plus3: 0,
        },
        100,
    );
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Betting);
    // 베팅창이 끝나야 딜한다. 베팅하지 않은 좌석은 이번 라운드를 쉰다.
    table.tick(BET_WINDOW_MS + 1).unwrap();
    assert_ne!(table.round.as_ref().unwrap().phase, Phase::Betting);
    assert!(table.seat(0).unwrap().in_hand);
    assert!(!table.seat(1).unwrap().in_hand);
}

#[test]
fn blackjack_without_bets_completes_quietly() {
    let mut table = blackjack_table();
    sit(&mut table, 60, 0, 10_000, 0);
    act(&mut table, 60, CasinoCommand::Start, 0);
    let events = table.tick(BET_WINDOW_MS + 1).unwrap();
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert!(
        events.is_empty(),
        "정산 결과가 없는데 이벤트가 나오면 안 된다: {events:?}"
    );
    assert!(table.history.is_empty());
    assert_eq!(table.seat(0).unwrap().stack, 10_000);
}

#[test]
fn leaving_during_a_hand_cashes_out_after_settlement() {
    let mut table = holdem_table();
    sit(&mut table, 70, 0, 10_000, 0);
    sit(&mut table, 71, 1, 10_000, 0);
    act(&mut table, 70, CasinoCommand::Start, 0);
    // 헤즈업: 버튼(0)이 스몰 블라인드이고 먼저 행동한다.
    assert_eq!(table.round.as_ref().unwrap().turn, 0);
    let events = act(&mut table, 70, CasinoCommand::Leave, 100);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert!(table.seats[0].is_none(), "핸드가 끝나면 좌석이 비워진다");
    assert!(
        events.iter().any(|event| matches!(
            event,
            CasinoEvent::CashOut {
                user_id: 70,
                amount: 9_950,
                ..
            }
        )),
        "{events:?}"
    );
    assert_eq!(table.seat(1).unwrap().stack, 10_050);
}

#[test]
fn timeouts_auto_act_and_two_in_a_row_sit_out() {
    let mut table = holdem_table();
    sit(&mut table, 80, 0, 10_000, 0);
    sit(&mut table, 81, 1, 10_000, 0);
    act(&mut table, 80, CasinoCommand::Start, 0);
    table.tick(TURN_MS + 1).unwrap();
    assert_eq!(
        table.round.as_ref().unwrap().phase,
        Phase::Complete,
        "시간 초과 폴드"
    );
    assert_eq!(table.seat(0).unwrap().missed, 1);
    assert!(!table.seat(0).unwrap().sit_out);
    table.seats[0].as_mut().unwrap().missed = 1;
    act(&mut table, 81, CasinoCommand::Start, 40_000);
    // 다음 핸드는 버튼이 넘어가 1이 먼저 행동한다. 그 다음 0의 차례를 넘겨 본다.
    let turn = table.round.as_ref().unwrap().turn;
    let first_actor = table.seat(turn).unwrap().user_id;
    if first_actor == 81 {
        act(&mut table, 81, CasinoCommand::Call, 40_100);
    }
    assert_eq!(table.round.as_ref().unwrap().turn, 0);
    let deadline = table.round.as_ref().unwrap().deadline;
    table.tick(deadline + 1).unwrap();
    assert!(
        table.seat(0).unwrap().sit_out,
        "2회 연속 시간 초과면 자리 비움"
    );
    // 라운드가 끝난 뒤 복귀할 수 있다.
    while table.playing() {
        let deadline = table.round.as_ref().unwrap().deadline;
        table.tick(deadline + 1).unwrap();
    }
    act(&mut table, 80, CasinoCommand::Resume, 90_000);
    assert!(!table.seat(0).unwrap().sit_out);
}

#[test]
fn join_and_chat_are_validated() {
    let mut table = holdem_table();
    let bad_amount = table.apply_command(
        90,
        "P90",
        &CasinoCommand::Join {
            seat: 0,
            amount: 4_900,
            name: "Alpha".into(),
        },
        None,
        0,
    );
    assert!(bad_amount.is_err());
    let bad_seat = table.apply_command(
        90,
        "P90",
        &CasinoCommand::Join {
            seat: 6,
            amount: 5_000,
            name: "Alpha".into(),
        },
        None,
        0,
    );
    assert!(bad_seat.is_err());
    let bad_name = table.apply_command(
        90,
        "P90",
        &CasinoCommand::Join {
            seat: 0,
            amount: 5_000,
            name: "A".into(),
        },
        None,
        0,
    );
    assert!(bad_name.is_err());
    let chat_without_seat = table.apply_command(
        90,
        "P90",
        &CasinoCommand::Chat {
            message: "hi".into(),
        },
        None,
        0,
    );
    assert!(chat_without_seat.is_err());

    sit(&mut table, 90, 0, 5_000, 0);
    let taken = table.apply_command(
        91,
        "P91",
        &CasinoCommand::Join {
            seat: 0,
            amount: 5_000,
            name: "Beta".into(),
        },
        None,
        0,
    );
    assert!(taken.is_err());
    act(
        &mut table,
        90,
        CasinoCommand::Chat {
            message: "안녕 소피아".into(),
        },
        1_000,
    );
    let too_fast = table.apply_command(
        90,
        "P90",
        &CasinoCommand::Chat {
            message: "again".into(),
        },
        None,
        1_500,
    );
    assert!(too_fast.is_err());
    assert!(
        table
            .messages
            .iter()
            .any(|message| message.text == "안녕 소피아" && message.user_id == Some(90))
    );
    assert!(
        table.messages.last().unwrap().dealer,
        "인사에는 소피아가 답한다"
    );
    let before = table.next_message_seq;
    table.relay_chat(99, "디스코드유저", "  채널에서 보냄  ", 2_000);
    assert_eq!(table.next_message_seq, before + 1);
    assert_eq!(table.messages.last().unwrap().text, "채널에서 보냄");
}

#[test]
fn idle_seats_cash_out_after_thirty_minutes() {
    let mut table = holdem_table();
    sit(&mut table, 100, 0, 8_000, 0);
    assert!(table.tick(IDLE_CASHOUT_MS).unwrap().is_empty());
    let events = table.tick(IDLE_CASHOUT_MS + 1).unwrap();
    assert_eq!(
        events,
        vec![CasinoEvent::CashOut {
            user_id: 100,
            name: "P100".to_string(),
            amount: 8_000
        }]
    );
    assert!(table.seats[0].is_none());
}

#[test]
fn closing_a_table_returns_pending_bets_too() {
    let mut table = blackjack_table();
    sit(&mut table, 110, 0, 10_000, 0);
    act(&mut table, 110, CasinoCommand::Start, 0);
    act(
        &mut table,
        110,
        CasinoCommand::Bet {
            amount: 300,
            pairs: 0,
            plus3: 0,
        },
        100,
    );
    assert_eq!(table.seat(0).unwrap().stack, 9_700);
    let events = table.close();
    assert_eq!(
        events,
        vec![CasinoEvent::CashOut {
            user_id: 110,
            name: "P110".to_string(),
            amount: 10_000
        }]
    );
    assert!(table.round.is_none());
}

#[test]
fn table_view_hides_other_players_cards_until_showdown() {
    let mut table = holdem_table();
    sit(&mut table, 120, 0, 10_000, 0);
    sit(&mut table, 121, 1, 10_000, 0);
    act(&mut table, 120, CasinoCommand::Start, 0);
    let mine = table_view(&table, Some(120), i64::MAX);
    assert_eq!(mine.my_seat, 0);
    assert_eq!(mine.seats[0].as_ref().unwrap().cards.len(), 2);
    assert!(
        mine.seats[0]
            .as_ref()
            .unwrap()
            .cards
            .iter()
            .all(|card| card != "??")
    );
    assert_eq!(mine.seats[1].as_ref().unwrap().cards, cards(&["??", "??"]));
    let spectator = table_view(&table, None, i64::MAX);
    assert_eq!(spectator.my_seat, -1);
    assert!(
        spectator
            .seats
            .iter()
            .flatten()
            .all(|seat| seat.cards.iter().all(|card| card == "??"))
    );
    assert!(spectator.legal.poker.is_none());
    let json = serde_json::to_string(&mine).unwrap();
    assert!(json.contains("\"kind\":\"holdem\""));
}
