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
    assert_eq!(result.won, 4_700);
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
    assert!(!table.seat(0).unwrap().insurance_decided);
    let stale = table.apply_command(81, "P81", &CasinoCommand::Hit, None, 150);
    assert!(stale.is_err(), "인슈어런스 중에는 플레이 액션이 막힌다");
    act(&mut table, 81, CasinoCommand::Insure { accept: true }, 200);
    // 혼자라서 바로 확인: 딜러 블랙잭 → 메인 -500, 인슈어런스 +500 = 0.
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    let result = &table.history[0].results[0];
    assert_eq!(result.wagered, 750);
    assert_eq!(result.net, 0);
    assert_eq!(result.won, 500);
    assert_eq!(result.paid, 750);
    assert!(
        result
            .notes
            .iter()
            .any(|note| note.contains("인슈어런스 2:1"))
    );
    assert_eq!(table.seat(0).unwrap().stack, 10_000);
}

#[test]
fn blackjack_winnings_keep_main_win_when_side_losses_make_net_negative() {
    let mut table = blackjack_table();
    sit(&mut table, 80, 0, 10_000, 0);
    table
        .start_with_deck(80, deck_from_top(&["Th", "9s", "Kd", "8c"]), 0)
        .unwrap();
    act(
        &mut table,
        80,
        CasinoCommand::Bet {
            amount: 2_000,
            pairs: 1_500,
            plus3: 1_500,
        },
        100,
    );
    act(&mut table, 80, CasinoCommand::Stand, 200);
    let result = &table.history[0].results[0];
    assert_eq!(result.net, -1_000);
    assert_eq!(table.seat(0).unwrap().stack, 9_000);
    assert_eq!(result.won, 2_000);
    assert_eq!(result.paid, 4_000);
}

#[test]
fn blackjack_winnings_exclude_returned_stakes_and_reset_each_round() {
    let mut table = blackjack_table();
    sit(&mut table, 80, 0, 10_000, 0);
    // 21+3 플러시 5:1 적중, 메인 13은 딜러 19에 패배.
    table
        .start_with_deck(80, deck_from_top(&["5h", "9h", "8h", "Tc"]), 0)
        .unwrap();
    act(
        &mut table,
        80,
        CasinoCommand::Bet {
            amount: 2_000,
            pairs: 0,
            plus3: 1_000,
        },
        100,
    );
    act(&mut table, 80, CasinoCommand::Stand, 200);
    assert_eq!(table.history[0].results[0].won, 5_000);
    assert_eq!(table.history[0].results[0].paid, 6_000);
    assert_eq!(table.history[0].results[0].net, 3_000);
    assert_eq!(table.seat(0).unwrap().stack, 13_000);
    // 다음 판에 전부 지면 이전 적중 금액이 남지 않는다.
    table
        .start_with_deck(80, deck_from_top(&["5h", "9s", "8d", "Tc"]), 300)
        .unwrap();
    act(
        &mut table,
        80,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 100,
            plus3: 100,
        },
        400,
    );
    act(&mut table, 80, CasinoCommand::Stand, 500);
    assert_eq!(table.history[0].results[0].won, 0);
    assert_eq!(table.history[0].results[0].paid, 0);
    assert_eq!(table.history[0].results[0].net, -700);
    // 푸시는 원금만 반환하므로 won·paid는 0.
    table
        .start_with_deck(80, deck_from_top(&["Th", "9s", "9d", "Tc"]), 600)
        .unwrap();
    act(
        &mut table,
        80,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 0,
            plus3: 0,
        },
        700,
    );
    act(&mut table, 80, CasinoCommand::Stand, 800);
    assert_eq!(table.history[0].results[0].won, 0);
    assert_eq!(table.history[0].results[0].paid, 0);
    assert_eq!(table.history[0].results[0].net, 0);
    let mut old = serde_json::to_value(&table).unwrap();
    old["history"][0]["results"][0]
        .as_object_mut()
        .unwrap()
        .remove("won");
    old["history"][0]["results"][0]
        .as_object_mut()
        .unwrap()
        .remove("paid");
    old["seats"][0].as_object_mut().unwrap().remove("side_won");
    old["seats"][0].as_object_mut().unwrap().remove("side_paid");
    let restored: CasinoTable = serde_json::from_value(old).unwrap();
    assert_eq!(restored.history[0].results[0].won, 0);
    assert_eq!(restored.history[0].results[0].paid, 0);
    assert_eq!(restored.seat(0).unwrap().side_won, 0);
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

fn prepared_shoe(remaining: usize) -> CasinoTable {
    let mut table = blackjack_table();
    sit(&mut table, 80, 0, 10_000, 0);
    table.shoe = vec!["2c".to_string(); remaining - 8];
    table.shoe.extend(deck_from_top(&[
        "Th", "9s", "Kd", "8c", "Th", "9s", "Kd", "8c",
    ]));
    table.shoe_total = 416;
    table.shoe_cut = 100;
    table.shuffled_at = 500;
    table
}

#[test]
fn blackjack_shoe_survives_rounds_and_saved_state() {
    let mut table = prepared_shoe(416);
    for (now, remaining) in [(1_000, 412), (2_000, 408)] {
        act(&mut table, 80, CasinoCommand::Start, now);
        assert!(table.shoe.is_empty());
        act(
            &mut table,
            80,
            CasinoCommand::Bet {
                amount: 100,
                pairs: 0,
                plus3: 0,
            },
            now + 100,
        );
        let view = table_view(&table, Some(80), now + 100);
        assert_eq!(view.shoe.unwrap().remaining, remaining);
        act(&mut table, 80, CasinoCommand::Stand, now + 200);
        assert_eq!(table.shoe.len(), remaining);
        assert!(table.round.as_ref().unwrap().deck.is_empty());
        assert_eq!(table.shuffled_at, 500);
        table = serde_json::from_value(serde_json::to_value(table).unwrap()).unwrap();
    }
    assert_eq!(table.shoe.len(), 408);
    assert!(table_view(&holdem_table(), None, 0).shoe.is_none());
}

#[test]
fn blackjack_cut_card_shuffles_only_on_next_start() {
    let mut table = prepared_shoe(104);
    act(&mut table, 80, CasinoCommand::Start, 1_000);
    act(
        &mut table,
        80,
        CasinoCommand::Bet {
            amount: 100,
            pairs: 0,
            plus3: 0,
        },
        1_100,
    );
    assert!(table_view(&table, None, 1_100).shoe.unwrap().reshuffle_due);
    assert_eq!(table.shuffled_at, 500);
    act(&mut table, 80, CasinoCommand::Stand, 1_200);
    assert_eq!(table.shoe.len(), 100);
    act(&mut table, 80, CasinoCommand::Start, 2_000);
    let shoe = table_view(&table, None, 2_000).shoe.unwrap();
    assert_eq!(
        (shoe.remaining, shoe.total, shoe.shuffled_at),
        (416, 416, 2_000)
    );
    assert!((84..=124).contains(&shoe.cut_at));
    assert!(!shoe.reshuffle_due);
    assert!(table.narration.contains("섞"));
}

#[test]
fn blackjack_empty_betting_and_explicit_decks_preserve_shoe() {
    let mut table = prepared_shoe(416);
    let original = table.shoe.clone();
    act(&mut table, 80, CasinoCommand::Start, 1_000);
    table.tick(1_000 + BET_WINDOW_MS).unwrap();
    assert_eq!(table.shoe, original);
    table
        .start_with_deck(80, deck_from_top(&["Th", "9s", "Kd", "8c"]), 20_000)
        .unwrap();
    act(
        &mut table,
        80,
        CasinoCommand::Bet {
            amount: 100,
            pairs: 0,
            plus3: 0,
        },
        20_100,
    );
    act(&mut table, 80, CasinoCommand::Stand, 20_200);
    assert_eq!(table.shoe, original);
    assert_eq!(table.shuffled_at, 500);
    // 새 필드가 없는 과거 저장 파일도 읽힌다.
    let mut old = serde_json::to_value(&table).unwrap();
    for field in ["shoe", "shoe_cut", "shoe_total", "shuffled_at"] {
        old.as_object_mut().unwrap().remove(field);
    }
    old["round"].as_object_mut().unwrap().remove("uses_shoe");
    let restored: CasinoTable = serde_json::from_value(old).unwrap();
    assert!(restored.shoe.is_empty());
    assert_eq!(restored.shoe_total, 0);
}

