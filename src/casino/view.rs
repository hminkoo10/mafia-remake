// casino/view.rs — 플레이어별로 공개 가능한 상태만 담은 투영 (비공개 카드는 "??")

use super::blackjack::BjLegal;
use super::cards::blackjack_value;
use super::holdem::PokerLegal;
use super::table::{
    BET_WINDOW_MS, BJ_BET_STEP, BJ_MAX_BET, BJ_MIN_BET, BUY_IN_STEP, CasinoTable, ChatMessage,
    GameKind, HOLDEM_BIG_BLIND, HOLDEM_SMALL_BLIND, HandResult, HandStatus, MAX_BUY_IN, MIN_BUY_IN,
    Phase, SEAT_COUNT, TURN_MS,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HandView {
    pub id: String,
    pub cards: Vec<String>,
    pub bet: i64,
    pub total: i64,
    pub soft: bool,
    pub status: HandStatus,
    pub result: Option<String>,
    pub payout: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeatView {
    pub seat: usize,
    pub user_id: u64,
    pub name: String,
    pub stack: i64,
    pub mine: bool,
    pub bet: i64,
    pub total: i64,
    pub folded: bool,
    pub in_hand: bool,
    pub sit_out: bool,
    pub leaving: bool,
    pub cards: Vec<String>,
    pub hands: Vec<HandView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoundView {
    pub id: String,
    pub phase: Phase,
    pub phase_text: String,
    pub board: Vec<String>,
    pub dealer: Vec<String>,
    pub dealer_total: Option<i64>,
    pub turn: i32,
    pub hand: usize,
    pub deadline: i64,
    pub current_bet: i64,
    pub pot: i64,
    pub reveal: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegalView {
    pub poker: Option<PokerLegal>,
    pub blackjack: Option<BjLegal>,
    pub can_bet: bool,
    pub can_start: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableRules {
    pub small_blind: i64,
    pub big_blind: i64,
    pub min_buy_in: i64,
    pub max_buy_in: i64,
    pub buy_in_step: i64,
    pub min_bet: i64,
    pub max_bet: i64,
    pub bet_step: i64,
    pub seat_count: usize,
    pub turn_ms: i64,
    pub bet_window_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableView {
    pub id: String,
    pub kind: GameKind,
    pub name: String,
    pub version: u64,
    pub button: i32,
    pub my_seat: i32,
    pub narration: String,
    pub seats: Vec<Option<SeatView>>,
    pub round: Option<RoundView>,
    pub legal: LegalView,
    pub messages: Vec<ChatMessage>,
    pub history: Vec<HandResult>,
    pub rules: TableRules,
}

pub fn table_rules() -> TableRules {
    TableRules {
        small_blind: HOLDEM_SMALL_BLIND,
        big_blind: HOLDEM_BIG_BLIND,
        min_buy_in: MIN_BUY_IN,
        max_buy_in: MAX_BUY_IN,
        buy_in_step: BUY_IN_STEP,
        min_bet: BJ_MIN_BET,
        max_bet: BJ_MAX_BET,
        bet_step: BJ_BET_STEP,
        seat_count: SEAT_COUNT,
        turn_ms: TURN_MS,
        bet_window_ms: BET_WINDOW_MS,
    }
}

/// `viewer`가 보는 테이블. None이면 관전자(모든 비공개 카드가 가려진다).
pub fn table_view(table: &CasinoTable, viewer: Option<u64>) -> TableView {
    let round = table.round.as_ref();
    let active = table.playing();
    let my_seat = viewer
        .and_then(|viewer| table.seat_index(viewer))
        .map_or(-1, |index| index as i32);
    let reveal = round.is_some_and(|round| round.reveal);
    let seats = table
        .seats
        .iter()
        .enumerate()
        .map(|(index, seat)| {
            let seat = seat.as_ref()?;
            let mine = viewer == Some(seat.user_id);
            Some(SeatView {
                seat: index,
                user_id: seat.user_id,
                name: seat.name.clone(),
                stack: seat.stack,
                mine,
                bet: seat.bet,
                total: seat.total,
                folded: seat.folded,
                in_hand: seat.in_hand,
                sit_out: seat.sit_out,
                leaving: seat.leaving,
                cards: seat
                    .cards
                    .iter()
                    .map(|card| {
                        if mine || (reveal && !seat.folded) {
                            card.clone()
                        } else {
                            "??".to_string()
                        }
                    })
                    .collect(),
                hands: seat
                    .hands
                    .iter()
                    .map(|hand| {
                        let (total, soft) = blackjack_value(&hand.cards);
                        HandView {
                            id: hand.id.clone(),
                            cards: hand.cards.clone(),
                            bet: hand.bet,
                            total,
                            soft,
                            status: hand.status,
                            result: hand.result.clone(),
                            payout: hand.payout,
                        }
                    })
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    let round_view = round.map(|round| RoundView {
        id: round.id.clone(),
        phase: round.phase,
        phase_text: round.phase.value().to_string(),
        board: round.board.clone(),
        dealer: round
            .dealer
            .iter()
            .enumerate()
            .map(|(index, card)| {
                if round.reveal || index == 0 {
                    card.clone()
                } else {
                    "??".to_string()
                }
            })
            .collect(),
        dealer_total: round.reveal.then(|| blackjack_value(&round.dealer).0),
        turn: round.turn,
        hand: round.hand,
        deadline: round.deadline,
        current_bet: round.current_bet,
        pot: if table.kind == GameKind::Holdem && active {
            table.seats.iter().flatten().map(|seat| seat.total).sum()
        } else {
            round.pot
        },
        reveal: round.reveal,
    });
    let minimum = if table.kind == GameKind::Holdem {
        1
    } else {
        BJ_MIN_BET
    };
    let my = table.seat(my_seat);
    let ready_count = table
        .seats
        .iter()
        .flatten()
        .filter(|seat| !seat.sit_out && !seat.leaving && seat.stack >= minimum)
        .count();
    let legal = LegalView {
        poker: table.poker_legal_for(my_seat),
        blackjack: table.blackjack_legal_for(my_seat),
        can_bet: table.kind == GameKind::Blackjack
            && round.is_some_and(|round| round.phase == Phase::Betting)
            && my.is_some_and(|seat| !seat.in_hand && !seat.sit_out && !seat.leaving),
        can_start: !active
            && my.is_some_and(|seat| !seat.sit_out && !seat.leaving && seat.stack >= minimum)
            && ready_count >= if table.kind == GameKind::Holdem { 2 } else { 1 },
    };
    TableView {
        id: table.id.clone(),
        kind: table.kind,
        name: table.name.clone(),
        version: table.version,
        button: table.button,
        my_seat,
        narration: table.narration.clone(),
        seats,
        round: round_view,
        legal,
        messages: table.messages.clone(),
        history: table.history.clone(),
        rules: table_rules(),
    }
}
