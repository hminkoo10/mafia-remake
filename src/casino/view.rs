// casino/view.rs — 플레이어별로 공개 가능한 상태만 담은 투영 (비공개 카드는 "??")

use super::blackjack::BjLegal;
use super::cards::{best_hand, blackjack_value};
use super::holdem::PokerLegal;
use super::table::{
    BET_WINDOW_MS, BJ_BET_STEP, BUY_IN_STEP, CasinoTable, ChatMessage, GameKind, HandResult,
    HandStatus, INSURANCE_MS, Phase, Round, SEAT_COUNT, SIDE_BET_MIN, TableSettings, live_timing,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HandView {
    pub id: String,
    pub cards: Vec<String>,
    /// 카드별 착지 시각. 클라이언트는 `reveal_at - card_flight_ms`에 슈에서 카드를 날린다.
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
    /// 홀덤: 핸드가 끝난 뒤 본인이 골라 카드를 보여 줬다.
    pub shown: bool,
    pub cards: Vec<String>,
    /// 홀 카드별 착지 시각.
    pub cards_reveal_at: Vec<i64>,
    /// 홀덤 쇼다운에서 이 좌석의 카드를 뒤집는 시각 (0이면 없음). 핸드가 정산된 뒤에만
    /// 값과 함께 보내고, 클라이언트는 이 시각까지 "??"로 두었다가 뒤집는다.
    pub showdown_at: i64,
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
    /// 보드 카드별 착지 시각 (플롭은 뒷면으로 놓인다).
    pub board_reveal_at: Vec<i64>,
    /// 플롭 세 장을 함께 뒤집는 시각 (플롭 전·블랙잭은 0).
    pub board_flip_at: i64,
    /// 이번 핸드의 번 카드 착지 시각 (카드 값은 보내지 않는다).
    pub burn_at: Vec<i64>,
    /// 딜러 카드별 착지 시각 (블랙잭).
    pub dealer_reveal_at: Vec<i64>,
    /// 딜러 홀 카드를 뒤집는 시각. 라운드가 정산된 뒤에만 실제 카드와 함께 보낸다 (0이면 없음).
    pub dealer_flip_at: i64,
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
    /// 홀덤: 끝난 핸드의 내 카드를 모두에게 보여 줄 수 있다 (쇼다운에서 이미 공개된 카드는 제외).
    pub can_show: bool,
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
    /// 카드가 슈에서 착지까지 나는 시간 (실제 값). `reveal_at - card_flight_ms`에 날리기 시작한다.
    pub card_flight_ms: i64,
    /// 카드 뒤집기 애니메이션 길이 (실제 값).
    pub card_flip_ms: i64,
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
    pub pending_rules: Option<TableRules>,
    /// 현재 딜러 (이름·초상 id).
    pub dealer: DealerView,
    pub shoe: Option<ShoeView>,
}

pub fn table_rules(settings: &TableSettings) -> TableRules {
    TableRules {
        small_blind: settings.small_blind,
        big_blind: settings.big_blind,
        min_buy_in: settings.min_buy_in,
        max_buy_in: settings.max_buy_in,
        buy_in_step: BUY_IN_STEP,
        min_bet: settings.min_bet,
        max_bet: settings.max_bet,
        bet_step: BJ_BET_STEP,
        side_bet_min: SIDE_BET_MIN,
        side_bet_max: settings.side_bet_max,
        insurance_ms: INSURANCE_MS,
        seat_count: SEAT_COUNT,
        turn_ms: settings.turn_ms,
        bet_window_ms: BET_WINDOW_MS,
        card_flight_ms: live_timing::CARD_FLIGHT_MS,
        card_flip_ms: live_timing::CARD_FLIP_MS,
    }
}

/// 라운드가 정산되어(쇼다운·딜러 공개가 확정되어) 더 정할 것이 없는지. 이때부터는 가려 둔
/// 카드를 뒤집을 시각과 함께 미리 보내 클라이언트가 제 시계로 뒤집게 한다.
fn settled_reveal(round: &Round) -> bool {
    round.reveal && round.phase == Phase::Complete
}

/// `now`까지 펠트에 놓여 앞면이 보이는 보드 카드 (플롭은 함께 뒤집힌 뒤부터).
fn landed_board(round: &Round, now: i64) -> Vec<String> {
    round
        .board
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            let landed = round
                .board_reveal_at
                .get(*index)
                .is_none_or(|at| *at <= now);
            let flipped = *index >= 3 || round.board_flip_at <= now;
            landed && flipped
        })
        .map(|(_, card)| card.clone())
        .collect()
}