#[test]
fn blackjack_depleted_live_shoe_recovers_without_replacing_test_decks() {
    let mut table = prepared_shoe(416);
    act(&mut table, 80, CasinoCommand::Start, 1_000);
    act(
        &mut table,
        80,
        CasinoCommand::Bet {
            amount: 100,
            pairs: 0,
            plus3: 0,
        },
        1_100,
    );
    table.round.as_mut().unwrap().deck.clear();
    act(&mut table, 80, CasinoCommand::Hit, 1_200);
    assert_eq!(table.shuffled_at, 1_200);
    assert_eq!(table_view(&table, None, 1_200).shoe.unwrap().remaining, 415);
    let mut explicit = blackjack_table();
    sit(&mut explicit, 80, 0, 10_000, 0);
    explicit
        .start_with_deck(80, deck_from_top(&["Th", "9s", "Kd", "8c"]), 0)
        .unwrap();
    act(
        &mut explicit,
        80,
        CasinoCommand::Bet {
            amount: 100,
            pairs: 0,
            plus3: 0,
        },
        100,
    );
    let error = explicit
        .apply_command(80, "P80", &CasinoCommand::Hit, None, 200)
        .unwrap_err();
    assert_eq!(error.code, "ENGINE_STATE");
    assert_eq!(explicit.shoe_total, 0);
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
fn blackjack_reveal_hides_turn_and_legal_actions_for_natural_and_normal_deals() {
    let mut natural = blackjack_table();
    let mut normal = blackjack_table();
    sit(&mut natural, 200, 0, 10_000, 0);
    sit(&mut normal, 201, 0, 10_000, 0);
    let natural_now = deal_blackjack_with(
        &mut natural,
        200,
        &[(200, 500)],
        &["8h", "Th", "8d", "As"],
        0,
    );
    let normal_now = deal_blackjack_with(
        &mut normal,
        201,
        &[(201, 500)],
        &["8h", "6s", "8d", "9s"],
        0,
    );
    natural.round.as_mut().unwrap().reveal_until = 10_000;
    normal.round.as_mut().unwrap().reveal_until = 10_000;
    let natural_round = natural.round.as_ref().unwrap();
    let normal_round = normal.round.as_ref().unwrap();
    let natural_view = table_view(&natural, Some(200), natural_round.reveal_until - 1);
    let normal_view = table_view(&normal, Some(201), normal_round.reveal_until - 1);
    let natural_round = natural_view.round.as_ref().unwrap();
    let normal_round = normal_view.round.as_ref().unwrap();
    assert_eq!(
        (
            natural_round.turn,
            natural_round.hand,
            natural_round.deadline
        ),
        (normal_round.turn, normal_round.hand, normal_round.deadline)
    );
    assert_eq!(natural_view.legal.blackjack, None);
    assert_eq!(normal_view.legal.blackjack, None);
    assert_eq!(natural_round.turn, -1);
    assert!(
        table_view(&normal, Some(201), normal_round.reveal_until)
            .legal
            .blackjack
            .is_some()
    );
    let _ = (natural_now, normal_now);
}

#[test]
fn blackjack_split_then_double_each_hand_settles_separately() {
    let mut table = blackjack_table();
    sit(&mut table, 50, 0, 10_000, 0);
    // 딜: P0 8h, 딜러 5c, P0 8d, 딜러 9s. 스플릿: 앞 핸드에 3h. 더블: 9d.
    // 그다음 차례가 온 뒤 핸드에 Tc. 딜러 드로: Kc.
    let now = deal_blackjack_with(
        &mut table,
        50,
        &[(50, 200)],
        &["8h", "5c", "8d", "9s", "3h", "9d", "Tc", "Kc"],
        0,
    );
    let legal = table.blackjack_legal_for(0).unwrap();
    assert!(legal.can_split && legal.can_double && !legal.can_surrender);
    act(&mut table, 50, CasinoCommand::Split, now + 100);
    let seat = table.seat(0).unwrap();
    assert_eq!(seat.hands.len(), 2);
    assert_eq!(seat.hands[0].cards, cards(&["8h", "3h"]));
    // 실제 카지노처럼 뒤 핸드는 차례가 와야 두 번째 카드를 받는다.
    assert_eq!(seat.hands[1].cards, cards(&["8d"]));
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
    assert_eq!(seat.hands[0].cards, cards(&["8h", "3h", "9d"]));
    assert_eq!(seat.hands[1].cards, cards(&["8d", "Tc"]));
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
    assert_eq!(too_fast.unwrap_err().code, "CHAT_TOO_FAST");
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
    // 제한 간격(0.7초)이 지나면 이어 쓴 메시지를 받는다.
    act(
        &mut table,
        90,
        CasinoCommand::Chat {
            message: "이어서".into(),
        },
        1_700,
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
    // 혼자 베팅하면 바로 딜되므로, 내추럴로 즉시 정산되지 않게 카드를 정해 둔다.
    table
        .start_with_deck(110, deck_from_top(&["2h", "3c", "4d", "5s", "6h", "7c"]), 0)
        .unwrap();
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

/// 닫을 때 돌려준 칩 (좌석 순서).
fn cash_outs(events: &[CasinoEvent]) -> Vec<(u64, i64)> {
    events
        .iter()
        .filter_map(|event| match event {
            CasinoEvent::CashOut {
                user_id, amount, ..
            } => Some((*user_id, *amount)),
            _ => None,
        })
        .collect()
}

/// 지금 테이블을 닫으면 돌려줄 칩 합계 (원본은 그대로 둔다).
fn closed_total(table: &CasinoTable) -> i64 {
    cash_outs(&table.clone().close())
        .iter()
        .map(|(_, amount)| amount)
        .sum()
}

#[test]
fn closing_after_a_finished_holdem_hand_pays_only_the_stacks() {
    let mut table = holdem_table();
    sit(&mut table, 111, 0, 10_000, 0);
    sit(&mut table, 112, 1, 10_000, 0);
    act(&mut table, 111, CasinoCommand::Start, 0);
    // 먼저 액션하는 쪽이 올인, 상대가 콜 → 보드를 끝까지 열고 정산된다.
    let first = table.round.as_ref().unwrap().turn;
    let first_user = table.seat(first).unwrap().user_id;
    let all_in = table.poker_legal_for(first).unwrap().max_raise_to;
    assert_eq!(all_in, 10_000);
    act(
        &mut table,
        first_user,
        CasinoCommand::Raise { amount: all_in },
        100,
    );
    let second = table.round.as_ref().unwrap().turn;
    let second_user = table.seat(second).unwrap().user_id;
    act(&mut table, second_user, CasinoCommand::Call, 200);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert_eq!(stacks(&table), 20_000);
    // 끝난 핸드의 total은 기록으로 남아 있지만, 이미 팟으로 정산되었다.
    assert_eq!(committed(&table), 20_000);
    let expected = table
        .seats
        .iter()
        .flatten()
        .map(|seat| (seat.user_id, seat.stack))
        .collect::<Vec<_>>();
    let events = table.close();
    assert_eq!(cash_outs(&events), expected);
    assert_eq!(
        cash_outs(&events)
            .iter()
            .map(|(_, amount)| amount)
            .sum::<i64>(),
        20_000,
        "끝난 핸드의 베팅을 한 번 더 돌려주면 안 된다"
    );
}

#[test]
fn closing_after_a_finished_blackjack_round_pays_only_the_stack() {
    let mut table = blackjack_table();
    sit(&mut table, 113, 0, 10_000, 0);
    // 내 17 (Th 7d) 대 딜러 19 (9s Kc): 메인·사이드 모두 진다.
    table
        .start_with_deck(113, deck_from_top(&["Th", "9s", "7d", "Kc"]), 0)
        .unwrap();
    act(
        &mut table,
        113,
        CasinoCommand::Bet {
            amount: 5_000,
            pairs: 100,
            plus3: 100,
        },
        100,
    );
    act(&mut table, 113, CasinoCommand::Stand, 200);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert_eq!(table.seat(0).unwrap().stack, 4_800);
    let events = table.close();
    assert_eq!(cash_outs(&events), vec![(113, 4_800)]);
}

#[test]
fn closing_during_blackjack_betting_refunds_every_stake_once() {
    let mut table = blackjack_table();
    sit(&mut table, 114, 0, 10_000, 0);
    sit(&mut table, 115, 1, 10_000, 0);
    act(&mut table, 114, CasinoCommand::Start, 0);
    act(
        &mut table,
        114,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 100,
            plus3: 100,
        },
        100,
    );
    // 115가 아직 베팅하지 않아 딜 전이다: 사이드베팅도 정산되지 않았다.
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Betting);
    assert_eq!(table.seat(0).unwrap().stack, 9_300);
    let events = table.close();
    assert_eq!(cash_outs(&events), vec![(114, 10_000), (115, 10_000)]);
}

#[test]
fn closing_mid_blackjack_round_keeps_side_bets_settled_on_the_deal() {
    // 사이드베팅 적중: 컬러 페어 +1,200, 21+3 트리플 +3,000은 이미 스택에 있다.
    let mut table = blackjack_table();
    sit(&mut table, 116, 0, 10_000, 0);
    table
        .start_with_deck(116, deck_from_top(&["8h", "8s", "8d", "5c", "Tc"]), 0)
        .unwrap();
    act(
        &mut table,
        116,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 100,
            plus3: 100,
        },
        100,
    );
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Playing);
    assert_eq!(table.seat(0).unwrap().stack, 13_700);
    // 메인 500만 돌려준다 (사이드 원금 200은 적중 때 이미 돌려받았다).
    assert_eq!(cash_outs(&table.close()), vec![(116, 14_200)]);

    // 사이드베팅 실패: 잃은 사이드 원금은 돌려주지 않는다.
    let mut table = blackjack_table();
    sit(&mut table, 117, 0, 10_000, 0);
    table
        .start_with_deck(117, deck_from_top(&["Th", "9s", "Kd", "8c"]), 0)
        .unwrap();
    act(
        &mut table,
        117,
        CasinoCommand::Bet {
            amount: 2_000,
            pairs: 1_500,
            plus3: 1_500,
        },
        100,
    );
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Playing);
    assert_eq!(table.seat(0).unwrap().stack, 5_000);
    assert_eq!(cash_outs(&table.close()), vec![(117, 7_000)]);

    // 스플릿 뒤에 닫으면 두 핸드의 베팅을 모두 돌려준다.
    let mut table = blackjack_table();
    sit(&mut table, 118, 0, 10_000, 0);
    let now = deal_blackjack_with(
        &mut table,
        118,
        &[(118, 200)],
        &["8h", "5c", "8d", "9s", "3h", "Tc"],
        0,
    );
    act(&mut table, 118, CasinoCommand::Split, now + 100);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Playing);
    assert_eq!(table.seat(0).unwrap().stack, 9_600);
    assert_eq!(cash_outs(&table.close()), vec![(118, 10_000)]);
}

#[test]
fn closing_refunds_insurance_only_before_the_dealer_checks() {
    let mut table = blackjack_table();
    sit(&mut table, 119, 0, 10_000, 0);
    sit(&mut table, 120, 1, 10_000, 0);
    // 119: 9h 7d, 120: 6c 5s, 딜러: As 5c (블랙잭 아님).
    table
        .start_with_deck(
            119,
            deck_from_top(&["9h", "6c", "As", "7d", "5s", "5c", "Tc", "9d"]),
            0,
        )
        .unwrap();
    for (user, amount, at) in [(119, 500, 100), (120, 300, 200)] {
        act(
            &mut table,
            user,
            CasinoCommand::Bet {
                amount,
                pairs: 0,
                plus3: 0,
            },
            at,
        );
    }
    act(&mut table, 119, CasinoCommand::Insure { accept: true }, 300);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Insurance);
    assert_eq!(table.seat(0).unwrap().stack, 9_250);
    // 딜러 확인 전: 인슈어런스는 아직 정산되지 않았으므로 베팅과 함께 돌려준다.
    assert_eq!(
        cash_outs(&table.clone().close()),
        vec![(119, 10_000), (120, 10_000)]
    );
    // 딜러가 블랙잭이 아니면 인슈어런스 250은 이미 잃은 것이다.
    table.tick(300 + INSURANCE_MS + 1).unwrap();
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Playing);
    assert_eq!(cash_outs(&table.close()), vec![(119, 9_750), (120, 10_000)]);
}

#[test]
fn closing_a_holdem_table_at_any_moment_conserves_chips() {
    let mut table = holdem_table();
    for (user, seat) in [(130, 0), (131, 2), (132, 4)] {
        sit(&mut table, user, seat, 10_000, 0);
    }
    let mut rng = crate::system_random::rng();
    let mut now = 0;
    for _ in 0..40 {
        let funded = table
            .seats
            .iter()
            .flatten()
            .filter(|seat| seat.stack > 0)
            .map(|seat| seat.user_id)
            .collect::<Vec<_>>();
        if funded.len() < 2 {
            break;
        }
        now += 1_000;
        act(&mut table, funded[0], CasinoCommand::Start, now);
        let mut guard = 0;
        while table.playing() {
            guard += 1;
            assert!(guard < 500, "라운드가 끝나지 않습니다");
            // 진행 중에 닫으면 건 칩(total)을 돌려줘 총액이 그대로다.
            assert_eq!(closed_total(&table), 30_000, "진행 중 닫기");
            now += 1_000;
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
                CasinoCommand::Raise {
                    amount: legal.max_raise_to,
                }
            };
            act(&mut table, actor, command, now);
        }
        // 끝난 뒤에 닫으면 스택만 돌려준다.
        assert_eq!(stacks(&table), 30_000);
        assert_eq!(closed_total(&table), 30_000, "끝난 뒤 닫기");
    }
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

