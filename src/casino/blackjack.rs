// casino/blackjack.rs — 8덱 슈 블랙잭 (S17, 3:2, 피크·인슈어런스·사이드베팅)

use super::cards::{CasinoError, blackjack_value, card_value, draw, shuffled_deck};
use super::cards::{perfect_pairs, twenty_one_plus_three};
use super::table::{
    BET_WINDOW_MS, BJ_BET_STEP, BjHand, CasinoTable, DEAL_CARD_MS, DEALER_DRAW_MS, GameKind,
    HandResult, HandStatus, INSURANCE_MS, Payout, Phase, Round, SEAT_COUNT, SETTLE_PAUSE_MS,
    SIDE_BET_MIN, SeatResult, format_chips, new_id, signed_chips,
};
use rand::Rng;
use serde::Serialize;

fn new_shoe(table: &mut CasinoTable, now: i64) -> Vec<String> {
    let deck = shuffled_deck(8);
    table.shoe_total = deck.len();
    // 바닥에 20~30%를 남긴다 (위에서 70~80% 지점).
    table.shoe_cut =
        crate::system_random::rng().random_range((deck.len() + 4) / 5..=deck.len() * 3 / 10);
    table.shuffled_at = now;
    deck
}

fn return_shoe(table: &mut CasinoTable) {
    if let Some(round) = table.round.as_mut().filter(|round| round.uses_shoe) {
        table.shoe = std::mem::take(&mut round.deck);
    }
}