/// 화면에 보여 줄 라운드 단계. 엔진은 결과가 정해지는 순간 라운드를 끝내지만(딜러 내추럴,
/// 마지막 참가자의 버스트, 올인 런아웃, 폴드 승리), 그 결과를 만든 카드가 아직 놓이는 중이면
/// 정산 전 단계로 보여 준다: 블랙잭은 딜러가 홀 카드를 뒤집기 전까지 "플레이 중", 홀덤은
/// 연출이 끝나기 전까지 지금 앞면으로 놓인 보드에 맞는 스트리트 (마지막 베팅 스트리트, 런아웃은
/// 카드가 놓이는 대로 넘어간다). 웹 화면·테이블 목록·Discord 상태가 모두 이 값을 쓴다.
pub fn shown_phase(kind: GameKind, round: &Round, now: i64) -> Phase {
    if round.phase != Phase::Complete || now >= round.reveal_until {
        return round.phase;
    }
    match kind {
        GameKind::Blackjack if round.reveal && now < round.dealer_flip_at => Phase::Playing,
        GameKind::Blackjack => round.phase,
        GameKind::Holdem => {
            let landed = match landed_board(round, now).len() {
                0..=2 => Phase::Preflop,
                3 => Phase::Flop,
                4 => Phase::Turn,
                _ => Phase::River,
            };
            match round.settled_from {
                Some(settled) if phase_rank(settled) > phase_rank(landed) => settled,
                _ => landed,
            }
        }
    }
}