#[test]
fn table_settings_default_to_the_classic_limits() {
    for kind in [GameKind::Holdem, GameKind::Blackjack] {
        assert_eq!(
            TableSettings::build(kind, SettingsRequest::default()).unwrap(),
            TableSettings::default()
        );
    }
    let high = TableSettings::build(
        GameKind::Blackjack,
        SettingsRequest {
            min_bet: Some(1_000),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        (high.min_bet, high.max_bet, high.side_bet_max),
        (1_000, 50_000, 25_000)
    );
    assert_eq!((high.min_buy_in, high.max_buy_in), (50_000, 200_000));
    assert!(
        high.summary(GameKind::Blackjack)
            .starts_with("베팅 1,000~50,000 · 사이드 최대 25,000")
    );
    let holdem = TableSettings::build(
        GameKind::Holdem,
        SettingsRequest {
            big_blind: Some(400),
            turn_secs: Some(20),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!((holdem.small_blind, holdem.big_blind), (200, 400));
    assert_eq!(
        (holdem.min_buy_in, holdem.max_buy_in, holdem.turn_ms),
        (20_000, 80_000, 20_000)
    );
    // 최대 바이인만 낮추면 최소 바이인도 따라 내려간다.
    let small = TableSettings::build(
        GameKind::Blackjack,
        SettingsRequest {
            max_buy_in: Some(3_000),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!((small.min_buy_in, small.max_buy_in), (3_000, 3_000));
    let no_side = TableSettings::build(
        GameKind::Blackjack,
        SettingsRequest {
            side_bet_max: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        no_side
            .summary(GameKind::Blackjack)
            .contains("사이드베팅 없음")
    );
}

#[test]
fn table_settings_reject_unplayable_limits() {
    let bad = |kind, request| TableSettings::build(kind, request).unwrap_err();
    let cases = [
        (
            GameKind::Blackjack,
            SettingsRequest {
                min_bet: Some(150),
                ..Default::default()
            },
            "최소 베팅",
        ),
        (
            GameKind::Blackjack,
            SettingsRequest {
                min_bet: Some(1_000),
                max_bet: Some(500),
                ..Default::default()
            },
            "최대 베팅",
        ),
        (
            GameKind::Blackjack,
            SettingsRequest {
                side_bet_max: Some(9_900),
                ..Default::default()
            },
            "사이드베팅",
        ),
        (
            GameKind::Holdem,
            SettingsRequest {
                big_blind: Some(55),
                ..Default::default()
            },
            "빅 블라인드",
        ),
        (
            GameKind::Holdem,
            SettingsRequest {
                min_buy_in: Some(500),
                ..Default::default()
            },
            "최소 바이인은 1,000 이상",
        ),
        (
            GameKind::Holdem,
            SettingsRequest {
                min_buy_in: Some(8_000),
                max_buy_in: Some(6_000),
                ..Default::default()
            },
            "최대 바이인",
        ),
        (
            GameKind::Holdem,
            SettingsRequest {
                turn_secs: Some(5),
                ..Default::default()
            },
            "제한 시간",
        ),
    ];
    for (kind, request, text) in cases {
        let message = bad(kind, request);
        assert!(message.contains(text), "{message}");
    }
    // 게임과 상관없는 항목은 무시한다.
    assert!(
        TableSettings::build(
            GameKind::Holdem,
            SettingsRequest {
                min_bet: Some(150),
                ..Default::default()
            }
        )
        .is_ok()
    );
}

#[test]
fn saved_tables_without_settings_load_the_classic_limits() {
    let mut json = serde_json::to_value(blackjack_table()).unwrap();
    json.as_object_mut().unwrap().remove("settings");
    let loaded: CasinoTable = serde_json::from_value(json).unwrap();
    assert_eq!(loaded.settings, TableSettings::default());
}

#[test]
fn only_call_and_raise_are_rejected_on_a_stale_version() {
    let mut table = blackjack_table();
    sit(&mut table, 70, 0, 5_000, 0);
    sit(&mut table, 71, 1, 5_000, 0);
    act(&mut table, 70, CasinoCommand::Start, 0);
    let seen = table.version;
    // 다른 사람이 먼저 베팅해 버전이 올라도 늦게 보낸 베팅은 받는다.
    table
        .apply_command(
            70,
            "P70",
            &CasinoCommand::Bet {
                amount: 100,
                pairs: 0,
                plus3: 0,
            },
            Some(seen),
            10,
        )
        .unwrap();
    assert!(
        table
            .apply_command(
                71,
                "P71",
                &CasinoCommand::Bet {
                    amount: 100,
                    pairs: 0,
                    plus3: 0,
                },
                Some(seen),
                20,
            )
            .is_ok()
    );
    assert!(CasinoCommand::Call.needs_current_state());
    assert!(CasinoCommand::Raise { amount: 300 }.needs_current_state());
    for command in [
        CasinoCommand::Fold,
        CasinoCommand::Check,
        CasinoCommand::Hit,
        CasinoCommand::Stand,
        CasinoCommand::Leave,
        CasinoCommand::Start,
    ] {
        assert!(!command.needs_current_state(), "{command:?}");
    }
}

#[test]
fn settings_change_applies_immediately_when_idle() {
    let mut table = blackjack_table();
    let settings = TableSettings::build(
        GameKind::Blackjack,
        SettingsRequest {
            min_bet: Some(500),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(table.change_settings(settings, 0));
    assert_eq!(table.settings, settings);
    assert_eq!(table.pending_settings, None);
}

#[test]
fn settings_change_waits_until_the_next_round() {
    let mut table = blackjack_table();
    sit(&mut table, 70, 0, 5_000, 0);
    table
        .start_with_deck(70, deck_from_top(&["As", "9c", "Kd", "7h", "5d"]), 0)
        .unwrap();
    let old = table.settings;
    let next = TableSettings::build(
        GameKind::Blackjack,
        SettingsRequest {
            min_bet: Some(500),
            max_bet: Some(2_000),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!table.change_settings(next, 1));
    assert_eq!(table.settings, old);
    assert_eq!(table.pending_settings, Some(next));
    table.round.as_mut().unwrap().phase = Phase::Complete;
    act(&mut table, 70, CasinoCommand::Start, 2);
    assert_eq!(table.settings, next);
    assert_eq!(table.pending_settings, None);
}

#[test]
fn queued_settings_apply_as_soon_as_the_round_ends() {
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
    // 이 좌석(약 10,000칩)이 새 최소 베팅(20,000)을 못 걸어도 다음 시작을 기다리지 않는다.
    let old = table.settings;
    let next = TableSettings::build(
        GameKind::Blackjack,
        SettingsRequest {
            min_bet: Some(20_000),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!table.change_settings(next, 150));
    assert_eq!(table.settings, old);
    let narration = table.narration.clone();
    // 딜러 블랙잭이라 인슈어런스 결정 뒤 라운드가 끝난다.
    act(&mut table, 81, CasinoCommand::Insure { accept: true }, 200);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert_eq!(table.settings, next);
    assert_eq!(table.pending_settings, None);
    assert_ne!(table.narration, narration, "결과 안내는 그대로 남는다");
    assert!(!table.narration.contains("방 설정이 바뀌었습니다"));
    assert!(
        table
            .messages
            .iter()
            .any(|message| message.text.contains("방 설정이 바뀌었습니다"))
    );
}

#[test]
fn resetting_to_the_current_settings_cancels_the_queued_change() {
    let mut table = blackjack_table();
    sit(&mut table, 70, 0, 5_000, 0);
    table
        .start_with_deck(70, deck_from_top(&["As", "9c", "Kd", "7h", "5d"]), 0)
        .unwrap();
    let old = table.settings;
    let next = TableSettings::build(
        GameKind::Blackjack,
        SettingsRequest {
            min_bet: Some(500),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!table.change_settings(next, 1));
    assert!(table.change_settings(old, 2));
    assert_eq!(table.pending_settings, None);
    assert_eq!(table.settings, old);
}

#[test]
fn pending_settings_round_trip_and_old_json_default() {
    let mut table = blackjack_table();
    table.pending_settings = Some(
        TableSettings::build(
            GameKind::Blackjack,
            SettingsRequest {
                min_bet: Some(500),
                ..Default::default()
            },
        )
        .unwrap(),
    );
    let json = serde_json::to_value(&table).unwrap();
    let loaded: CasinoTable = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(loaded.pending_settings, table.pending_settings);
    let mut old_json = json;
    old_json.as_object_mut().unwrap().remove("pending_settings");
    let loaded_old: CasinoTable = serde_json::from_value(old_json).unwrap();
    assert_eq!(loaded_old.pending_settings, None);
}

#[test]
fn blackjack_bets_and_buy_ins_follow_the_table_settings() {
    let settings = TableSettings::build(
        GameKind::Blackjack,
        SettingsRequest {
            min_bet: Some(500),
            max_bet: Some(2_000),
            side_bet_max: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    let mut table = blackjack_table().with_settings(settings);
    let join = |amount| CasinoCommand::Join {
        seat: 0,
        amount,
        name: "P70".to_string(),
    };
    // 최소 바이인은 최소 베팅 50번 = 25,000.
    let error = table
        .apply_command(70, "P70", &join(10_000), None, 0)
        .unwrap_err();
    assert!(error.message.contains("25,000"), "{}", error.message);
    sit(&mut table, 70, 0, 25_000, 0);
    act(&mut table, 70, CasinoCommand::Start, 0);
    let bet = |amount, pairs| CasinoCommand::Bet {
        amount,
        pairs,
        plus3: 0,
    };
    let version = table.version;
    for (command, text) in [
        (bet(400, 0), "500~2,000"),
        (bet(2_500, 0), "500~2,000"),
        (bet(1_000, 100), "사이드베팅을 받지 않"),
    ] {
        let error = table
            .apply_command(70, "P70", &command, Some(version), 10)
            .unwrap_err();
        assert!(error.message.contains(text), "{}", error.message);
    }
    act(&mut table, 70, bet(2_000, 0), 20);
    assert_eq!(table.seat(0).unwrap().hands[0].bet, 2_000);
    let view = table_view(&table, Some(70), i64::MAX);
    assert_eq!(
        (
            view.rules.min_bet,
            view.rules.max_bet,
            view.rules.side_bet_max,
            view.rules.min_buy_in
        ),
        (500, 2_000, 0, 25_000)
    );
}

#[test]
fn holdem_blinds_and_turn_time_follow_the_table_settings() {
    let settings = TableSettings::build(
        GameKind::Holdem,
        SettingsRequest {
            big_blind: Some(400),
            turn_secs: Some(20),
            ..Default::default()
        },
    )
    .unwrap();
    let mut table = holdem_table().with_settings(settings);
    sit(&mut table, 80, 0, 40_000, 0);
    sit(&mut table, 81, 1, 40_000, 0);
    act(&mut table, 80, CasinoCommand::Start, 0);
    let mut blinds = table
        .seats
        .iter()
        .flatten()
        .map(|seat| seat.bet)
        .collect::<Vec<_>>();
    blinds.sort();
    assert_eq!(blinds, vec![200, 400]);
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.current_bet, 400);
    assert_eq!(round.deadline, 20_000);
    let legal = table.poker_legal_for(round.turn).unwrap();
    assert_eq!(legal.min_raise_to, 800);
}

#[test]
fn blackjack_split_keeps_a_reveal_time_for_every_card() {
    let mut table = blackjack_table();
    sit(&mut table, 90, 0, 10_000, 0);
    let now = deal_blackjack_with(
        &mut table,
        90,
        &[(90, 200)],
        &["8h", "5c", "8d", "9s", "3h", "Tc"],
        0,
    );
    let moved_at = table.seat(0).unwrap().hands[0].reveal_at[1];
    act(&mut table, 90, CasinoCommand::Split, now + 100);
    let seat = table.seat(0).unwrap();
    for hand in &seat.hands {
        assert_eq!(hand.cards.len(), hand.reveal_at.len());
    }
    // 옮겨진 카드는 원래 놓인 시각을 유지한다. 앞 핸드만 지금 새 카드를 받는다.
    assert_eq!(seat.hands[1].reveal_at, vec![moved_at]);
    assert_eq!(seat.hands[0].reveal_at[1], now + 100 + DEAL_LEAD_MS);
    assert_eq!(
        table.round.as_ref().unwrap().reveal_until,
        now + 100 + DEAL_LEAD_MS + LAND_SETTLE_MS
    );
    let view = table_view(&table, Some(90), i64::MAX);
    let hands = &view.seats[0].as_ref().unwrap().hands;
    assert_eq!(hands.len(), 2);
    assert_eq!((hands[0].total, hands[1].total), (11, 8));

    // 앞 핸드를 마치면 뒤 핸드가 두 번째 카드를 받고 차례가 넘어간다.
    act(&mut table, 90, CasinoCommand::Stand, now + 5_000);
    let round = table.round.as_ref().unwrap();
    assert_eq!((round.turn, round.hand), (0, 1));
    let second = &table.seat(0).unwrap().hands[1];
    assert_eq!(second.cards, cards(&["8d", "Tc"]));
    assert_eq!(second.reveal_at, vec![moved_at, now + 5_000 + DEAL_LEAD_MS]);
    assert_eq!(
        round.reveal_until,
        now + 5_000 + DEAL_LEAD_MS + LAND_SETTLE_MS
    );
    let view = table_view(&table, Some(90), i64::MAX);
    let hands = &view.seats[0].as_ref().unwrap().hands;
    assert_eq!((hands[0].total, hands[1].total), (11, 18));
}

#[test]
fn blackjack_natural_returns_two_and_a_half_times_the_bet() {
    let mut table = blackjack_table();
    sit(&mut table, 95, 0, 20_000, 0);
    // 딜: P0 As, 딜러 9c, P0 Kd, 딜러 7h → P0 블랙잭, 딜러 16 (내추럴은 바로 정산).
    table
        .start_with_deck(95, deck_from_top(&["As", "9c", "Kd", "7h", "5d"]), 0)
        .unwrap();
    act(
        &mut table,
        95,
        CasinoCommand::Bet {
            amount: 5_000,
            pairs: 0,
            plus3: 0,
        },
        100,
    );
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    let hand = &table.seat(0).unwrap().hands[0];
    assert_eq!(
        hand.payout,
        Some(12_500),
        "블랙잭은 베팅의 2.5배를 돌려준다"
    );
    let result = &table.history[0].results[0];
    assert_eq!(result.paid, 12_500, "결과 헤드라인은 돌려받는 12,500");
    assert_eq!(result.won, 7_500);
    assert_eq!(result.net, 7_500);
    assert_eq!(table.seat(0).unwrap().stack, 20_000 + 7_500);
}

#[test]
fn holdem_winner_is_shown_the_pot_it_collects() {
    let mut table = holdem_table();
    sit(&mut table, 96, 0, 10_000, 0);
    sit(&mut table, 97, 1, 10_000, 0);
    act(&mut table, 96, CasinoCommand::Start, 0);
    // 차례인 사람이 폴드하면 상대가 블라인드 팟을 가져간다.
    let turn = table.round.as_ref().unwrap().turn;
    let folder = table.seat(turn).unwrap().user_id;
    act(&mut table, folder, CasinoCommand::Fold, 100);
    let result = table.history[0]
        .results
        .iter()
        .find(|result| result.user_id != folder)
        .unwrap();
    assert_eq!(result.won, result.net);
    assert_eq!(result.paid, result.wagered + result.net, "가져간 팟 전체");
    let loser = table.history[0]
        .results
        .iter()
        .find(|result| result.user_id == folder)
        .unwrap();
    assert_eq!((loser.won, loser.paid), (0, 0));
}

// ------------------------------------------------------------ 딜 연출 시각·공개 시점

#[test]
fn live_deal_timing_keeps_one_card_in_the_air() {
    use live_timing as live;
    let flight = live::CARD_FLIGHT_MS;
    let flip = live::CARD_FLIP_MS;
    for (name, value, minimum) in [
        // 착지 간격이 비행 시간보다 넉넉히 길어야 공중에 카드가 한 장뿐이다.
        ("DEAL_CARD_MS", live::DEAL_CARD_MS, flight + 80),
        ("HOLE_CARD_MS", live::HOLE_CARD_MS, flight + 80),
        ("DEALER_DRAW_MS", live::DEALER_DRAW_MS, flight + 80),
        ("STREET_PAUSE_MS", live::STREET_PAUSE_MS, flight + 80),
        // 첫 비행 전에 푸시 지연을 흡수할 여유를 둔다.
        ("DEAL_LEAD_MS", live::DEAL_LEAD_MS, flight + 200),
        // 뒤집기가 끝난 뒤에 다음 공개·결과로 넘어간다.
        ("SHOWDOWN_STEP_MS", live::SHOWDOWN_STEP_MS, flip),
        ("SETTLE_PAUSE_MS", live::SETTLE_PAUSE_MS, flip),
    ] {
        assert!(
            value >= minimum,
            "{name} {value}ms는 {minimum}ms 이상이어야 한다"
        );
    }
    // 웹 규칙에는 테스트의 0이 아니라 실제 값이 나간다.
    let rules = table_rules(&TableSettings::default());
    assert_eq!(
        (rules.card_flight_ms, rules.card_flip_ms),
        (live::CARD_FLIGHT_MS, live::CARD_FLIP_MS)
    );
    assert!(rules.card_flight_ms > 0 && rules.card_flip_ms > 0);
}

#[test]
fn blackjack_deal_lands_one_card_at_a_time_in_casino_order() {
    let mut table = blackjack_table();
    sit(&mut table, 130, 0, 10_000, 0);
    sit(&mut table, 131, 2, 10_000, 0);
    table
        .start_with_deck(130, deck_from_top(&["2h", "3h", "4h", "5h", "6h", "7h"]), 0)
        .unwrap();
    for (user, at) in [(130, 100), (131, 200)] {
        act(
            &mut table,
            user,
            CasinoCommand::Bet {
                amount: 100,
                pairs: 0,
                plus3: 0,
            },
            at,
        );
    }
    // 모두 베팅해 200에 딜: 좌석0, 좌석2, 딜러 업카드, 좌석0, 좌석2, 딜러 홀 카드.
    let first = &table.seat(0).unwrap().hands[0];
    let second = &table.seat(2).unwrap().hands[0];
    let round = table.round.as_ref().unwrap();
    assert_eq!(first.cards, cards(&["2h", "5h"]));
    assert_eq!(second.cards, cards(&["3h", "6h"]));
    assert_eq!(round.dealer, cards(&["4h", "7h"]));
    let landings = vec![
        first.reveal_at[0],
        second.reveal_at[0],
        round.dealer_reveal_at[0],
        first.reveal_at[1],
        second.reveal_at[1],
        round.dealer_reveal_at[1],
    ];
    let expected = (0..6)
        .map(|n| 200 + DEAL_LEAD_MS + n * DEAL_CARD_MS)
        .collect::<Vec<_>>();
    assert_eq!(landings, expected, "reveal_at은 한 장씩 착지하는 시각");
    assert_eq!(round.reveal_until, expected[5] + LAND_SETTLE_MS);
    // 첫 차례의 제한 시간은 카드가 다 놓인 뒤부터 센다.
    assert_eq!(round.deadline, round.reveal_until + table.settings.turn_ms);
}

#[test]
fn blackjack_hit_lands_after_the_lead_and_unlocks_once_it_settles() {
    let mut table = blackjack_table();
    sit(&mut table, 132, 0, 10_000, 0);
    let now = deal_blackjack_with(
        &mut table,
        132,
        &[(132, 100)],
        &["2h", "9c", "3h", "7d", "4h", "Td"],
        0,
    );
    act(&mut table, 132, CasinoCommand::Hit, now + 1_000);
    let hand = &table.seat(0).unwrap().hands[0];
    assert_eq!(hand.cards.len(), 3);
    assert_eq!(hand.reveal_at[2], now + 1_000 + DEAL_LEAD_MS);
    assert_eq!(
        table.round.as_ref().unwrap().reveal_until,
        hand.reveal_at[2] + LAND_SETTLE_MS
    );
}

#[test]
fn blackjack_split_aces_get_both_second_cards_one_after_the_other() {
    let mut table = blackjack_table();
    sit(&mut table, 133, 0, 10_000, 0);
    // 딜: 나 Ah, 딜러 9c, 나 Ad, 딜러 7d. 스플릿: 5h, 6s. 딜러 드로 2c (18).
    let now = deal_blackjack_with(
        &mut table,
        133,
        &[(133, 200)],
        &["Ah", "9c", "Ad", "7d", "5h", "6s", "2c"],
        0,
    );
    act(&mut table, 133, CasinoCommand::Split, now + 100);
    let seat = table.seat(0).unwrap();
    assert_eq!(seat.hands[0].cards, cards(&["Ah", "5h"]));
    assert_eq!(seat.hands[1].cards, cards(&["Ad", "6s"]));
    assert!(
        seat.hands
            .iter()
            .all(|hand| hand.status == HandStatus::Stand)
    );
    assert_eq!(
        seat.hands[1].reveal_at[1],
        seat.hands[0].reveal_at[1] + DEAL_CARD_MS,
        "에이스 스플릿은 두 핸드에 한 장씩 차례로 준다"
    );
    let round = table.round.as_ref().unwrap();
    assert_eq!(
        round.phase,
        Phase::Complete,
        "에이스 스플릿은 더 칠 수 없어 바로 정산한다"
    );
    // 딜러는 두 핸드의 카드가 모두 놓인 뒤에 홀 카드를 뒤집는다.
    assert!(round.dealer_flip_at >= seat.hands[1].reveal_at[1] + LAND_SETTLE_MS);
}

#[test]
fn blackjack_split_hand_that_makes_21_passes_the_turn_on() {
    let mut table = blackjack_table();
    sit(&mut table, 134, 0, 10_000, 0);
    // 딜: 나 Th, 딜러 9c, 나 Kd, 딜러 7d. 스플릿: 앞 핸드 5h. 뒤 핸드 Ac (21). 딜러 드로 2c.
    let now = deal_blackjack_with(
        &mut table,
        134,
        &[(134, 200)],
        &["Th", "9c", "Kd", "7d", "5h", "Ac", "2c"],
        0,
    );
    act(&mut table, 134, CasinoCommand::Split, now + 100);
    assert_eq!(table.round.as_ref().unwrap().hand, 0);
    act(&mut table, 134, CasinoCommand::Stand, now + 200);
    let seat = table.seat(0).unwrap();
    assert_eq!(seat.hands[1].cards, cards(&["Kd", "Ac"]));
    assert_eq!(seat.hands[1].status, HandStatus::Stand);
    let round = table.round.as_ref().unwrap();
    assert_eq!(
        round.phase,
        Phase::Complete,
        "21이 된 뒤 핸드는 바로 넘어간다"
    );
    // 앞 핸드 15 패배(-200), 뒤 핸드 21 승리(+200, 스플릿 21은 블랙잭이 아니다).
    assert_eq!(seat.hands[1].result.as_deref(), Some("승리"));
    assert_eq!(seat.stack, 10_000);
}

#[test]
fn leaving_before_the_split_hand_is_played_still_settles_two_cards() {
    let mut table = blackjack_table();
    sit(&mut table, 135, 0, 10_000, 0);
    let now = deal_blackjack_with(
        &mut table,
        135,
        &[(135, 200)],
        &["8h", "9c", "8d", "Td", "3h", "Tc"],
        0,
    );
    act(&mut table, 135, CasinoCommand::Split, now + 100);
    let events = act(&mut table, 135, CasinoCommand::Leave, now + 200);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    // 떠난 좌석도 뒤 핸드에 두 번째 카드를 받아 정산된다: 11, 18 대 딜러 19 → 둘 다 패배.
    let result = &table.history[0].results[0];
    assert_eq!(result.net, -400);
    assert!(
        events.iter().any(|event| matches!(
            event,
            CasinoEvent::CashOut {
                user_id: 135,
                amount: 9_600,
                ..
            }
        )),
        "{events:?}"
    );
}

/// 헤즈업 홀덤: 좌석 0(버튼·스몰), 1(빅). 딜 순서 1, 0, 1, 0, 그다음 번·플롭·번·턴·번·리버.
fn heads_up_holdem(now: i64) -> CasinoTable {
    let mut table = holdem_table();
    sit(&mut table, 140, 0, 10_000, 0);
    sit(&mut table, 141, 1, 10_000, 0);
    let deck = deck_from_top(&[
        "Kh", "Qd", "Kc", "Qs", "2c", "7h", "8d", "9s", "3c", "Jd", "4c", "5h",
    ]);
    table.start_with_deck(140, deck, now).unwrap();
    table
}

/// 모두 체크(프리플롭은 스몰 콜 → 빅 체크)로 한 스트리트를 넘긴다.
fn check_round(table: &mut CasinoTable, preflop: bool, now: i64) {
    if preflop {
        act(table, 140, CasinoCommand::Call, now);
        act(table, 141, CasinoCommand::Check, now + 50);
    } else {
        // 플롭부터는 버튼 왼쪽(빅)이 먼저 행동한다.
        act(table, 141, CasinoCommand::Check, now);
        act(table, 140, CasinoCommand::Check, now + 50);
    }
}

#[test]
fn holdem_hole_cards_land_one_at_a_time_from_the_button_left() {
    let table = heads_up_holdem(1_000);
    let lands = |n: i64| 1_000 + DEAL_LEAD_MS + n * HOLE_CARD_MS;
    assert_eq!(
        table.seat(1).unwrap().cards_reveal_at,
        vec![lands(0), lands(2)]
    );
    assert_eq!(
        table.seat(0).unwrap().cards_reveal_at,
        vec![lands(1), lands(3)]
    );
    assert_eq!(
        table.round.as_ref().unwrap().reveal_until,
        lands(3) + LAND_SETTLE_MS
    );
}

#[test]
fn holdem_streets_burn_a_card_and_flip_the_flop_together() {
    let mut table = heads_up_holdem(1_000);
    check_round(&mut table, true, 1_100);
    let flop_at = 1_150;
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.phase, Phase::Flop);
    assert_eq!(round.board, cards(&["7h", "8d", "9s"]));
    let burn = flop_at + STREET_PAUSE_MS;
    assert_eq!(round.burn_at, vec![burn]);
    // 플롭 세 장은 번 카드 뒤에 한 장씩 뒷면으로 놓이고, 다 놓인 뒤 함께 뒤집힌다.
    assert_eq!(
        round.board_reveal_at,
        vec![
            burn + HOLE_CARD_MS,
            burn + 2 * HOLE_CARD_MS,
            burn + 3 * HOLE_CARD_MS
        ]
    );
    assert_eq!(
        round.board_flip_at,
        burn + 3 * HOLE_CARD_MS + LAND_SETTLE_MS
    );
    assert_eq!(round.reveal_until, round.board_flip_at + CARD_FLIP_MS);
    let flop_until = round.reveal_until;
    // "플롭 카드가 열렸어요"는 플롭이 뒤집힌 뒤에 뜬다.
    let opened = table
        .messages
        .iter()
        .find(|message| message.text.starts_with("플롭"))
        .unwrap();
    assert_eq!(opened.at, flop_until);
    let view = table_view(&table, None, i64::MAX);
    let round_view = view.round.as_ref().unwrap();
    assert_eq!(round_view.burn_at, vec![burn]);
    assert_eq!(
        round_view.board_flip_at,
        table.round.as_ref().unwrap().board_flip_at
    );
    assert!(
        !serde_json::to_string(&view).unwrap().contains("\"2c\""),
        "번 카드 값은 보내지 않는다"
    );

    // 턴·리버: 번 카드 뒤에 앞면으로 한 장.
    check_round(&mut table, false, 3_000);
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.phase, Phase::Turn);
    let turn_burn = 3_050 + STREET_PAUSE_MS;
    assert_eq!(round.burn_at, vec![burn, turn_burn]);
    assert_eq!(round.board_reveal_at[3], turn_burn + HOLE_CARD_MS);
    assert_eq!(
        round.reveal_until,
        round.board_reveal_at[3] + LAND_SETTLE_MS
    );
    assert_eq!(
        round.board_flip_at,
        burn + 3 * HOLE_CARD_MS + LAND_SETTLE_MS,
        "뒤집기 시각은 플롭에만 쓴다"
    );
    check_round(&mut table, false, 4_000);
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.phase, Phase::River);
    assert_eq!(round.burn_at.len(), 3);
    let json = serde_json::to_string(&table_view(&table, Some(140), i64::MAX)).unwrap();
    for burnt in ["2c", "3c", "4c"] {
        assert!(!json.contains(&format!("\"{burnt}\"")), "{burnt}");
    }
}

#[test]
fn holdem_view_never_leaks_other_hole_cards_before_settlement() {
    let mut table = heads_up_holdem(1_000);
    let hidden_everywhere = |table: &CasinoTable| {
        for (viewer, secret) in [
            (Some(140), vec!["Kh", "Kc"]),
            (Some(141), vec!["Qd", "Qs"]),
            (None, vec!["Kh", "Kc", "Qd", "Qs"]),
        ] {
            // 아무리 늦은 시각이어도 정산 전에는 가려져 있다.
            let view = table_view(table, viewer, i64::MAX);
            for seat in view.seats.iter().flatten() {
                if !seat.mine {
                    assert!(seat.cards.iter().all(|card| card == "??"));
                    assert_eq!(seat.showdown_at, 0);
                    assert_eq!(seat.hand_name, None);
                }
            }
            let json = serde_json::to_string(&view).unwrap();
            for card in secret {
                assert!(!json.contains(&format!("\"{card}\"")), "{card} 노출");
            }
        }
    };
    hidden_everywhere(&table);
    check_round(&mut table, true, 1_100);
    hidden_everywhere(&table);
    check_round(&mut table, false, 3_000);
    hidden_everywhere(&table);
    check_round(&mut table, false, 4_000);
    hidden_everywhere(&table);
    // 리버에서 마지막 체크 직전까지도 가려져 있다.
    act(&mut table, 141, CasinoCommand::Check, 5_000);
    hidden_everywhere(&table);
    act(&mut table, 140, CasinoCommand::Check, 5_050);
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.phase, Phase::Complete);
    assert!(round.reveal);
    let until = round.reveal_until;
    let flip_at = round.showdown_reveal_at[1];
    assert!(flip_at >= 5_050);

    // 정산된 뒤에는 뒤집을 시각과 함께 경쟁자의 카드를 보낸다 (클라이언트가 제 시계로 뒤집는다).
    let view = table_view(&table, Some(140), 5_050);
    let other = view.seats[1].as_ref().unwrap();
    assert_eq!(other.cards, cards(&["Kh", "Kc"]));
    assert_eq!(other.showdown_at, flip_at);
    assert_eq!(
        view.seats[0].as_ref().unwrap().showdown_at,
        round.showdown_reveal_at[0]
    );
    // 족보 이름은 쇼다운 연출이 끝난 뒤에.
    assert_eq!(
        table_view(&table, Some(140), until - 1).seats[1]
            .as_ref()
            .unwrap()
            .hand_name,
        None
    );
    assert_eq!(
        table_view(&table, Some(140), until).seats[1]
            .as_ref()
            .unwrap()
            .hand_name
            .as_deref(),
        Some("원 페어")
    );
    let spectator = table_view(&table, None, 5_050);
    assert!(
        spectator
            .seats
            .iter()
            .flatten()
            .all(|seat| seat.cards.iter().all(|card| card != "??"))
    );
}

#[test]
fn holdem_fold_win_keeps_the_winners_cards_hidden() {
    let mut table = heads_up_holdem(1_000);
    act(&mut table, 140, CasinoCommand::Fold, 1_100);
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.phase, Phase::Complete);
    assert!(!round.reveal);
    let view = table_view(&table, Some(140), i64::MAX);
    let winner = view.seats[1].as_ref().unwrap();
    assert_eq!(winner.cards, cards(&["??", "??"]));
    assert_eq!(winner.showdown_at, 0);
    let spectator = table_view(&table, None, i64::MAX);
    assert!(
        spectator
            .seats
            .iter()
            .flatten()
            .all(|seat| seat.cards.iter().all(|card| card == "??"))
    );
}

#[test]
fn blackjack_view_never_leaks_the_hole_card_before_settlement() {
    let mut table = blackjack_table();
    sit(&mut table, 150, 0, 10_000, 0);
    sit(&mut table, 151, 1, 10_000, 0);
    // 딜: P150 5h, P151 6h, 딜러 Th, P150 7h, P151 8h, 딜러 Qc(홀 카드).
    table
        .start_with_deck(150, deck_from_top(&["5h", "6h", "Th", "7h", "8h", "Qc"]), 0)
        .unwrap();
    for (user, at) in [(150, 100), (151, 200)] {
        act(
            &mut table,
            user,
            CasinoCommand::Bet {
                amount: 100,
                pairs: 0,
                plus3: 0,
            },
            at,
        );
    }
    let hidden_everywhere = |table: &CasinoTable| {
        for viewer in [Some(150), Some(151), None] {
            let view = table_view(table, viewer, i64::MAX);
            let round = view.round.as_ref().unwrap();
            assert_eq!(round.dealer, cards(&["Th", "??"]));
            assert_eq!(round.dealer_flip_at, 0);
            assert_eq!(round.dealer_total, None);
            assert!(
                !serde_json::to_string(&view).unwrap().contains("\"Qc\""),
                "딜러 홀 카드가 새면 안 된다"
            );
        }
    };
    hidden_everywhere(&table);
    act(&mut table, 150, CasinoCommand::Stand, 1_000);
    hidden_everywhere(&table);
    act(&mut table, 151, CasinoCommand::Stand, 2_000);
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.phase, Phase::Complete);
    // 정산된 뒤에는 실제 카드와 뒤집을 시각을 함께 보낸다.
    let view = table_view(&table, Some(151), 2_000);
    let round_view = view.round.as_ref().unwrap();
    assert_eq!(round_view.dealer, cards(&["Th", "Qc"]));
    assert_eq!(round_view.dealer_flip_at, round.dealer_flip_at);
    assert!(round_view.dealer_flip_at >= 2_000);
}

#[test]
fn blackjack_results_stay_hidden_until_the_cards_have_landed() {
    let mut table = blackjack_table();
    sit(&mut table, 160, 0, 10_000, 0);
    // 딜: 나 9h, 딜러 7c, 나 Kd, 딜러 9s → 19 대 16. 딜러 드로 Td → 26 버스트.
    let now = deal_blackjack_with(
        &mut table,
        160,
        &[(160, 1_000)],
        &["9h", "7c", "Kd", "9s", "Td"],
        0,
    );
    act(&mut table, 160, CasinoCommand::Stand, now + 500);
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.phase, Phase::Complete);
    let until = round.reveal_until;
    // 실제 칩은 바로 들어온다 (코인·캐시아웃 정산은 그대로).
    assert_eq!(table.seat(0).unwrap().stack, 11_000);

    let early = table_view(&table, Some(160), until - 1);
    let seat = early.seats[0].as_ref().unwrap();
    assert_eq!(seat.stack, 9_000, "당첨금은 카드가 다 놓인 뒤에 보인다");
    assert!(seat.hands[0].result.is_none());
    assert_eq!(early.round.as_ref().unwrap().dealer_total, None);
    assert!(early.history.is_empty(), "이번 라운드 기록도 그 뒤에");
    assert!(!early.narration.starts_with("딜러 버스트"));
    assert!(
        early
            .messages
            .iter()
            .all(|message| !message.text.starts_with("딜러 버스트"))
    );

    let late = table_view(&table, Some(160), until);
    assert_eq!(late.seats[0].as_ref().unwrap().stack, 11_000);
    assert_eq!(late.history.len(), 1);
    assert!(late.narration.starts_with("딜러 버스트"));
    assert!(
        late.messages
            .last()
            .unwrap()
            .text
            .starts_with("딜러 버스트")
    );
    // 허브는 이 공개 시각에 한 번 화면을 다시 민다.
    assert!(table.reveals_between(until - 1, until));
    assert!(!table.reveals_between(until, until + 60_000));
}

#[test]
fn side_bet_wins_show_after_the_deal_and_stay_through_later_hits() {
    let mut table = blackjack_table();
    sit(&mut table, 170, 0, 10_000, 0);
    // 딜: 나 8h, 딜러 5c, 나 8d(컬러 페어 12:1), 딜러 9s. 히트 2c(18). 딜러 드로 Tc → 24 버스트.
    table
        .start_with_deck(170, deck_from_top(&["8h", "5c", "8d", "9s", "2c", "Tc"]), 0)
        .unwrap();
    act(
        &mut table,
        170,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 100,
            plus3: 0,
        },
        100,
    );
    let landed = table.seat(0).unwrap().hands[0].reveal_at[1];
    let won_line = table
        .messages
        .iter()
        .find(|message| message.text.contains("컬러 페어"))
        .unwrap();
    assert_eq!(won_line.at, landed, "당첨 안내는 두 번째 카드가 놓일 때");
    let dealt_until = table.round.as_ref().unwrap().reveal_until;
    assert_eq!(table.seat(0).unwrap().stack, 10_000 - 600 + 1_300);
    let visible = |table: &CasinoTable, at: i64| {
        table_view(table, Some(170), at).seats[0]
            .as_ref()
            .unwrap()
            .stack
    };
    assert_eq!(visible(&table, dealt_until - 1), 9_400);
    assert_eq!(visible(&table, dealt_until), 10_700);

    // 히트 연출 동안 이미 보인 사이드 당첨금을 다시 숨기지 않는다.
    act(&mut table, 170, CasinoCommand::Hit, 1_000);
    let hit_until = table.round.as_ref().unwrap().reveal_until;
    assert_eq!(visible(&table, hit_until - 1), 10_700);
    act(&mut table, 170, CasinoCommand::Stand, 2_000);
    let settled_until = table.round.as_ref().unwrap().reveal_until;
    assert_eq!(table.seat(0).unwrap().stack, 10_700 + 1_000);
    // 정산 연출 동안에는 메인 당첨금만 숨긴다.
    assert_eq!(visible(&table, settled_until - 1), 10_700);
    assert_eq!(visible(&table, settled_until), 11_700);
}

#[test]
fn holdem_pot_shows_in_the_winners_stack_after_the_showdown() {
    let mut table = heads_up_holdem(1_000);
    check_round(&mut table, true, 1_100);
    check_round(&mut table, false, 3_000);
    check_round(&mut table, false, 4_000);
    check_round(&mut table, false, 5_000);
    let until = table.round.as_ref().unwrap().reveal_until;
    assert_eq!(
        table.seat(1).unwrap().stack,
        10_100,
        "실제 스택에는 바로 들어온다"
    );
    let stack = |at: i64| {
        table_view(&table, None, at).seats[1]
            .as_ref()
            .unwrap()
            .stack
    };
    assert_eq!(stack(until - 1), 9_900);
    assert_eq!(stack(until), 10_100);
    // 다음 핸드를 시작하면 보류분은 비워진다.
    act(&mut table, 140, CasinoCommand::Start, until + 1_000);
    assert_eq!(table.seat(1).unwrap().pending_credit, 0);
}

#[test]
fn scheduled_narration_keeps_the_previous_line_until_its_time() {
    let mut table = blackjack_table();
    table.say("첫 안내", 1_000);
    table.say_at("사이드 결과", 1_000, 1_500);
    table.say_at("결과", 1_000, 3_000);
    assert_eq!(table.narration_at(1_200), "첫 안내");
    assert_eq!(table.narration_at(1_500), "사이드 결과");
    assert_eq!(table.narration_at(2_999), "사이드 결과");
    assert_eq!(table.narration_at(3_000), "결과");
    assert_eq!(table.narration, "결과", "저장되는 안내는 가장 마지막 안내");
    let view = table_view(&table, None, 1_200);
    assert_eq!(view.narration, "첫 안내");
    assert!(
        view.messages
            .iter()
            .all(|message| message.text != "결과" && message.text != "사이드 결과")
    );
    // 지금 바로 한 말은 바로 보이지만, 예약된 결과 시각이 되면 결과가 뜬다 (가장 늦게 뜬 안내).
    table.say("새 안내", 2_000);
    assert_eq!(table.narration_at(2_000), "새 안내");
    assert_eq!(table.narration_at(2_999), "새 안내");
    assert_eq!(table.narration_at(3_500), "결과");
    assert_eq!(
        table.narration, "결과",
        "저장되는 안내는 가장 늦게 뜨는 안내"
    );
    // 예약이 없는 예전 저장본은 저장된 안내를 그대로 보여 준다.
    let old = blackjack_table();
    assert!(old.narrations.is_empty());
    assert_eq!(old.narration_at(0), old.narration);
    // 저장·로드해도 예약이 유지된다.
    table.say_at("나중 안내", 2_000, 9_000);
    let restored: CasinoTable =
        serde_json::from_str(&serde_json::to_string(&table).unwrap()).unwrap();
    assert_eq!(restored.narration_at(8_999), "결과");
    assert_eq!(restored.narration_at(9_000), "나중 안내");
}

#[test]
fn a_scheduled_summary_shows_even_after_a_later_immediate_reply() {
    let mut table = holdem_table();
    table.say("카드를 나눠드렸어요.", 1_000);
    // 정산 요약은 카드가 다 놓이는 3,000에 뜨도록 예약되고, 그 사이 채팅 답이 바로 나간다.
    table.say_at("P1 +200 (원 페어)", 1_000, 3_000);
    table.say("함께해 주셔서 반가워요.", 2_000);
    assert_eq!(table.narration_at(1_500), "카드를 나눠드렸어요.");
    assert_eq!(table.narration_at(2_000), "함께해 주셔서 반가워요.");
    assert_eq!(table.narration_at(3_000), "P1 +200 (원 페어)");
    assert_eq!(table.narration_at(60_000), "P1 +200 (원 페어)");
    // 같은 시각이면 나중에 한 말이 보인다.
    table.say_at("같은 시각 앞", 2_500, 4_000);
    table.say_at("같은 시각 뒤", 2_600, 4_000);
    assert_eq!(table.narration_at(4_000), "같은 시각 뒤");
    // 새 안내를 넣을 때 정리해도 앞으로 보일 안내는 남는다.
    table.say("지금 안내", 3_500);
    assert_eq!(table.narration_at(3_500), "지금 안내");
    assert_eq!(table.narration_at(4_000), "같은 시각 뒤");
    assert!(
        table
            .narrations
            .windows(2)
            .all(|pair| pair[0].at <= pair[1].at),
        "안내는 뜨는 시각 순: {:?}",
        table.narrations
    );
    // 예전 저장본처럼 넣은 순서로 저장된 목록도 가장 늦게 뜬 안내를 고른다.
    let mut old = holdem_table();
    old.narrations = vec![
        Narration {
            at: i64::MIN,
            text: "처음".to_string(),
        },
        Narration {
            at: 3_000,
            text: "요약".to_string(),
        },
        Narration {
            at: 2_000,
            text: "답".to_string(),
        },
    ];
    assert_eq!(old.narration_at(2_500), "답");
    assert_eq!(old.narration_at(3_000), "요약");
}

#[test]
fn old_saves_without_the_new_timing_fields_still_load() {
    let mut table = heads_up_holdem(1_000);
    check_round(&mut table, true, 1_100);
    let mut json = serde_json::to_value(&table).unwrap();
    let round = json["round"].as_object_mut().unwrap();
    round.remove("burn_at");
    round.remove("board_flip_at");
    for seat in json["seats"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter_map(|seat| seat.as_object_mut())
    {
        seat.remove("pending_credit");
        seat.remove("pending_until");
    }
    json.as_object_mut().unwrap().remove("narrations");
    let loaded: CasinoTable = serde_json::from_value(json).unwrap();
    let round = loaded.round.as_ref().unwrap();
    assert!(round.burn_at.is_empty());
    assert_eq!(round.board_flip_at, 0);
    // 뒤집기 시각이 없던 플롭은 이미 앞면이다.
    let view = table_view(&loaded, Some(140), i64::MAX);
    assert_eq!(view.round.as_ref().unwrap().board_flip_at, 0);
    assert!(view.seats[0].as_ref().unwrap().hand_name.is_some());
}

/// 헤즈업 올인: 140(버튼·스몰, 5,000)이 올인하고 141(10,000)이 콜해 런아웃으로 끝난다.
/// 홀 카드는 141, 140, 141, 140 순서로 나간다.
fn all_in_heads_up(deck: &[&str]) -> CasinoTable {
    let mut table = holdem_table();
    sit(&mut table, 140, 0, 5_000, 0);
    sit(&mut table, 141, 1, 10_000, 0);
    table
        .start_with_deck(140, deck_from_top(deck), 1_000)
        .unwrap();
    act(
        &mut table,
        140,
        CasinoCommand::Raise { amount: 5_000 },
        1_100,
    );
    act(&mut table, 141, CasinoCommand::Call, 1_200);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    table
}

fn can_start(table: &CasinoTable, viewer: Option<u64>, at: i64) -> bool {
    table_view(table, viewer, at).legal.can_start
}

#[test]
fn start_waits_for_the_reveal_and_uses_the_visible_stack() {
    // 140이 KK로 이겨 두 배(10,000)가 되거나, QQ로 져서 파산한다.
    for (deck, doubled) in [
        (
            [
                "Qd", "Kh", "Qs", "Kc", "2c", "7h", "8d", "9s", "3c", "Jd", "4c", "5h",
            ],
            true,
        ),
        (
            [
                "Kh", "Qd", "Kc", "Qs", "2c", "7h", "8d", "9s", "3c", "Jd", "4c", "5h",
            ],
            false,
        ),
    ] {
        let table = all_in_heads_up(&deck);
        let until = table.round.as_ref().unwrap().reveal_until;
        let stacks_at = |at: i64| {
            table_view(&table, None, at)
                .seats
                .iter()
                .flatten()
                .map(|seat| seat.stack)
                .collect::<Vec<_>>()
        };
        for viewer in [Some(140), Some(141), None] {
            assert!(
                !can_start(&table, viewer, until - 1),
                "카드를 다 열기 전에는 아무도 시작할 수 없다 ({viewer:?})"
            );
        }
        if doubled {
            assert_eq!(stacks_at(until - 1), vec![0, 5_000]);
            assert_eq!(stacks_at(until), vec![10_000, 5_000]);
            assert!(can_start(&table, Some(140), until));
            assert!(can_start(&table, Some(141), until));
        } else {
            assert_eq!(stacks_at(until - 1), vec![0, 5_000]);
            assert_eq!(stacks_at(until), vec![0, 15_000]);
            // 파산한 140은 칩이 없고, 141은 혼자라 시작할 수 없다.
            assert!(!can_start(&table, Some(140), until));
            assert!(!can_start(&table, Some(141), until));
        }
    }

    // 블랙잭: 5,000 전부를 걸고 스탠드. 이기면 10,000, 지면 0.
    for (deck, won) in [
        (["9h", "7c", "Kd", "9s", "Td"], true),
        (["9h", "7c", "Kd", "9s", "4d"], false),
    ] {
        let mut table = blackjack_table();
        sit(&mut table, 160, 0, 5_000, 0);
        let now = deal_blackjack_with(&mut table, 160, &[(160, 5_000)], &deck, 0);
        act(&mut table, 160, CasinoCommand::Stand, now + 500);
        let round = table.round.as_ref().unwrap();
        assert_eq!(round.phase, Phase::Complete);
        let until = round.reveal_until;
        let seat = table_view(&table, Some(160), until - 1).seats[0]
            .clone()
            .unwrap();
        assert_eq!(seat.stack, 0, "당첨금은 카드가 다 놓인 뒤에 보인다");
        for viewer in [Some(160), None] {
            assert!(!can_start(&table, viewer, until - 1));
        }
        assert_eq!(can_start(&table, Some(160), until), won);
        assert_eq!(
            table_view(&table, Some(160), until).seats[0]
                .as_ref()
                .unwrap()
                .stack,
            if won { 10_000 } else { 0 }
        );
    }
}

#[test]
fn insurance_is_offered_on_the_visible_stack() {
    let mut table = blackjack_table();
    sit(&mut table, 161, 0, 10_000, 0);
    // 딜: 나 8h, 딜러 As, 나 8d(컬러 페어 12:1), 딜러 9s → 인슈어런스.
    table
        .start_with_deck(161, deck_from_top(&["8h", "As", "8d", "9s"]), 0)
        .unwrap();
    act(
        &mut table,
        161,
        CasinoCommand::Bet {
            amount: 4_000,
            pairs: 1_000,
            plus3: 0,
        },
        100,
    );
    let round = table.round.as_mut().unwrap();
    assert_eq!(round.phase, Phase::Insurance);
    // 실제 시간표처럼 딜 연출이 남아 있고, 사이드 당첨금(13,000)도 그때까지 가려져 있다.
    round.reveal_until = 3_000;
    let seat = table.seats[0].as_mut().unwrap();
    assert_eq!(seat.stack, 5_000 + 13_000);
    seat.pending_until = 3_000;
    // 보이는 스택 5,000으로는 인슈어런스(2,000)를 걸 수 있다.
    assert!(table_view(&table, Some(161), 2_999).legal.can_insure);
    let error = table
        .apply_command(
            161,
            "P161",
            &CasinoCommand::Insure { accept: true },
            None,
            2_999,
        )
        .unwrap_err();
    assert_eq!(error.code, "REVEALING");
    // 보이는 스택이 모자라면 (가려진 당첨금이 있어도) 인슈어런스를 켜지 않는다.
    table.seats[0].as_mut().unwrap().stack = 1_000 + 13_000;
    assert!(!table_view(&table, Some(161), 2_999).legal.can_insure);
    assert!(table_view(&table, Some(161), 3_000).legal.can_insure);
    let seat = table.seats[0].as_mut().unwrap();
    seat.stack = 1_000;
    seat.pending_credit = 2_000;
    seat.pending_until = 3_000;
    assert_eq!(seat.visible_stack(2_999), 0);
}

#[test]
fn insurance_is_rejected_during_reveal_and_accepted_afterwards() {
    let mut table = blackjack_table();
    sit(&mut table, 164, 0, 10_000, 0);
    table
        .start_with_deck(164, deck_from_top(&["8h", "As", "8d", "9s"]), 0)
        .unwrap();
    act(
        &mut table,
        164,
        CasinoCommand::Bet {
            amount: 4_000,
            pairs: 1_000,
            plus3: 0,
        },
        100,
    );
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Insurance);
    let round = table.round.as_mut().unwrap();
    round.reveal_until = 3_000;
    round.deadline = 4_000;
    assert_eq!(
        table
            .apply_command(
                164,
                "P164",
                &CasinoCommand::Insure { accept: true },
                None,
                2_999,
            )
            .unwrap_err()
            .code,
        "REVEALING"
    );
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Insurance);
    assert!(!table.seat(0).unwrap().insurance_decided);
    table
        .apply_command(
            164,
            "P164",
            &CasinoCommand::Insure { accept: true },
            None,
            3_000,
        )
        .unwrap();
    assert!(table.seat(0).unwrap().insurance_decided);
}

#[test]
fn a_round_settled_before_its_cards_land_shows_the_phase_before_settlement() {
    // 블랙잭 딜러 내추럴 (업카드 T라 인슈어런스 없이 딜하자마자 정산된다).
    let mut table = blackjack_table();
    sit(&mut table, 180, 0, 10_000, 0);
    table
        .start_with_deck(180, deck_from_top(&["9h", "Td", "7c", "As"]), 0)
        .unwrap();
    act(
        &mut table,
        180,
        CasinoCommand::Bet {
            amount: 500,
            pairs: 0,
            plus3: 0,
        },
        100,
    );
    let round = table.round.as_mut().unwrap();
    assert_eq!(round.phase, Phase::Complete);
    // 실제 시간표처럼 딜 카드가 놓인 뒤 홀 카드를 뒤집고, 그 뒤에 결과를 띄운다.
    round.dealer_flip_at = 3_000;
    round.reveal_until = 3_700;
    let phase = |table: &CasinoTable, at: i64| {
        let round = table_view(table, None, at).round.unwrap();
        (round.phase, round.phase_text)
    };
    assert_eq!(
        phase(&table, 150),
        (Phase::Playing, "플레이 중".to_string())
    );
    assert_eq!(phase(&table, 2_999).0, Phase::Playing);
    assert_eq!(
        phase(&table, 3_000),
        (Phase::Complete, "라운드 종료".to_string())
    );
    assert_eq!(phase(&table, 3_700).0, Phase::Complete);
    // 허브는 홀 카드를 뒤집는 시각에도 화면을 다시 민다 (단계가 바뀐다).
    assert!(table.reveals_between(2_999, 3_000));

    // 홀덤 폴드 승리: 연출이 끝날 때까지 마지막 베팅 스트리트(플롭)를 보여 준다.
    let mut table = heads_up_holdem(1_000);
    check_round(&mut table, true, 1_100);
    act(&mut table, 141, CasinoCommand::Check, 3_000);
    act(&mut table, 140, CasinoCommand::Fold, 3_050);
    let round = table.round.as_mut().unwrap();
    assert_eq!(round.phase, Phase::Complete);
    round.reveal_until = 4_000;
    assert_eq!(phase(&table, 3_999), (Phase::Flop, "플롭".to_string()));
    assert_eq!(phase(&table, 4_000).0, Phase::Complete);

    // 올인 런아웃: 보드가 앞면으로 놓이는 대로 스트리트가 넘어간다.
    let mut table = all_in_heads_up(&[
        "Kh", "Qd", "Kc", "Qs", "2c", "7h", "8d", "9s", "3c", "Jd", "4c", "5h",
    ]);
    let round = table.round.as_mut().unwrap();
    round.board_reveal_at = vec![2_000, 2_100, 2_200, 3_000, 4_000];
    round.board_flip_at = 2_500;
    round.reveal_until = 5_000;
    for (at, expected) in [
        (1_500, Phase::River),
        (2_400, Phase::River),
        (2_500, Phase::River),
        (3_000, Phase::River),
        (4_000, Phase::River),
        (5_000, Phase::Complete),
    ] {
        assert_eq!(phase(&table, at).0, expected, "{at}");
        assert_eq!(
            shown_phase(GameKind::Holdem, table.round.as_ref().unwrap(), at),
            expected
        );
    }
}

#[test]
fn queued_settings_are_announced_once_the_cards_are_revealed() {
    let mut table = heads_up_holdem(1_000);
    let next = TableSettings::build(
        GameKind::Holdem,
        SettingsRequest {
            big_blind: Some(200),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!table.change_settings(next, 1_050));
    // 결과를 만든 카드가 아직 놓이는 중에 버튼(140)이 퇴장하며 폴드해 핸드가 끝난다.
    table.round.as_mut().unwrap().reveal_until = 5_000;
    act(&mut table, 140, CasinoCommand::Leave, 1_100);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert_eq!(table.settings, next, "설정은 라운드가 끝나면 바로 적용된다");
    let announced = table
        .messages
        .iter()
        .find(|message| message.text.starts_with("방 설정이 바뀌었습니다"))
        .unwrap();
    assert_eq!(announced.at, 5_000, "안내는 카드를 다 연 뒤에 뜬다");
    assert!(
        table_view(&table, None, 4_999)
            .messages
            .iter()
            .all(|message| !message.text.starts_with("방 설정이 바뀌었습니다"))
    );
}

#[test]
fn leaving_seats_cash_out_only_after_the_cards_are_revealed() {
    let mut table = heads_up_holdem(1_000);
    assert_eq!(stacks(&table) + committed(&table), 20_000);
    // 결과를 만든 카드가 아직 놓이는 중인 것처럼 연출 끝을 늦춘다.
    table.round.as_mut().unwrap().reveal_until = 5_000;
    // 버튼(140)이 자기 차례에 퇴장하면 폴드로 핸드가 끝난다.
    let events = act(&mut table, 140, CasinoCommand::Leave, 1_100);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert!(cash_outs(&events).is_empty(), "{events:?}");
    assert!(table.seats[0].as_ref().is_some_and(|seat| seat.leaving));
    // 이긴 141이 연출 중에 퇴장해도 (가려 둔 팟이 코인으로 새지 않게) 기다린다.
    let events = act(&mut table, 141, CasinoCommand::Leave, 1_200);
    assert!(cash_outs(&events).is_empty(), "{events:?}");
    assert!(table.seats[1].as_ref().is_some_and(|seat| seat.leaving));
    let version = table.version;
    assert!(cash_outs(&table.tick(4_999).unwrap()).is_empty());
    assert_eq!(table.version, version);
    // 연출이 끝나면 틱이 돌려주고 버전을 올린다 (허브가 알리고 저장한다).
    let events = table.tick(5_000).unwrap();
    assert_eq!(cash_outs(&events), vec![(140, 9_950), (141, 10_050)]);
    assert!(table.version > version);
    assert!(table.seats.iter().all(Option::is_none));
    assert_eq!(9_950 + 10_050, 20_000, "칩이 사라지지 않는다");
    assert!(cash_outs(&table.tick(5_250).unwrap()).is_empty());
    // 연출이 끝난 뒤의 퇴장은 바로 돌려준다.
    let mut table = heads_up_holdem(1_000);
    act(&mut table, 140, CasinoCommand::Fold, 1_100);
    let until = table.round.as_ref().unwrap().reveal_until;
    let events = act(&mut table, 141, CasinoCommand::Leave, until);
    assert_eq!(cash_outs(&events), vec![(141, 10_050)]);

    // 블랙잭: 퇴장한 좌석의 당첨금도 카드를 다 연 뒤에 코인으로 돌아간다.
    let mut table = blackjack_table();
    sit(&mut table, 162, 0, 10_000, 0);
    // 딜: 나 9h, 딜러 7c, 나 Kd, 딜러 9s → 19 대 16. 딜러 드로 Td → 버스트.
    let now = deal_blackjack_with(
        &mut table,
        162,
        &[(162, 1_000)],
        &["9h", "7c", "Kd", "9s", "Td"],
        0,
    );
    table.round.as_mut().unwrap().reveal_until = now + 5_000;
    let events = act(&mut table, 162, CasinoCommand::Leave, now + 100);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert!(cash_outs(&events).is_empty(), "{events:?}");
    assert!(cash_outs(&table.tick(now + 4_999).unwrap()).is_empty());
    assert_eq!(
        cash_outs(&table.tick(now + 5_000).unwrap()),
        vec![(162, 11_000)]
    );
}

#[test]
fn a_new_round_waits_until_the_last_one_is_revealed() {
    let mut table = blackjack_table();
    sit(&mut table, 190, 0, 10_000, 0);
    sit(&mut table, 191, 1, 10_000, 0);
    // 딜: 190 9h, 191 8h, 딜러 7c, 190 Kd(19), 191 Kc(18), 딜러 9s(16). 딜러 드로 Td → 버스트.
    table
        .start_with_deck(
            190,
            deck_from_top(&["9h", "8h", "7c", "Kd", "Kc", "9s", "Td"]),
            0,
        )
        .unwrap();
    for user in [190, 191] {
        act(
            &mut table,
            user,
            CasinoCommand::Bet {
                amount: 500,
                pairs: 0,
                plus3: 0,
            },
            100,
        );
    }
    // 191은 핸드 중에 퇴장을 예약한다.
    act(&mut table, 191, CasinoCommand::Leave, 200);
    // 190이 시간 초과로 스탠드해 라운드가 끝나는데, 카드는 아직 놓이는 중이다.
    let deadline = table.round.as_ref().unwrap().deadline;
    table.round.as_mut().unwrap().reveal_until = deadline + 5_000;
    let events = table.tick(deadline).unwrap();
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
    assert!(cash_outs(&events).is_empty());
    let until = table.round.as_ref().unwrap().reveal_until;
    assert_eq!(until, deadline + 5_000);
    // 190은 보이는 스택만으로도 충분하지만, 카드를 다 열기 전에는 시작 버튼이 없다.
    assert!(
        table_view(&table, Some(190), until - 1).seats[0]
            .as_ref()
            .is_some_and(|seat| seat.stack >= table.settings.min_bet)
    );
    assert!(!can_start(&table, Some(190), until - 1));
    assert!(can_start(&table, Some(190), until));

    let version = table.version;
    let error = table
        .apply_command(190, "P190", &CasinoCommand::Start, None, until - 1)
        .unwrap_err();
    assert_eq!(error.code, "REVEALING");
    let error = table
        .start_with_deck(190, deck_from_top(&["2h", "3h", "4h", "5h"]), until - 1)
        .unwrap_err();
    assert_eq!(error.code, "REVEALING");
    assert_eq!(
        table.version, version,
        "거절된 시작은 테이블을 바꾸지 않는다"
    );
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);

    // 연출이 끝나면 시작할 수 있고, 아직 남아 있던 퇴장 대기 좌석은 이때 칩을 돌려받는다.
    let events = act(&mut table, 190, CasinoCommand::Start, until);
    assert_eq!(table.round.as_ref().unwrap().phase, Phase::Betting);
    assert_eq!(cash_outs(&events), vec![(191, 10_500)]);
    assert!(table.seats[1].is_none());
    assert_eq!(table.seat(0).unwrap().stack, 10_500);
}

#[test]
fn house_delta_of_a_round_still_revealing_is_reported_separately() {
    let mut table = blackjack_table();
    sit(&mut table, 163, 0, 10_000, 0);
    let now = deal_blackjack_with(
        &mut table,
        163,
        &[(163, 1_000)],
        &["9h", "7c", "Kd", "9s", "Td"],
        0,
    );
    let events = act(&mut table, 163, CasinoCommand::Stand, now + 500);
    let house_delta = events
        .iter()
        .find_map(|event| match event {
            CasinoEvent::RoundSettled { house_delta, .. } => Some(*house_delta),
            _ => None,
        })
        .unwrap();
    assert_eq!(house_delta, -1_000, "정산 이벤트(코인 회계)는 그대로다");
    let until = table.round.as_ref().unwrap().reveal_until;
    assert_eq!(table.unrevealed_house_delta(until - 1), -1_000);
    assert_eq!(table.unrevealed_house_delta(until), 0);
    // 다른 라운드의 기록이나 진행 중인 라운드는 빼지 않는다.
    act(&mut table, 163, CasinoCommand::Start, until);
    assert_eq!(table.unrevealed_house_delta(until), 0);
}

#[test]
fn shoe_counter_does_not_count_cards_still_in_flight() {
    let mut table = blackjack_table();
    sit(&mut table, 82, 0, 10_000, 0);
    // 딜: 나 Th, 딜러 6s, 나 Kd, 딜러 Tc(16). 딜러는 슈 바닥의 2c를 한 장 뽑는다(18).
    table.shoe = vec!["2c".to_string(); 200];
    table.shoe.extend(deck_from_top(&["Th", "6s", "Kd", "Tc"]));
    table.shoe_total = 416;
    table.shoe_cut = 100;
    table.shuffled_at = 500;
    act(&mut table, 82, CasinoCommand::Start, 1_000);
    act(
        &mut table,
        82,
        CasinoCommand::Bet {
            amount: 100,
            pairs: 0,
            plus3: 0,
        },
        1_100,
    );
    let remaining =
        |table: &CasinoTable, at: i64| table_view(table, None, at).shoe.unwrap().remaining;
    // 딜 카드가 놓이기 전에는 아직 슈에 있는 것으로 센다.
    assert_eq!(remaining(&table, 1_099), 204);
    assert_eq!(remaining(&table, 1_100), 200);
    act(&mut table, 82, CasinoCommand::Stand, 1_200);
    let round = table.round.as_ref().unwrap();
    assert_eq!(round.phase, Phase::Complete);
    assert_eq!(round.dealer.len(), 3);
    assert_eq!(table.shoe.len(), 199);
    // 실제 시간표처럼 딜러 드로가 나중에 착지한다.
    let round = table.round.as_mut().unwrap();
    round.dealer_reveal_at[2] = 3_000;
    round.reveal_until = 3_700;
    assert_eq!(
        remaining(&table, 2_999),
        200,
        "딜러가 몇 장 뽑을지 카운터가 먼저 알려 주지 않는다"
    );
    assert_eq!(remaining(&table, 3_000), 199);
    assert_eq!(remaining(&table, 3_700), 199);
}