/// 고갈된 저장 상태에서도 실제 게임을 끝낼 수 있게 슈 바닥에 새 카드를 보충한다.
/// 명시 덱의 오류는 그대로 반환해 재현 테스트가 무작위 카드로 바뀌지 않게 한다.
fn ensure_shoe_cards(table: &mut CasinoTable, needed: usize, now: i64) {
    if !table
        .round
        .as_ref()
        .is_some_and(|round| round.uses_shoe && round.deck.len() < needed)
    {
        return;
    }
    let mut deck = new_shoe(table, now);
    let round = table.round.as_mut().expect("round exists");
    table.shoe_total += round.deck.len();
    deck.append(&mut round.deck);
    round.deck = deck;
    table.say("슈가 소진되어 새 슈를 섞습니다.", now);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BjAction {
    Hit,
    Stand,
    Double,
    Split,
    Surrender,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BjLegal {
    pub can_double: bool,
    pub can_split: bool,
    pub can_surrender: bool,
}

pub(super) fn start_blackjack(
    table: &mut CasinoTable,
    now: i64,
    deck: Option<Vec<String>>,
) -> Result<(), CasinoError> {
    let min_bet = table.settings.min_bet;
    let ready = table
        .seats
        .iter()
        .flatten()
        .any(|seat| !seat.leaving && !seat.sit_out && seat.stack >= min_bet);
    if !ready {
        return Err(CasinoError::invalid("베팅 가능한 참가자가 필요합니다."));
    }
    for seat in table.seats.iter_mut().flatten() {
        seat.in_hand = false;
        seat.hands.clear();
        seat.cards.clear();
        seat.total = 0;
        seat.bet = 0;
        seat.side_pairs = 0;
        seat.side_plus3 = 0;
        seat.insurance = 0;
        seat.insurance_decided = false;
        seat.side_net = 0;
        seat.side_won = 0;
        seat.side_paid = 0;
        seat.side_notes.clear();
    }
    let uses_shoe = deck.is_none();
    let mut shuffle_text = "";
    let deck = if let Some(deck) = deck {
        deck
    } else {
        if table.shoe.is_empty() || table.shoe.len() <= table.shoe_cut {
            shuffle_text = if table.shoe_total == 0 {
                "새 슈를 준비합니다. "
            } else {
                "컷 카드가 나왔어요. 새 슈를 섞습니다. "
            };
            table.shoe = new_shoe(table, now);
        }
        std::mem::take(&mut table.shoe)
    };
    let (reveal_until, board_reveal_at, dealer_reveal_at, dealer_flip_at, showdown_reveal_at) =
        Round::schedule_defaults(now);
    table.round = Some(Round {
        id: new_id(),
        deck,
        uses_shoe,
        board: Vec::new(),
        dealer: Vec::new(),
        phase: Phase::Betting,
        turn: -1,
        hand: 0,
        current_bet: 0,
        min_raise: 0,
        deadline: now + BET_WINDOW_MS,
        pot: 0,
        reveal: false,
        reveal_until,
        board_reveal_at,
        dealer_reveal_at,
        dealer_flip_at,
        showdown_reveal_at,
    });
    table.say(
        format!("{shuffle_text}베팅을 받습니다. 15초 안에 칩을 놓아주세요."),
        now,
    );
    Ok(())
}

fn valid_side_bet(amount: i64, max: i64) -> bool {
    amount == 0 || (SIDE_BET_MIN..=max).contains(&amount) && amount % BJ_BET_STEP == 0
}

pub(super) fn place_bet(
    table: &mut CasinoTable,
    index: usize,
    amount: i64,
    pairs: i64,
    plus3: i64,
    now: i64,
) -> Result<(), CasinoError> {
    let betting = table
        .round
        .as_ref()
        .is_some_and(|round| round.phase == Phase::Betting);
    let rules = table.settings;
    let seat = table.seats[index].as_mut().expect("seat exists");
    if !betting || seat.in_hand || seat.sit_out || seat.leaving {
        return Err(CasinoError::invalid("지금은 베팅할 수 없습니다."));
    }
    if amount < rules.min_bet || amount > rules.max_bet || amount % BJ_BET_STEP != 0 {
        return Err(CasinoError::invalid(format!(
            "{}~{} 사이, {} 단위로 베팅해 주세요.",
            format_chips(rules.min_bet),
            format_chips(rules.max_bet),
            format_chips(BJ_BET_STEP)
        )));
    }
    if rules.side_bet_max == 0 && (pairs > 0 || plus3 > 0) {
        return Err(CasinoError::invalid(
            "이 테이블은 사이드베팅을 받지 않습니다.",
        ));
    }
    if !valid_side_bet(pairs, rules.side_bet_max) || !valid_side_bet(plus3, rules.side_bet_max) {
        return Err(CasinoError::invalid(format!(
            "사이드베팅은 {}~{} 사이, {} 단위입니다.",
            format_chips(SIDE_BET_MIN),
            format_chips(rules.side_bet_max),
            format_chips(BJ_BET_STEP)
        )));
    }
    if amount + pairs + plus3 > seat.stack {
        return Err(CasinoError::invalid("테이블 칩이 부족합니다."));
    }
    seat.stack -= amount + pairs + plus3;
    seat.total = amount + pairs + plus3;
    seat.bet = amount;
    seat.side_pairs = pairs;
    seat.side_plus3 = plus3;
    seat.in_hand = true;
    seat.last_seen = now;
    seat.hands = vec![BjHand::new(Vec::new(), amount, false, false)];
    let name = seat.name.clone();
    let mut extra = Vec::new();
    if pairs > 0 {
        extra.push(format!("퍼펙트 페어 {}", format_chips(pairs)));
    }
    if plus3 > 0 {
        extra.push(format!("21+3 {}", format_chips(plus3)));
    }
    let extra_text = if extra.is_empty() {
        String::new()
    } else {
        format!(" (사이드 {})", extra.join(", "))
    };
    table.say(
        format!("{name}님, {} 칩 베팅{extra_text}.", format_chips(amount)),
        now,
    );
    // 베팅할 수 있는 좌석이 모두 베팅했으면 베팅창을 기다리지 않고 바로 딜한다.
    let everyone_in = table
        .seats
        .iter()
        .flatten()
        .all(|seat| seat.in_hand || seat.sit_out || seat.leaving || seat.stack < rules.min_bet);
    if everyone_in {
        return deal_blackjack(table, now);
    }
    Ok(())
}

pub(super) fn deal_blackjack(table: &mut CasinoTable, now: i64) -> Result<(), CasinoError> {
    let players = (0..SEAT_COUNT)
        .filter(|&index| table.seats[index].as_ref().is_some_and(|seat| seat.in_hand))
        .collect::<Vec<_>>();
    if players.is_empty() {
        let round = table.round.as_mut().expect("round exists");
        round.phase = Phase::Complete;
        round.deadline = 0;
        return_shoe(table);
        table.say(
            "베팅이 없어 라운드를 마쳤어요. 다음 라운드에 참여해 주세요.",
            now,
        );
        return Ok(());
    }
    ensure_shoe_cards(table, (players.len() + 1) * 2, now);
    // 참가자 순서대로 한 장씩, 딜러는 마지막에. 카드마다 시간차를 둔다.
    let mut at = table.round.as_ref().expect("round exists").reveal_base(now);
    for _ in 0..2 {
        for &index in &players {
            let card = draw(&mut table.round.as_mut().expect("round exists").deck)?;
            at += DEAL_CARD_MS;
            if let Some(hand) = table.seats[index]
                .as_mut()
                .and_then(|seat| seat.hands.first_mut())
            {
                hand.cards.push(card);
                hand.reveal_at.push(at);
            }
        }
        let card = draw(&mut table.round.as_mut().expect("round exists").deck)?;
        at += DEAL_CARD_MS;
        let round = table.round.as_mut().expect("round exists");
        round.dealer.push(card);
        round.dealer_reveal_at.push(at);
    }
    {
        let round = table.round.as_mut().expect("round exists");
        round.phase = Phase::Playing;
        round.reveal_until = at + DEAL_CARD_MS;
    }
    for &index in &players {
        if let Some(hand) = table.seats[index]
            .as_mut()
            .and_then(|seat| seat.hands.first_mut())
        {
            if hand.natural() {
                hand.status = HandStatus::Stand;
            }
        }
    }
    settle_side_bets(table, &players, now);
    let up_card = table.round.as_ref().expect("round exists").dealer[0].clone();
    if up_card.starts_with('A') {
        // 에볼루션 규칙: 딜러 에이스면 먼저 인슈어런스를 받고, 그다음 블랙잭을 확인한다.
        let round = table.round.as_mut().expect("round exists");
        round.phase = Phase::Insurance;
        round.turn = -1;
        round.deadline = round.reveal_until + INSURANCE_MS;
        table.say(
            "딜러가 에이스를 보여요. 인슈어런스를 받으시겠어요? 베팅의 절반이고, 딜러가 블랙잭이면 2:1로 드려요.",
            now,
        );
        return Ok(());
    }
    let dealer_total = blackjack_value(&table.round.as_ref().expect("round exists").dealer).0;
    if dealer_total == 21 {
        return settle_blackjack(table, now);
    }
    table.say("카드를 나눠드렸어요. 21에 도전해 보세요.", now);
    next_blackjack(table, now)
}

/// 딜 직후 사이드베팅(퍼펙트 페어·21+3)을 정산한다. 딴 칩은 바로 스택에 얹는다.
fn settle_side_bets(table: &mut CasinoTable, players: &[usize], now: i64) {
    let up_card = table.round.as_ref().expect("round exists").dealer[0].clone();
    let mut lines = Vec::new();
    for &index in players {
        let Some(seat) = table.seats[index].as_mut() else {
            continue;
        };
        let Some(hand) = seat.hands.first() else {
            continue;
        };
        if hand.cards.len() < 2 {
            continue;
        }
        let (first, second) = (hand.cards[0].clone(), hand.cards[1].clone());
        if seat.side_pairs > 0 {
            let stake = seat.side_pairs;
            match perfect_pairs(&first, &second) {
                Some((label, odds)) => {
                    seat.stack += stake * (odds + 1);
                    seat.side_net += stake * odds;
                    seat.side_won += stake * odds;
                    seat.side_paid += stake * (odds + 1);
                    seat.side_notes
                        .push(format!("{label} {odds}:1 {}", signed_chips(stake * odds)));
                    lines.push(format!("{}님 {label} {odds}:1!", seat.name));
                }
                None => {
                    seat.side_net -= stake;
                    seat.side_notes
                        .push(format!("퍼펙트 페어 {}", signed_chips(-stake)));
                }
            }
        }
        if seat.side_plus3 > 0 {
            let stake = seat.side_plus3;
            match twenty_one_plus_three(&first, &second, &up_card) {
                Some((label, odds)) => {
                    seat.stack += stake * (odds + 1);
                    seat.side_net += stake * odds;
                    seat.side_won += stake * odds;
                    seat.side_paid += stake * (odds + 1);
                    seat.side_notes.push(format!(
                        "21+3 {label} {odds}:1 {}",
                        signed_chips(stake * odds)
                    ));
                    lines.push(format!("{}님 21+3 {label} {odds}:1!", seat.name));
                }
                None => {
                    seat.side_net -= stake;
                    seat.side_notes
                        .push(format!("21+3 {}", signed_chips(-stake)));
                }
            }
        }
    }
    if !lines.is_empty() {
        table.say(lines.join(" "), now);
    }
}

/// 인슈어런스 받기/거절. 모두 정하면 바로 딜러 카드를 확인한다.
pub(super) fn insurance_decision(
    table: &mut CasinoTable,
    index: usize,
    accept: bool,
    now: i64,
) -> Result<(), CasinoError> {
    let open = table
        .round
        .as_ref()
        .is_some_and(|round| round.phase == Phase::Insurance);
    let seat = table.seats[index].as_mut().expect("seat exists");
    if !open || !seat.in_hand || seat.insurance_decided {
        return Err(CasinoError::invalid(
            "지금은 인슈어런스를 정할 수 없습니다.",
        ));
    }
    let name = seat.name.clone();
    if accept {
        let cost = seat.hands.first().map_or(0, |hand| hand.bet) / 2;
        if cost <= 0 || cost > seat.stack {
            return Err(CasinoError::invalid("인슈어런스를 걸 칩이 부족합니다."));
        }
        seat.stack -= cost;
        seat.total += cost;
        seat.insurance = cost;
    }
    seat.insurance_decided = true;
    seat.last_seen = now;
    table.say(
        format!(
            "{name}님, 인슈어런스 {}.",
            if accept { "받음" } else { "거절" }
        ),
        now,
    );
    let everyone = table
        .seats
        .iter()
        .flatten()
        .filter(|seat| seat.in_hand)
        .all(|seat| seat.insurance_decided);
    if everyone {
        return resolve_insurance(table, now);
    }
    Ok(())
}

/// 인슈어런스 시간이 끝나면 딜러 카드를 확인한다. 블랙잭이면 바로 정산, 아니면 플레이로 넘어간다.
pub(super) fn resolve_insurance(table: &mut CasinoTable, now: i64) -> Result<(), CasinoError> {
    {
        let round = table.round.as_mut().expect("round exists");
        if round.phase != Phase::Insurance {
            return Ok(());
        }
        round.phase = Phase::Playing;
    }
    for seat in table.seats.iter_mut().flatten() {
        if seat.in_hand {
            seat.insurance_decided = true;
        }
    }
    let dealer_total = blackjack_value(&table.round.as_ref().expect("round exists").dealer).0;
    if dealer_total == 21 {
        table.say("딜러 블랙잭! 인슈어런스를 확인할게요.", now);
        return settle_blackjack(table, now);
    }
    for seat in table.seats.iter_mut().flatten() {
        if seat.in_hand && seat.insurance > 0 {
            seat.side_net -= seat.insurance;
            seat.side_notes
                .push(format!("인슈어런스 {}", signed_chips(-seat.insurance)));
        }
    }
    table.say("딜러는 블랙잭이 아니에요. 게임을 계속합니다.", now);
    next_blackjack(table, now)
}

pub(super) fn blackjack_legal(table: &CasinoTable, index: i32) -> Option<BjLegal> {
    let round = table.round.as_ref()?;
    let seat = table.seat(index)?;
    if round.phase != Phase::Playing || round.turn != index {
        return None;
    }
    let hand = seat.hands.get(round.hand)?;
    if hand.status != HandStatus::Playing {
        return None;
    }
    let two_cards = hand.cards.len() == 2;
    Some(BjLegal {
        // 에볼루션 규칙: 처음 두 장에서 더블(스플릿 뒤에도 가능, 에이스 스플릿은 불가),
        // 스플릿은 한 번만(같은 값 두 장), 서렌더 없음.
        can_double: two_cards && seat.stack >= hand.bet && !hand.split_aces,
        can_split: two_cards
            && !hand.split
            && seat.hands.len() < 2
            && seat.stack >= hand.bet
            && card_value(&hand.cards[0]) == card_value(&hand.cards[1]),
        can_surrender: false,
    })
}

fn next_blackjack(table: &mut CasinoTable, now: i64) -> Result<(), CasinoError> {
    let turn_ms = table.settings.turn_ms;
    for index in 0..SEAT_COUNT {
        let Some(seat) = table.seats[index].as_ref() else {
            continue;
        };
        if !seat.in_hand {
            continue;
        }
        if let Some(hand) = seat
            .hands
            .iter()
            .position(|hand| hand.status == HandStatus::Playing)
        {
            let round = table.round.as_mut().expect("round exists");
            round.turn = index as i32;
            round.hand = hand;
            round.deadline = round.reveal_base(now) + turn_ms;
            return Ok(());
        }
    }
    settle_blackjack(table, now)
}

pub(super) fn settle_blackjack(table: &mut CasinoTable, now: i64) -> Result<(), CasinoError> {
    {
        let round = table
            .round
            .as_mut()
            .ok_or_else(|| CasinoError::invalid("진행 중인 라운드가 없습니다."))?;
        if round.phase == Phase::Complete {
            return Err(CasinoError::invalid("이미 정산된 라운드입니다."));
        }
        round.reveal = true;
        // 딜러가 뒤집힌 카드를 공개한 뒤 한 장씩 뽑는다.
        round.dealer_flip_at = round.reveal_base(now) + SETTLE_PAUSE_MS;
        round.reveal_until = round.dealer_flip_at + SETTLE_PAUSE_MS;
    }
    let any_live = table
        .seats
        .iter()
        .flatten()
        .flat_map(|seat| seat.hands.iter())
        .any(|hand| {
            hand.status != HandStatus::Bust
                && hand.status != HandStatus::Surrender
                && !hand.natural()
        });
    if any_live {
        loop {
            if blackjack_value(&table.round.as_ref().expect("round exists").dealer).0 >= 17 {
                break;
            }
            ensure_shoe_cards(table, 1, now);
            let round = table.round.as_mut().expect("round exists");
            let card = draw(&mut round.deck)?;
            let at = round.reveal_until - SETTLE_PAUSE_MS + DEALER_DRAW_MS;
            round.dealer.push(card);
            round.dealer_reveal_at.push(at);
            round.reveal_until = at + SETTLE_PAUSE_MS;
        }
    }
    let (dealer, dealer_cards) = {
        let round = table.round.as_ref().expect("round exists");
        (blackjack_value(&round.dealer).0, round.dealer.len())
    };
    let dealer_natural = dealer_cards == 2 && dealer == 21;
    let mut payouts = Vec::new();
    let mut results = Vec::new();
    let mut pot = 0;
    for (index, slot) in table.seats.iter_mut().enumerate() {
        let Some(seat) = slot.as_mut() else {
            continue;
        };
        if !seat.in_hand {
            continue;
        }
        if seat.insurance > 0 && dealer_natural {
            // 인슈어런스 2:1 (원금 포함 3배 반환).
            seat.stack += seat.insurance * 3;
            seat.side_net += seat.insurance * 2;
            seat.side_won += seat.insurance * 2;
            seat.side_paid += seat.insurance * 3;
            seat.side_notes.push(format!(
                "인슈어런스 2:1 {}",
                signed_chips(seat.insurance * 2)
            ));
        }
        let mut wagered = seat.side_pairs + seat.side_plus3 + seat.insurance;
        let mut net = seat.side_net;
        let mut won = seat.side_won;
        let mut paid = seat.side_paid;
        let mut labels = Vec::new();
        for hand in &mut seat.hands {
            let score = blackjack_value(&hand.cards).0;
            let (payout, label) = if hand.status == HandStatus::Surrender {
                (hand.bet / 2, "서렌더")
            } else if hand.status == HandStatus::Bust {
                (0, "버스트")
            } else if dealer_natural {
                if hand.natural() {
                    (hand.bet, "푸시")
                } else {
                    (0, "패배")
                }
            } else if hand.natural() {
                (hand.bet * 5 / 2, "블랙잭 3:2")
            } else if dealer > 21 || score > dealer {
                (hand.bet * 2, "승리")
            } else if score == dealer {
                (hand.bet, "푸시")
            } else {
                (0, "패배")
            };
            hand.payout = Some(payout);
            hand.result = Some(label.to_string());
            seat.stack += payout;
            pot += hand.bet;
            wagered += hand.bet;
            net += payout - hand.bet;
            won += (payout - hand.bet).max(0);
            if payout > hand.bet {
                paid += payout;
            }
            labels.push(label.to_string());
            payouts.push(Payout {
                name: seat.name.clone(),
                amount: payout - hand.bet,
                label: label.to_string(),
            });
        }
        results.push(SeatResult {
            user_id: seat.user_id,
            name: seat.name.clone(),
            seat: index,
            wagered,
            net,
            won,
            paid,
            label: labels.join(" / "),
            notes: seat.side_notes.clone(),
        });
    }
    let (round_id, board) = {
        let round = table.round.as_mut().expect("round exists");
        round.pot = pot;
        round.phase = Phase::Complete;
        round.turn = -1;
        round.deadline = 0;
        (round.id.clone(), round.dealer.clone())
    };
    let dealer_text = if dealer > 21 {
        "버스트".to_string()
    } else {
        dealer.to_string()
    };
    let summary = format!(
        "딜러 {dealer_text} · {}",
        results
            .iter()
            .map(|result| {
                format!(
                    "{} {} ({})",
                    result.name,
                    signed_chips(result.net),
                    result.label
                )
            })
            .collect::<Vec<_>>()
            .join(" · ")
    );
    table.say(summary.clone(), now);
    table.remember(HandResult {
        id: round_id,
        game: GameKind::Blackjack,
        at: now,
        summary,
        board,
        payouts,
        results,
    });
    return_shoe(table);
    Ok(())
}

pub(super) fn blackjack_action(
    table: &mut CasinoTable,
    actor: u64,
    action: BjAction,
    now: i64,
    timeout: bool,
) -> Result<(), CasinoError> {
    let index = table.seat_index(actor).map_or(-1, |index| index as i32);
    let Some(legal) = blackjack_legal(table, index) else {
        return Err(CasinoError::new(
            "NOT_YOUR_TURN",
            "현재 내 차례가 아닙니다.",
        ));
    };
    match action {
        BjAction::Double if !legal.can_double => {
            return Err(CasinoError::invalid("더블할 수 없습니다."));
        }
        BjAction::Split if !legal.can_split => {
            return Err(CasinoError::invalid("스플릿할 수 없습니다."));
        }
        BjAction::Surrender if !legal.can_surrender => {
            return Err(CasinoError::invalid("서렌더할 수 없습니다."));
        }
        _ => {}
    }
    let hand_index = table.round.as_ref().expect("round exists").hand;
    let needed = match action {
        BjAction::Hit | BjAction::Double => 1,
        BjAction::Split => 2,
        _ => 0,
    };
    ensure_shoe_cards(table, needed, now);
    let name = {
        let CasinoTable { seats, round, .. } = table;
        let round = round.as_mut().expect("round exists");
        // 새 카드는 딜러가 슈에서 한 장씩 꺼내 놓는 시간을 둔다. 그동안 액션은 막힌다.
        let first_at = round.reveal_base(now) + DEAL_CARD_MS;
        let deck = &mut round.deck;
        let seat = seats[index as usize].as_mut().expect("seat exists");
        let mut reveal_until = None;
        match action {
            BjAction::Hit => {
                let hand = &mut seat.hands[hand_index];
                hand.cards.push(draw(deck)?);
                hand.reveal_at.push(first_at);
                reveal_until = Some(first_at + DEAL_CARD_MS);
                let total = blackjack_value(&hand.cards).0;
                if total >= 21 {
                    hand.status = if total > 21 {
                        HandStatus::Bust
                    } else {
                        HandStatus::Stand
                    };
                }
            }
            BjAction::Stand => seat.hands[hand_index].status = HandStatus::Stand,
            BjAction::Double => {
                let bet = seat.hands[hand_index].bet;
                seat.stack -= bet;
                seat.total += bet;
                let hand = &mut seat.hands[hand_index];
                hand.bet *= 2;
                hand.cards.push(draw(deck)?);
                hand.reveal_at.push(first_at);
                reveal_until = Some(first_at + DEAL_CARD_MS);
                hand.status = if blackjack_value(&hand.cards).0 > 21 {
                    HandStatus::Bust
                } else {
                    HandStatus::Stand
                };
            }
            BjAction::Split => {
                let bet = seat.hands[hand_index].bet;
                seat.stack -= bet;
                seat.total += bet;
                let other = seat.hands[hand_index]
                    .cards
                    .pop()
                    .ok_or_else(|| CasinoError::new("ENGINE_STATE", "스플릿 상태 오류"))?;
                // 나뉜 카드는 이미 테이블 위에 있다. 두 핸드에 한 장씩 차례로 새 카드를 놓는다.
                let moved_at = seat.hands[hand_index].reveal_at.pop().unwrap_or(0);
                let second_at = first_at + DEAL_CARD_MS;
                let split_aces = seat.hands[hand_index].cards[0].starts_with('A');
                {
                    let hand = &mut seat.hands[hand_index];
                    hand.split = true;
                    hand.split_aces = split_aces;
                    hand.cards.push(draw(deck)?);
                    hand.reveal_at.push(first_at);
                }
                let second_cards = vec![other, draw(deck)?];
                let mut second = BjHand::new(second_cards, bet, true, split_aces);
                second.reveal_at = vec![moved_at, second_at];
                reveal_until = Some(second_at + DEAL_CARD_MS);
                if split_aces || blackjack_value(&seat.hands[hand_index].cards).0 == 21 {
                    seat.hands[hand_index].status = HandStatus::Stand;
                }
                if split_aces || blackjack_value(&second.cards).0 == 21 {
                    second.status = HandStatus::Stand;
                }
                seat.hands.insert(hand_index + 1, second);
            }
            BjAction::Surrender => seat.hands[hand_index].status = HandStatus::Surrender,
        }
        if let Some(until) = reveal_until {
            round.reveal_until = until;
        }
        seat.last_seen = now;
        seat.note_timeout(timeout);
        seat.name.clone()
    };
    let label = match action {
        BjAction::Hit => "히트",
        BjAction::Stand => "스탠드",
        BjAction::Double => "더블",
        BjAction::Split => "스플릿",
        BjAction::Surrender => "서렌더",
    };
    let timeout_text = if timeout { " (시간 초과)" } else { "" };
    table.say(format!("{name}님, {label}{timeout_text}."), now);
    next_blackjack(table, now)
}