fn phase_rank(phase: Phase) -> u8 {
    match phase {
        Phase::Preflop => 0,
        Phase::Flop => 1,
        Phase::Turn => 2,
        Phase::River => 3,
        _ => 0,
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
    let reveal_done = round.is_none_or(|round| now >= round.reveal_until);
    let settled = round.is_some_and(settled_reveal);
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
            // 다른 사람의 홀 카드는 쇼다운이 정산된 뒤에만, 폴드하지 않은 경쟁자의 것만
            // 뒤집을 시각과 함께 보낸다. 그 전에는 어떤 시각에도 "??"다.
            let contender = settled
                && table.kind == GameKind::Holdem
                && seat.in_hand
                && !seat.folded
                && !seat.cards.is_empty();
            // 핸드가 끝난 뒤 본인이 골라 보여 준 카드 (연출이 끝난 뒤에만 고를 수 있다).
            let volunteered =
                table.kind == GameKind::Holdem && seat.shown && !seat.cards.is_empty();
            let shown = mine || contender || volunteered;
            let hole_landed = seat.cards_reveal_at.iter().all(|at| *at <= now);
            // 족보 이름: 내 좌석은 내 카드와 이미 펠트에 놓여 뒤집힌 보드로 (스트리트 연출
            // 중에도 앞 스트리트의 족보를 유지한다), 다른 경쟁자는 쇼다운 연출이 끝난 뒤에.
            let hand_source = match round {
                Some(round) if table.kind == GameKind::Holdem && !seat.cards.is_empty() => {
                    if mine && hole_landed {
                        Some(landed_board(round, now))
                    } else if (contender || volunteered) && reveal_done {
                        Some(round.board.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let (hand_name, hand_cards) = hand_source
                .and_then(|board| {
                    let mut all = seat.cards.clone();
                    all.extend(board);
                    best_hand(&all)
                })
                .map_or((None, Vec::new()), |best| (Some(best.name), best.cards));
            Some(SeatView {
                seat: index,
                user_id: seat.user_id,
                name: seat.name.clone(),
                // 이번 라운드 당첨금은 카드가 다 놓인 뒤에 스택에 보인다.
                stack: seat.visible_stack(now),
                mine,
                bet: seat.bet,
                total: seat.total,
                folded: seat.folded,
                in_hand: seat.in_hand,
                sit_out: seat.sit_out,
                leaving: seat.leaving,
                shown: volunteered,
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
                showdown_at: if contender { showdown_at(index) } else { 0 },
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
    let round_view = round.map(|round| {
        let phase = shown_phase(table.kind, round, now);
        RoundView {
            id: round.id.clone(),
            phase,
            phase_text: phase.value().to_string(),
            board: round.board.clone(),
            // 딜러 홀 카드는 라운드가 정산되기 전에는 어떤 시각에도 "??"다. 정산된 뒤에는 실제
            // 카드와 뒤집을 시각(`dealer_flip_at`)을 함께 보내 클라이언트가 제 시계로 뒤집는다.
            dealer: round
                .dealer
                .iter()
                .enumerate()
                .map(|(index, card)| {
                    if index == 0 || settled_reveal(round) {
                        card.clone()
                    } else {
                        "??".to_string()
                    }
                })
                .collect(),
            dealer_total: (round.reveal && now >= round.reveal_until)
                .then(|| blackjack_value(&round.dealer).0),
            turn: if table.kind == GameKind::Blackjack && !reveal_done {
                -1
            } else {
                round.turn
            },
            hand: if table.kind == GameKind::Blackjack && !reveal_done {
                0
            } else {
                round.hand
            },
            deadline: if table.kind == GameKind::Blackjack && !reveal_done {
                0
            } else {
                round.deadline
            },
            current_bet: round.current_bet,
            pot: if table.kind == GameKind::Holdem && active {
                table.seats.iter().flatten().map(|seat| seat.total).sum()
            } else {
                round.pot
            },
            reveal: round.reveal,
            reveal_until: round.reveal_until,
            board_reveal_at: round.board_reveal_at.clone(),
            board_flip_at: round.board_flip_at,
            burn_at: round.burn_at.clone(),
            dealer_reveal_at: round.dealer_reveal_at.clone(),
            dealer_flip_at: if table.kind == GameKind::Blackjack && settled_reveal(round) {
                round.dealer_flip_at
            } else {
                0
            },
        }
    });
    let minimum = if table.kind == GameKind::Holdem {
        1
    } else {
        table.settings.min_bet
    };
    let my = table.seat(my_seat);
    let insurance_cost = my.map_or(0, |seat| seat.hands.first().map_or(0, |hand| hand.bet) / 2);
    // 스택 검사는 화면에 보이는 스택으로 한다: 아직 드러나면 안 되는 당첨금으로 "시작"·
    // "인슈어런스"가 먼저 켜지면 결과가 샌다.
    let ready_count = table
        .seats
        .iter()
        .flatten()
        .filter(|seat| !seat.sit_out && !seat.leaving && seat.visible_stack(now) >= minimum)
        .count();
    let legal = LegalView {
        poker: (table.kind == GameKind::Holdem)
            .then(|| table.poker_legal_for(my_seat))
            .flatten(),
        blackjack: (table.kind == GameKind::Blackjack && reveal_done)
            .then(|| table.blackjack_legal_for(my_seat))
            .flatten(),
        can_bet: table.kind == GameKind::Blackjack
            && round.is_some_and(|round| round.phase == Phase::Betting)
            && my.is_some_and(|seat| !seat.in_hand && !seat.sit_out && !seat.leaving),
        // 지난 라운드의 카드를 다 연 뒤에만 시작할 수 있다 (엔진도 그 전에는 거절한다).
        can_start: !active
            && reveal_done
            && my.is_some_and(|seat| {
                !seat.sit_out && !seat.leaving && seat.visible_stack(now) >= minimum
            })
            && ready_count >= if table.kind == GameKind::Holdem { 2 } else { 1 },
        can_insure: table.kind == GameKind::Blackjack
            && round.is_some_and(|round| round.phase == Phase::Insurance)
            && my.is_some_and(|seat| {
                seat.in_hand && !seat.insurance_decided && seat.visible_stack(now) >= insurance_cost
            }),
        insurance_cost,
        can_show: table.kind == GameKind::Holdem
            && !active
            && reveal_done
            && my.is_some_and(|seat| {
                !seat.cards.is_empty()
                    && !seat.shown
                    && !(round.is_some_and(|round| round.reveal) && seat.in_hand && !seat.folded)
            }),
    };
    TableView {
        id: table.id.clone(),
        kind: table.kind,
        name: table.name.clone(),
        version: table.version,
        button: table.button,
        my_seat,
        // 결과 안내·채팅·기록은 그 결과를 만든 카드가 다 놓인 뒤에 보인다.
        narration: table.narration_at(now).to_string(),
        seats,
        round: round_view,
        legal,
        messages: table
            .messages
            .iter()
            .filter(|message| message.at <= now)
            .cloned()
            .collect(),
        history: table.visible_history(now).to_vec(),
        rules: table_rules(&table.settings),
        pending_rules: table.pending_settings.as_ref().map(table_rules),
        shoe: (table.kind == GameKind::Blackjack).then(|| {
            let mut remaining = round
                .filter(|round| round.uses_shoe && active)
                .map_or(table.shoe.len(), |round| round.deck.len());
            // 카드를 나누는 중에는 아직 착지하지 않은 카드를 슈에 남은 것으로 센다. 그러지 않으면
            // 카운터가 딜러가 몇 장을 뽑을지(곧 결과를) 먼저 알려 준다.
            let revealing = round.filter(|round| round.uses_shoe && now < round.reveal_until);
            if let Some(round) = revealing {
                let pending = |times: &[i64]| times.iter().filter(|at| **at > now).count();
                remaining += pending(&round.dealer_reveal_at)
                    + pending(&round.board_reveal_at)
                    + pending(&round.burn_at)
                    + table
                        .seats
                        .iter()
                        .flatten()
                        .map(|seat| {
                            pending(&seat.cards_reveal_at)
                                + seat
                                    .hands
                                    .iter()
                                    .map(|hand| pending(&hand.reveal_at))
                                    .sum::<usize>()
                        })
                        .sum::<usize>();
            }
            ShoeView {
                remaining: remaining.min(table.shoe_total),
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
