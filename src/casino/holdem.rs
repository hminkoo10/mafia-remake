// casino/holdem.rs — 2~6인 노 리밋 텍사스 홀덤 (방 설정의 블라인드, 사이드팟, 헤즈업 버튼 규칙)

use super::cards::{CasinoError, PokerRank, draw, poker_rank, shuffled_deck};
use super::table::{
    CasinoTable, DEAL_CARD_MS, GameKind, HandResult, Payout, Phase, Round, SEAT_COUNT,
    SETTLE_PAUSE_MS, SHOWDOWN_STEP_MS, STREET_PAUSE_MS, Seat, SeatResult, format_chips, new_id,
    signed_chips,
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
        seat.hands.clear();
        seat.bet = 0;
        seat.total = 0;
        seat.folded = false;
        seat.acted_at = None;
        seat.checked = false;
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
        board_reveal_at,
        dealer_reveal_at,
        dealer_flip_at,
        showdown_reveal_at,
    };
    // 홀 카드는 버튼 왼쪽부터 한 장씩 시간차를 두고 나눈다.
    let mut at = now;
    for _ in 0..2 {
        let mut index = table.button;
        for _ in 0..count {
            index = table.next_seat(index, |seat| seat.in_hand);
            let card = draw(&mut round.deck)?;
            at += DEAL_CARD_MS;
            if let Some(seat) = table.seat_mut(index) {
                seat.cards.push(card);
                seat.cards_reveal_at.push(at);
            }
        }
    }
    round.reveal_until = at + DEAL_CARD_MS;
    if let Some(seat) = table.seat_mut(small) {
        commit(seat, rules.small_blind);
    }
    if let Some(seat) = table.seat_mut(big) {
        commit(seat, rules.big_blind);
    }
    round.turn = table.next_seat(big, actionable);
    table.round = Some(round);
    table.say("카드를 나눠드렸어요. 첫 번째 베팅을 시작합니다.", now);
    advance(table, big, now)
}

fn award(
    table: &mut CasinoTable,
    awards: &mut Vec<(u64, Payout)>,
    index: usize,
    value: i64,
    label: &str,
) {
    let Some(seat) = table.seats[index].as_mut() else {
        return;
    };
    seat.stack += value;
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
    let mut awards: Vec<(u64, Payout)> = Vec::new();
    let mut reveal = false;
    if contenders.len() == 1 {
        award(table, &mut awards, contenders[0], pot, "폴드 승리");
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
            let amount = (level - previous) * contributors.len() as i64;
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
    let round_id = {
        let round = table.round.as_mut().expect("round exists");
        round.pot = pot;
        round.reveal = reveal;
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
    let summary = results
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
    if summary.is_empty() {
        table.say("핸드가 종료되었습니다.", now);
    } else {
        table.say(summary.clone(), now);
    }
    table.remember(HandResult {
        id: round_id,
        game: GameKind::Holdem,
        at: now,
        summary,
        board,
        payouts,
        results,
    });
    Ok(())
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
    let phase = {
        let round = table.round.as_mut().expect("round exists");
        round.current_bet = 0;
        round.min_raise = rules.big_blind;
        // 스트리트마다 한 장을 버린다.
        draw(&mut round.deck)?;
        let count = if round.phase == Phase::Preflop { 3 } else { 1 };
        // 베팅이 끝나고 잠깐 뜸을 들인 뒤 한 장씩 연다.
        let mut at = round.reveal_base(now) + STREET_PAUSE_MS;
        for _ in 0..count {
            let card = draw(&mut round.deck)?;
            round.board.push(card);
            round.board_reveal_at.push(at);
            at += DEAL_CARD_MS;
        }
        round.reveal_until = at;
        round.phase = match round.board.len() {
            3 => Phase::Flop,
            4 => Phase::Turn,
            _ => Phase::River,
        };
        round.phase
    };
    table.say(format!("{} 카드가 열렸어요.", phase.value()), now);
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
