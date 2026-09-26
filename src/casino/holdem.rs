// casino/holdem.rs — 2~6인 노 리밋 텍사스 홀덤 (방 설정의 블라인드, 사이드팟, 헤즈업 버튼 규칙)

use super::cards::{
    CasinoError, FOUR_OF_A_KIND, PokerRank, draw, hand_category, poker_rank, shuffled_deck,
};
use super::table::{
    CARD_FLIP_MS, CasinoTable, DEAL_LEAD_MS, GameKind, HOLE_CARD_MS, HandResult, JackpotHit,
    JackpotKind, LAND_SETTLE_MS, Payout, Phase, Round, SEAT_COUNT, SETTLE_PAUSE_MS,
    SHOWDOWN_STEP_MS, STREET_PAUSE_MS, Seat, SeatResult, format_chips, new_id, signed_chips,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PokerAction {
    Fold,
    Check,
    Call,
    Raise(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PokerLegal {
    pub to_call: i64,
    pub can_check: bool,
    pub can_raise: bool,
    pub min_raise_to: i64,
    pub max_raise_to: i64,
}

fn live(seat: &Seat) -> bool {
    seat.in_hand && !seat.folded
}

fn actionable(seat: &Seat) -> bool {
    live(seat) && seat.stack > 0
}

fn owes(current_bet: i64, seat: &Seat) -> bool {
    actionable(seat) && (seat.acted_at.is_none() || seat.bet < current_bet)
}

fn commit(seat: &mut Seat, amount: i64) {
    let pay = amount.clamp(0, seat.stack);
    seat.stack -= pay;
    seat.bet += pay;
    seat.total += pay;
}

pub(super) fn poker_legal(table: &CasinoTable, index: i32) -> Option<PokerLegal> {
    let round = table.round.as_ref()?;
    let seat = table.seat(index)?;
    if round.phase == Phase::Complete || round.turn != index || !actionable(seat) {
        return None;
    }
    let to_call = (round.current_bet - seat.bet).max(0);
    let reopened = match seat.acted_at {
        None => true,
        Some(acted_at) => {
            (seat.checked && round.current_bet > 0)
                || round.current_bet - acted_at >= round.min_raise
        }
    };
    let opponent_can_call = table
        .occupied_seats()
        .any(|(other, seat)| other as i32 != index && actionable(seat));
    Some(PokerLegal {
        to_call: to_call.min(seat.stack),
        can_check: to_call == 0,
        can_raise: reopened && opponent_can_call && seat.stack > to_call,
        min_raise_to: if round.current_bet < table.settings.big_blind {
            table.settings.big_blind
        } else {
            round.current_bet + round.min_raise
        },
        max_raise_to: seat.bet + seat.stack,
    })
}

pub(super) fn start_poker(
    table: &mut CasinoTable,
    now: i64,
    deck: Option<Vec<String>>,
) -> Result<(), CasinoError> {
    let ready = table
        .seats
        .iter()
        .flatten()
        .filter(|seat| !seat.sit_out && !seat.leaving && seat.stack > 0)
        .count();
    if ready < 2 {
        return Err(CasinoError::invalid(
            "홀덤은 칩이 있는 참가자 2명 이상 필요합니다.",
        ));
    }
    let previous_big_blind = table.big_blind_seat;
    let rules = table.settings;
    for seat in table.seats.iter_mut().flatten() {
        seat.in_hand = !seat.sit_out && !seat.leaving && seat.stack > 0;
        seat.cards.clear();
        seat.cards_reveal_at.clear();
        seat.shown = false;
        seat.hands.clear();
        seat.bet = 0;
        seat.total = 0;
        seat.folded = false;
        seat.acted_at = None;
        seat.checked = false;
        seat.clear_pending_credit();
    }
    table.button = table.next_seat(table.button, |seat| seat.in_hand);
    let count = table
        .seats
        .iter()
        .flatten()
        .filter(|seat| seat.in_hand)
        .count();
    if count == 2 && previous_big_blind >= 0 {
        // 헤즈업: 빅 블라인드였던 사람이 다음 핸드 버튼(스몰 블라인드)이 되도록 맞춘다.
        let next_big_blind = table.next_seat(previous_big_blind, |seat| seat.in_hand);
        table.button = table.next_seat(next_big_blind, |seat| seat.in_hand);
    }
    // 헤즈업에서는 버튼이 스몰 블라인드다.
    let small = if count == 2 {
        table.button
    } else {
        table.next_seat(table.button, |seat| seat.in_hand)
    };
    let big = table.next_seat(small, |seat| seat.in_hand);
    table.big_blind_seat = big;
    let (reveal_until, board_reveal_at, dealer_reveal_at, dealer_flip_at, showdown_reveal_at) =
        Round::schedule_defaults(now);
    let mut round = Round {
        id: new_id(),
        deck: deck.unwrap_or_else(|| shuffled_deck(1)),
        uses_shoe: false,
        board: Vec::new(),
        dealer: Vec::new(),
        phase: Phase::Preflop,
        turn: -1,
        hand: 0,
        current_bet: rules.big_blind,
        min_raise: rules.big_blind,
        deadline: now + rules.turn_ms,
        pot: 0,
        reveal: false,
        reveal_until,
        settled_from: None,
        board_reveal_at,
        board_flip_at: 0,
        burn_at: Vec::new(),
        dealer_reveal_at,
        dealer_flip_at,
        showdown_reveal_at,
    };
    // 홀 카드는 버튼 왼쪽부터 한 장씩 두 바퀴 나눈다. `reveal_at`은 착지 시각이고, 착지 간격이
    // 비행 시간보다 길어 공중에는 한 장씩만 난다.
    let mut next_at = now + DEAL_LEAD_MS;
    let mut last_at = now;
    for _ in 0..2 {
        let mut index = table.button;
        for _ in 0..count {
            index = table.next_seat(index, |seat| seat.in_hand);
            let card = draw(&mut round.deck)?;
            last_at = next_at;
            next_at += HOLE_CARD_MS;
            if let Some(seat) = table.seat_mut(index) {
                seat.cards.push(card);
                seat.cards_reveal_at.push(last_at);
            }
        }
    }
    round.reveal_until = last_at + LAND_SETTLE_MS;
    let dealt_until = round.reveal_until;
    if let Some(seat) = table.seat_mut(small) {
        commit(seat, rules.small_blind);
    }
    if let Some(seat) = table.seat_mut(big) {
        commit(seat, rules.big_blind);
    }
    round.turn = table.next_seat(big, actionable);
    table.round = Some(round);
    table.say_at(
        "카드를 나눠드렸어요. 첫 번째 베팅을 시작합니다.",
        now,
        dealt_until,
    );
    advance(table, big, now)
}

/// 팟 몫을 기록한다. 칩은 연출 시각이 정해진 뒤 `settle_poker`가 한꺼번에 스택에 넣는다.
fn award(
    table: &CasinoTable,
    awards: &mut Vec<(u64, Payout)>,
    index: usize,
    value: i64,
    label: &str,
) {
    let Some(seat) = table.seats[index].as_ref() else {
        return;
    };
    if let Some((_, payout)) = awards
        .iter_mut()
        .find(|(user_id, _)| *user_id == seat.user_id)
    {
        payout.amount += value;
        // 미매칭 베팅 반환은 이미 붙은 족보 이름을 덮어쓰지 않는다.
        if label != "미매칭 베팅 반환" {
            payout.label = label.to_string();
        }
    } else {
        awards.push((
            seat.user_id,
            Payout {
                name: seat.name.clone(),
                amount: value,
                label: label.to_string(),
            },
        ));
    }
}

fn seat_at(table: &CasinoTable, index: usize) -> &Seat {
    table.seats[index].as_ref().expect("participant seat")
}

pub(super) fn settle_poker(table: &mut CasinoTable, now: i64) -> Result<(), CasinoError> {
    if !table.playing() {
        return Err(CasinoError::invalid("이미 정산된 라운드입니다."));
    }
    let board = table
        .round
        .as_ref()
        .map(|round| round.board.clone())
        .unwrap_or_default();
    let button = table.button;
    let participants = (0..SEAT_COUNT)
        .filter(|&index| table.seats[index].as_ref().is_some_and(|seat| seat.in_hand))
        .collect::<Vec<_>>();
    let contenders = participants
        .iter()
        .copied()
        .filter(|&index| live(seat_at(table, index)))
        .collect::<Vec<_>>();
    let pot = participants
        .iter()
        .map(|&index| seat_at(table, index).total)
        .sum::<i64>();
    // 레이크는 플롭을 연 핸드에서만 뗀다 (노 플롭 노 드롭). 아무도 받지 않은 베팅(맨 위 한 사람의
    // 초과분)은 돌려줄 칩이라 떼지 않는다.
    let contested = {
        let mut totals = participants
            .iter()
            .map(|&index| seat_at(table, index).total)
            .collect::<Vec<_>>();
        totals.sort_unstable_by(|left, right| right.cmp(left));
        let unmatched = match totals.as_slice() {
            [top, second, ..] => top - second,
            [only] => *only,
            [] => 0,
        };
        pot - unmatched
    };
    let rake = if board.len() >= 3 {
        table.rake.on(contested)
    } else {
        0
    };
    let mut rake_left = rake;
    let mut awards: Vec<(u64, Payout)> = Vec::new();
    let mut reveal = false;
    if contenders.len() == 1 {
        award(table, &mut awards, contenders[0], pot - rake, "폴드 승리");
        rake_left = 0;
    } else {
        reveal = true;
        let mut levels = participants
            .iter()
            .map(|&index| seat_at(table, index).total)
            .filter(|total| *total > 0)
            .collect::<Vec<_>>();
        levels.sort_unstable();
        levels.dedup();
        let mut previous = 0;
        for level in levels {
            let contributors = participants
                .iter()
                .copied()
                .filter(|&index| seat_at(table, index).total >= level)
                .collect::<Vec<_>>();
            let mut amount = (level - previous) * contributors.len() as i64;
            previous = level;
            if contributors.len() == 1 {
                award(
                    table,
                    &mut awards,
                    contributors[0],
                    amount,
                    "미매칭 베팅 반환",
                );
                continue;
            }
            let eligible = contributors
                .iter()
                .copied()
                .filter(|&index| live(seat_at(table, index)))
                .collect::<Vec<_>>();
            if eligible.is_empty() {
                return Err(CasinoError::new("ENGINE_STATE", "팟 정산 상태 오류"));
            }
            // 레이크는 메인 팟부터 뗀다 (작은 메인 팟이 모자라면 다음 사이드 팟에서).
            let take = rake_left.min(amount);
            amount -= take;
            rake_left -= take;
            let ranked = eligible
                .iter()
                .map(|&index| {
                    let mut cards = seat_at(table, index).cards.clone();
                    cards.extend(board.iter().cloned());
                    poker_rank(&cards).map(|rank| (index, rank))
                })
                .collect::<Result<Vec<(usize, PokerRank)>, CasinoError>>()?;
            let best = ranked
                .iter()
                .map(|(_, rank)| rank.score)
                .max()
                .unwrap_or(-1);
            let mut winners = ranked
                .iter()
                .filter(|(_, rank)| rank.score == best)
                .collect::<Vec<_>>();
            // 남는 칩은 버튼 왼쪽부터 준다.
            winners.sort_by_key(|(index, _)| {
                (*index as i32 - button + 5).rem_euclid(SEAT_COUNT as i32)
            });
            let share = amount / winners.len() as i64;
            let mut remainder = amount % winners.len() as i64;
            for (index, rank) in winners {
                let extra = if remainder > 0 {
                    remainder -= 1;
                    1
                } else {
                    0
                };
                award(table, &mut awards, *index, share + extra, &rank.name);
            }
        }
    }
    if rake_left != 0 {
        return Err(CasinoError::new("ENGINE_STATE", "레이크 정산 상태 오류"));
    }
    let jackpot = if reveal {
        bad_beat(table, &contenders, &board)
    } else {
        None
    };
    let round_id = {
        let round = table.round.as_mut().expect("round exists");
        round.pot = pot;
        round.reveal = reveal;
        round.settled_from = Some(round.phase);
        round.phase = Phase::Complete;
        round.turn = -1;
        round.deadline = 0;
        // 쇼다운: 버튼 왼쪽부터 한 명씩 카드를 공개하고, 마지막 공개 뒤에 결과를 띄운다.
        let base = round.reveal_base(now) + SETTLE_PAUSE_MS;
        if round.showdown_reveal_at.len() < SEAT_COUNT {
            round.showdown_reveal_at.resize(SEAT_COUNT, 0);
        }
        if reveal {
            let mut ordered = contenders.clone();
            ordered.sort_by_key(|index| {
                (*index as i32 - button + SEAT_COUNT as i32 - 1).rem_euclid(SEAT_COUNT as i32)
            });
            let mut at = base;
            for index in ordered {
                round.showdown_reveal_at[index] = at;
                at += SHOWDOWN_STEP_MS;
            }
            round.reveal_until = at - SHOWDOWN_STEP_MS + SETTLE_PAUSE_MS;
        } else {
            round.reveal_until = base;
        }
        round.id.clone()
    };
    // 딴 칩은 스택에 바로 넣되, 화면에는 쇼다운 연출이 끝난 뒤에 드러난다.
    let settled_until = table.round.as_ref().expect("round exists").reveal_until;
    for (user_id, payout) in &awards {
        let seat = table
            .seat_index(*user_id)
            .and_then(|index| table.seats[index].as_mut());
        if let Some(seat) = seat {
            seat.credit_after(payout.amount, settled_until, now);
        }
    }
    // 참가한 모든 좌석의 순손익. 진 사람도 족보(쇼다운) 또는 "폴드"로 남긴다.
    let results = participants
        .iter()
        .map(|&index| {
            let seat = seat_at(table, index);
            let award = awards
                .iter()
                .find(|(user_id, _)| *user_id == seat.user_id)
                .map(|(_, payout)| payout);
            let returned = award.map_or(0, |payout| payout.amount);
            let label = award
                .map(|payout| payout.label.clone())
                .or_else(|| {
                    if !live(seat) {
                        Some("폴드".to_string())
                    } else if reveal {
                        let mut cards = seat.cards.clone();
                        cards.extend(board.iter().cloned());
                        poker_rank(&cards).ok().map(|rank| rank.name)
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "패배".to_string());
            SeatResult {
                user_id: seat.user_id,
                name: seat.name.clone(),
                seat: index,
                wagered: seat.total,
                net: returned - seat.total,
                won: (returned - seat.total).max(0),
                // 딴 핸드는 가져간 팟 전체를 보여 준다.
                paid: if returned > seat.total { returned } else { 0 },
                label,
                notes: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    let payouts = awards
        .into_iter()
        .map(|(_, payout)| payout)
        .collect::<Vec<_>>();
    let mut summary = results
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
        .join(" · ");
    if rake > 0 && !summary.is_empty() {
        summary.push_str(&format!(" · 레이크 {}", format_chips(rake)));
    }
    // 결과 안내는 쇼다운 카드가 모두 공개된 뒤에 뜬다.
    if summary.is_empty() {
        table.say_at("핸드가 종료되었습니다.", now, settled_until);
    } else {
        table.say_at(summary.clone(), now, settled_until);
    }
    table.remember(HandResult {
        id: round_id,
        game: GameKind::Holdem,
        at: now,
        summary,
        board,
        payouts,
        results,
        rake,
        jackpot,
    });
    Ok(())
}

/// 배드비트: 쇼다운에서 포카드 이상으로 진 사람. 보드만으로 이미 그 족보면(보드 포카드 등)
/// 홀 카드가 만든 족보가 아니므로 치지 않는다.
fn bad_beat(table: &CasinoTable, contenders: &[usize], board: &[String]) -> Option<JackpotHit> {
    if board.len() != 5 {
        return None;
    }
    let board_category = hand_category(poker_rank(board).ok()?.score);
    let ranked = contenders
        .iter()
        .filter_map(|&index| {
            let seat = table.seats[index].as_ref()?;
            let mut cards = seat.cards.clone();
            cards.extend(board.iter().cloned());
            poker_rank(&cards).ok().map(|rank| (seat.user_id, rank))
        })
        .collect::<Vec<_>>();
    let best = ranked.iter().map(|(_, rank)| rank.score).max()?;
    let mut losers = ranked
        .iter()
        .filter(|(_, rank)| {
            let category = hand_category(rank.score);
            rank.score < best && category >= FOUR_OF_A_KIND && category > board_category
        })
        .collect::<Vec<_>>();
    if losers.is_empty() {
        return None;
    }
    losers.sort_by_key(|(_, rank)| std::cmp::Reverse(rank.score));
    let hand = losers[0].1.name.clone();
    Some(JackpotHit {
        kind: JackpotKind::BadBeat,
        hitters: losers.iter().map(|(user_id, _)| *user_id).collect(),
        winners: ranked
            .iter()
            .filter(|(_, rank)| rank.score == best)
            .map(|(user_id, _)| *user_id)
            .collect(),
        hand,
    })
}

fn advance(table: &mut CasinoTable, from: i32, now: i64) -> Result<(), CasinoError> {
    let rules = table.settings;
    let live_count = table
        .seats
        .iter()
        .flatten()
        .filter(|seat| live(seat))
        .count();
    if live_count == 1 {
        return settle_poker(table, now);
    }
    let funded = (0..SEAT_COUNT)
        .filter(|&index| table.seats[index].as_ref().is_some_and(actionable))
        .collect::<Vec<_>>();
    let current_bet = table.round.as_ref().expect("round exists").current_bet;
    if funded.len() == 1 {
        if let Some(seat) = table.seats[funded[0]].as_mut() {
            if seat.bet >= current_bet {
                seat.acted_at = Some(current_bet);
            }
        }
    }
    let next = table.next_seat(from, |seat| owes(current_bet, seat));
    if next != -1 {
        let round = table.round.as_mut().expect("round exists");
        round.turn = next;
        round.deadline = round.reveal_base(now) + rules.turn_ms;
        return Ok(());
    }
    if table.round.as_ref().expect("round exists").phase == Phase::River {
        return settle_poker(table, now);
    }
    for seat in table.seats.iter_mut().flatten() {
        seat.bet = 0;
        seat.acted_at = None;
        seat.checked = false;
    }
    let (phase, opened_at) = {
        let round = table.round.as_mut().expect("round exists");
        round.current_bet = 0;
        round.min_raise = rules.big_blind;
        // 실제 카지노처럼 베팅이 끝나고 잠깐 뜸을 들인 뒤 한 장을 뒷면으로 버리고(번),
        // 플롭은 세 장을 뒷면으로 한 장씩 놓았다가 함께 뒤집는다. 턴·리버는 앞면으로 놓는다.
        let burn_at = round.reveal_base(now) + STREET_PAUSE_MS;
        draw(&mut round.deck)?;
        round.burn_at.push(burn_at);
        let flop = round.phase == Phase::Preflop;
        let count = if flop { 3 } else { 1 };
        let mut at = burn_at;
        for _ in 0..count {
            at += HOLE_CARD_MS;
            let card = draw(&mut round.deck)?;
            round.board.push(card);
            round.board_reveal_at.push(at);
        }
        round.reveal_until = if flop {
            round.board_flip_at = at + LAND_SETTLE_MS;
            round.board_flip_at + CARD_FLIP_MS
        } else {
            at + LAND_SETTLE_MS
        };
        round.phase = match round.board.len() {
            3 => Phase::Flop,
            4 => Phase::Turn,
            _ => Phase::River,
        };
        (round.phase, round.reveal_until)
    };
    table.say_at(
        format!("{} 카드가 열렸어요.", phase.value()),
        now,
        opened_at,
    );
    let button = table.button;
    if funded.len() <= 1 {
        return advance(table, button, now);
    }
    let turn = table.next_seat(button, actionable);
    let round = table.round.as_mut().expect("round exists");
    round.turn = turn;
    round.deadline = round.reveal_base(now) + rules.turn_ms;
    Ok(())
}

pub(super) fn poker_action(
    table: &mut CasinoTable,
    actor: u64,
    action: PokerAction,
    now: i64,
    timeout: bool,
) -> Result<(), CasinoError> {
    let index = table.seat_index(actor).map_or(-1, |index| index as i32);
    let Some(legal) = poker_legal(table, index) else {
        return Err(CasinoError::new(
            "NOT_YOUR_TURN",
            "현재 내 차례가 아닙니다.",
        ));
    };
    let (current_bet, min_raise) = {
        let round = table.round.as_ref().expect("round exists");
        (round.current_bet, round.min_raise)
    };
    // 검증을 모두 끝낸 뒤에 상태를 바꾼다 (오류 시 테이블이 변하지 않게).
    match action {
        PokerAction::Check if !legal.can_check => {
            return Err(CasinoError::invalid("콜하거나 폴드해야 합니다."));
        }
        PokerAction::Call if legal.can_check => {
            return Err(CasinoError::invalid("콜할 베팅이 없습니다."));
        }
        PokerAction::Raise(value) => {
            if !(legal.can_raise && value > current_bet && value <= legal.max_raise_to) {
                return Err(CasinoError::invalid("레이즈 금액을 확인해 주세요."));
            }
            if !(value >= legal.min_raise_to || value == legal.max_raise_to) {
                return Err(CasinoError::invalid(format!(
                    "최소 레이즈는 {} 칩입니다.",
                    legal.min_raise_to
                )));
            }
        }
        _ => {}
    }
    let mut new_current_bet = current_bet;
    let mut new_min_raise = min_raise;
    let (name, bet_after) = {
        let seat = table.seat_mut(index).expect("seat exists");
        seat.checked = false;
        match action {
            PokerAction::Fold => seat.folded = true,
            PokerAction::Check => seat.checked = current_bet == 0,
            PokerAction::Call => commit(seat, legal.to_call),
            PokerAction::Raise(value) => {
                let increment = value - current_bet;
                if increment >= min_raise {
                    new_min_raise = increment;
                }
                commit(seat, value - seat.bet);
                new_current_bet = value;
            }
        }
        seat.acted_at = Some(new_current_bet);
        seat.last_seen = now;
        seat.note_timeout(timeout);
        (seat.name.clone(), seat.bet)
    };
    {
        let round = table.round.as_mut().expect("round exists");
        round.current_bet = new_current_bet;
        round.min_raise = new_min_raise;
    }
    let label = match action {
        PokerAction::Fold => "폴드".to_string(),
        PokerAction::Check => "체크".to_string(),
        PokerAction::Call => "콜".to_string(),
        PokerAction::Raise(_) => format!("레이즈 {}", format_chips(bet_after)),
    };
    let timeout_text = if timeout { " (시간 초과)" } else { "" };
    table.say(format!("{name}님, {label}{timeout_text}."), now);
    advance(table, index, now)
}
