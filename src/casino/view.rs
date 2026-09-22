// casino/view.rs — 플레이어별로 공개 가능한 상태만 담은 투영 (비공개 카드는 "??")

use super::blackjack::BjLegal;
use super::cards::{best_hand, blackjack_value};
use super::holdem::PokerLegal;
use super::table::{
    BET_WINDOW_MS, BJ_BET_STEP, BJ_MAX_BET, BJ_MIN_BET, BUY_IN_STEP, CasinoTable, ChatMessage,
    GameKind, HOLDEM_BIG_BLIND, HOLDEM_SMALL_BLIND, HandResult, HandStatus, INSURANCE_MS,
    MAX_BUY_IN, MIN_BUY_IN, Phase, SEAT_COUNT, SIDE_BET_MAX, SIDE_BET_MIN, TURN_MS,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HandView {
    pub id: String,
    pub cards: Vec<String>,
    /// 카드별 등장 시각 (클라이언트가 이 시각까지 카드를 숨기고 딜 애니메이션을 낸다).
    pub reveal_at: Vec<i64>,
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
    /// 홀 카드별 등장 시각.
    pub cards_reveal_at: Vec<i64>,
    pub hands: Vec<HandView>,
    /// 블랙잭 사이드베팅·인슈어런스.
    pub side_pairs: i64,
    pub side_plus3: i64,
    pub insurance: i64,
    pub insurance_decided: bool,
    pub side_notes: Vec<String>,
    /// 홀덤: 지금 만들어진 족보 이름 (내 좌석은 항상, 다른 좌석은 쇼다운에서만).
    pub hand_name: Option<String>,
    /// 그 족보를 이루는 카드 (강조 표시용, 보드 카드 포함).
    pub hand_cards: Vec<String>,
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
    /// 카드 연출이 끝나는 시각. 그 전에는 액션 버튼을 숨기고 결과도 띄우지 않는다.
    pub reveal_until: i64,
    pub board_reveal_at: Vec<i64>,
    pub dealer_reveal_at: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegalView {
    pub poker: Option<PokerLegal>,
    pub blackjack: Option<BjLegal>,
    pub can_bet: bool,
    pub can_start: bool,
    /// 인슈어런스를 정할 수 있다 (딜러 에이스, 아직 미결정).
    pub can_insure: bool,
    /// 인슈어런스 비용 (베팅의 절반).
    pub insurance_cost: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DealerView {
    pub id: String,
    pub name: String,
    pub tagline: String,
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
    pub side_bet_min: i64,
    pub side_bet_max: i64,
    pub insurance_ms: i64,
    pub seat_count: usize,
    pub turn_ms: i64,
    pub bet_window_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShoeView {
    pub remaining: usize,
    pub total: usize,
    pub cut_at: usize,
    pub shuffled_at: i64,
    pub reshuffle_due: bool,
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
    /// 현재 딜러 (이름·초상 id).
    pub dealer: DealerView,
    pub shoe: Option<ShoeView>,
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
        side_bet_min: SIDE_BET_MIN,
        side_bet_max: SIDE_BET_MAX,
        insurance_ms: INSURANCE_MS,
        seat_count: SEAT_COUNT,
        turn_ms: TURN_MS,
        bet_window_ms: BET_WINDOW_MS,
    }
}

/// `viewer`가 보는 테이블. None이면 관전자(모든 비공개 카드가 가려진다).
/// `now`는 서버 시각(ms): 아직 공개 시각이 안 된 비밀 카드는 가려서 보낸다.
pub fn table_view(table: &CasinoTable, viewer: Option<u64>, now: i64) -> TableView {
    let round = table.round.as_ref();
    let active = table.playing();
    let my_seat = viewer
        .and_then(|viewer| table.seat_index(viewer))
        .map_or(-1, |index| index as i32);
    let reveal = round.is_some_and(|round| round.reveal);
    let reveal_done = round.is_none_or(|round| now >= round.reveal_until);
    let showdown_at = |index: usize| -> i64 {
        round
            .and_then(|round| round.showdown_reveal_at.get(index).copied())
            .unwrap_or(0)
    };
    let seats = table
        .seats
        .iter()
        .enumerate()
        .map(|(index, seat)| {
            let seat = seat.as_ref()?;
            let mine = viewer == Some(seat.user_id);
            let shown = mine || (reveal && !seat.folded && showdown_at(index) <= now);
            let (hand_name, hand_cards) =
                if table.kind == GameKind::Holdem && !seat.cards.is_empty() && reveal_done && shown
                {
                    let mut all = seat.cards.clone();
                    if let Some(round) = round {
                        all.extend(round.board.iter().cloned());
                    }
                    best_hand(&all)
                        .map(|best| (Some(best.name), best.cards))
                        .unwrap_or((None, Vec::new()))
                } else {
                    (None, Vec::new())
                };
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
                side_pairs: seat.side_pairs,
                side_plus3: seat.side_plus3,
                insurance: seat.insurance,
                insurance_decided: seat.insurance_decided,
                side_notes: if reveal_done {
                    seat.side_notes.clone()
                } else {
                    Vec::new()
                },
                hand_name,
                hand_cards,
                cards: seat
                    .cards
                    .iter()
                    .map(|card| {
                        if shown {
                            card.clone()
                        } else {
                            "??".to_string()
                        }
                    })
                    .collect(),
                cards_reveal_at: seat.cards_reveal_at.clone(),
                hands: seat
                    .hands
                    .iter()
                    .map(|hand| {
                        // 아직 화면에 놓이지 않은 카드는 합계에 넣지 않는다.
                        let visible = hand
                            .cards
                            .iter()
                            .enumerate()
                            .filter(|(index, _)| {
                                hand.reveal_at.get(*index).is_none_or(|at| *at <= now)
                            })
                            .map(|(_, card)| card.clone())
                            .collect::<Vec<_>>();
                        let (total, soft) = blackjack_value(&visible);
                        HandView {
                            id: hand.id.clone(),
                            cards: hand.cards.clone(),
                            reveal_at: hand.reveal_at.clone(),
                            bet: hand.bet,
                            total,
                            soft,
                            status: hand.status,
                            result: if reveal_done {
                                hand.result.clone()
                            } else {
                                None
                            },
                            payout: if reveal_done { hand.payout } else { None },
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
                if index == 0 || (round.reveal && now >= round.dealer_flip_at) {
                    card.clone()
                } else {
                    "??".to_string()
                }
            })
            .collect(),
        dealer_total: (round.reveal && now >= round.reveal_until)
            .then(|| blackjack_value(&round.dealer).0),
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
        reveal_until: round.reveal_until,
        board_reveal_at: round.board_reveal_at.clone(),
        dealer_reveal_at: round.dealer_reveal_at.clone(),
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
        poker: (table.kind == GameKind::Holdem)
            .then(|| table.poker_legal_for(my_seat))
            .flatten(),
        blackjack: (table.kind == GameKind::Blackjack)
            .then(|| table.blackjack_legal_for(my_seat))
            .flatten(),
        can_bet: table.kind == GameKind::Blackjack
            && round.is_some_and(|round| round.phase == Phase::Betting)
            && my.is_some_and(|seat| !seat.in_hand && !seat.sit_out && !seat.leaving),
        can_start: !active
            && my.is_some_and(|seat| !seat.sit_out && !seat.leaving && seat.stack >= minimum)
            && ready_count >= if table.kind == GameKind::Holdem { 2 } else { 1 },
        can_insure: table.kind == GameKind::Blackjack
            && round.is_some_and(|round| round.phase == Phase::Insurance)
            && my.is_some_and(|seat| {
                seat.in_hand
                    && !seat.insurance_decided
                    && seat.stack >= seat.hands.first().map_or(0, |hand| hand.bet) / 2
            }),
        insurance_cost: my.map_or(0, |seat| seat.hands.first().map_or(0, |hand| hand.bet) / 2),
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
        shoe: (table.kind == GameKind::Blackjack).then(|| {
            let remaining = round
                .filter(|round| round.uses_shoe && active)
                .map_or(table.shoe.len(), |round| round.deck.len());
            ShoeView {
                remaining,
                total: table.shoe_total,
                cut_at: table.shoe_cut,
                shuffled_at: table.shuffled_at,
                reshuffle_due: table.shoe_total > 0 && remaining <= table.shoe_cut,
            }
        }),
        dealer: {
            let profile = table.dealer_profile();
            DealerView {
                id: profile.id.to_string(),
                name: profile.name.to_string(),
                tagline: profile.tagline.to_string(),
            }
        },
    }
}
