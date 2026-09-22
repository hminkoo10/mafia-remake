// casino/table.rs — 테이블·좌석·라운드 상태, 명령 적용, 시간 초과 처리

use super::blackjack::{
    BjAction, blackjack_action, blackjack_legal, deal_blackjack, insurance_decision, place_bet,
    resolve_insurance, start_blackjack,
};
use super::cards::{CasinoError, blackjack_value};
use super::holdem::{PokerAction, poker_action, poker_legal, start_poker};
use serde::{Deserialize, Serialize};

pub const SEAT_COUNT: usize = 6;
/// 액션 제한시간.
pub const TURN_MS: i64 = 30_000;
/// 연출용 시간차 (ms). 카드는 이 간격으로 한 장씩 열리고, 그동안 액션은 막힌다.
/// 테스트에서는 0으로 두어 게임 흐름을 즉시 검증한다.
pub const DEAL_CARD_MS: i64 = if cfg!(test) { 0 } else { 380 };
pub const STREET_PAUSE_MS: i64 = if cfg!(test) { 0 } else { 900 };
pub const SHOWDOWN_STEP_MS: i64 = if cfg!(test) { 0 } else { 800 };
pub const DEALER_DRAW_MS: i64 = if cfg!(test) { 0 } else { 900 };
pub const SETTLE_PAUSE_MS: i64 = if cfg!(test) { 0 } else { 700 };
/// 블랙잭 베팅창.
pub const BET_WINDOW_MS: i64 = 15_000;
/// 라운드가 없는 상태에서 이 시간 동안 아무 행동이 없으면 자동 퇴장(칩 반환).
pub const IDLE_CASHOUT_MS: i64 = 30 * 60_000;
pub const HOLDEM_SMALL_BLIND: i64 = 50;
pub const HOLDEM_BIG_BLIND: i64 = 100;
pub const MIN_BUY_IN: i64 = 5_000;
pub const MAX_BUY_IN: i64 = 20_000;
pub const BUY_IN_STEP: i64 = 100;
pub const BJ_MIN_BET: i64 = 100;
pub const BJ_MAX_BET: i64 = 5_000;
pub const BJ_BET_STEP: i64 = 100;
/// 사이드베팅(퍼펙트 페어·21+3) 한도.
pub const SIDE_BET_MIN: i64 = 100;
pub const SIDE_BET_MAX: i64 = 2_500;
/// 인슈어런스 결정 시간.
pub const INSURANCE_MS: i64 = 10_000;
const MESSAGE_LIMIT: usize = 40;

/// 딜러 프로필. 초상은 casino-web/public/dealers/<id>.png (dealer.png와 같은 1536x1024 구도),
/// 있으면 dealers/<id>.webm(무음 루프)도 함께 쓴다. `has_portrait`가 true인 딜러만 교대에 들어간다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DealerProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub tagline: &'static str,
    pub has_portrait: bool,
}

pub const DEALERS: &[DealerProfile] = &[
    DealerProfile {
        id: "sophia",
        name: "소피아",
        tagline: "YOUR DEALER",
        has_portrait: true,
    },
    DealerProfile {
        id: "mia",
        name: "미아",
        tagline: "EVENING SHIFT",
        has_portrait: false,
    },
    DealerProfile {
        id: "hana",
        name: "하나",
        tagline: "NIGHT SHIFT",
        has_portrait: false,
    },
    DealerProfile {
        id: "lina",
        name: "리나",
        tagline: "LATE SHIFT",
        has_portrait: false,
    },
];

/// 이 판 수마다 딜러가 교대한다.
pub const DEALER_SHIFT_HANDS: u32 = 12;

pub fn dealer_profile(id: &str) -> DealerProfile {
    DEALERS
        .iter()
        .copied()
        .find(|dealer| dealer.id == id)
        .unwrap_or(DEALERS[0])
}
const HISTORY_LIMIT: usize = 20;
const CHAT_COOLDOWN_MS: i64 = 1_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameKind {
    Holdem,
    Blackjack,
}

impl GameKind {
    pub fn value(self) -> &'static str {
        match self {
            Self::Holdem => "홀덤",
            Self::Blackjack => "블랙잭",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Holdem => "holdem",
            Self::Blackjack => "blackjack",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "holdem" | "홀덤" | "포커" | "texas" => Some(Self::Holdem),
            "blackjack" | "블랙잭" | "bj" => Some(Self::Blackjack),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Preflop,
    Flop,
    Turn,
    River,
    Betting,
    /// 딜러가 에이스를 보일 때 인슈어런스를 받는 시간.
    Insurance,
    Playing,
    Complete,
}

impl Phase {
    pub fn value(self) -> &'static str {
        match self {
            Self::Preflop => "프리플롭",
            Self::Flop => "플롭",
            Self::Turn => "턴",
            Self::River => "리버",
            Self::Betting => "베팅 접수",
            Self::Insurance => "인슈어런스",
            Self::Playing => "플레이 중",
            Self::Complete => "라운드 종료",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HandStatus {
    Playing,
    Stand,
    Bust,
    Surrender,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BjHand {
    pub id: String,
    pub cards: Vec<String>,
    pub bet: i64,
    pub status: HandStatus,
    pub split: bool,
    pub split_aces: bool,
    #[serde(default)]
    pub payout: Option<i64>,
    #[serde(default)]
    pub result: Option<String>,
    /// 카드별로 화면에 나타나는 시각 (ms). 비어 있거나 짧으면 나머지는 즉시 보인다.
    #[serde(default)]
    pub reveal_at: Vec<i64>,
}

impl BjHand {
    pub fn new(cards: Vec<String>, bet: i64, split: bool, split_aces: bool) -> Self {
        Self {
            id: new_id(),
            cards,
            bet,
            status: HandStatus::Playing,
            split,
            split_aces,
            payout: None,
            result: None,
            reveal_at: Vec::new(),
        }
    }

    /// 스플릿하지 않은 첫 두 장 21.
    pub fn natural(&self) -> bool {
        !self.split && self.cards.len() == 2 && blackjack_value(&self.cards).0 == 21
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seat {
    pub user_id: u64,
    pub name: String,
    pub stack: i64,
    pub cards: Vec<String>,
    pub bet: i64,
    pub total: i64,
    pub folded: bool,
    pub in_hand: bool,
    pub acted_at: Option<i64>,
    pub checked: bool,
    pub hands: Vec<BjHand>,
    pub leaving: bool,
    pub sit_out: bool,
    pub missed: u32,
    pub last_seen: i64,
    /// 홀덤 홀 카드가 화면에 나타나는 시각 (카드와 같은 순서).
    #[serde(default)]
    pub cards_reveal_at: Vec<i64>,
    /// 블랙잭 사이드베팅: 퍼펙트 페어 / 21+3 에 건 칩.
    #[serde(default)]
    pub side_pairs: i64,
    #[serde(default)]
    pub side_plus3: i64,
    /// 인슈어런스에 건 칩 (베팅의 절반).
    #[serde(default)]
    pub insurance: i64,
    #[serde(default)]
    pub insurance_decided: bool,
    /// 사이드베팅·인슈어런스 순손익 합계 (이번 라운드).
    #[serde(default)]
    pub side_net: i64,
    /// 사이드베팅·인슈어런스 결과 설명 (이번 라운드).
    #[serde(default)]
    pub side_notes: Vec<String>,
}

impl Seat {
    fn new(user_id: u64, name: String, stack: i64, now: i64) -> Self {
        Self {
            user_id,
            name,
            stack,
            cards: Vec::new(),
            cards_reveal_at: Vec::new(),
            side_pairs: 0,
            side_plus3: 0,
            insurance: 0,
            insurance_decided: false,
            side_net: 0,
            side_notes: Vec::new(),
            bet: 0,
            total: 0,
            folded: false,
            in_hand: false,
            acted_at: None,
            checked: false,
            hands: Vec::new(),
            leaving: false,
            sit_out: false,
            missed: 0,
            last_seen: now,
        }
    }

    /// 시간 초과를 기록한다. 2회 연속이면 자리 비움.
    pub(super) fn note_timeout(&mut self, timeout: bool) {
        if timeout {
            self.missed += 1;
            if self.missed >= 2 {
                self.sit_out = true;
            }
        } else {
            self.missed = 0;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Round {
    pub id: String,
    pub deck: Vec<String>,
    pub board: Vec<String>,
    pub dealer: Vec<String>,
    pub phase: Phase,
    /// 현재 차례 좌석 (-1이면 없음).
    pub turn: i32,
    /// 블랙잭에서 현재 플레이 중인 핸드 번호.
    pub hand: usize,
    pub current_bet: i64,
    pub min_raise: i64,
    pub deadline: i64,
    pub pot: i64,
    pub reveal: bool,
    /// 연출이 끝나는 시각. 그 전에는 액션을 받지 않는다.
    #[serde(default)]
    pub reveal_until: i64,
    /// 보드 카드가 열리는 시각 (보드와 같은 순서).
    #[serde(default)]
    pub board_reveal_at: Vec<i64>,
    /// 딜러 카드가 놓이는 시각 (블랙잭).
    #[serde(default)]
    pub dealer_reveal_at: Vec<i64>,
    /// 딜러의 뒤집힌 카드가 공개되는 시각 (블랙잭 정산).
    #[serde(default)]
    pub dealer_flip_at: i64,
    /// 쇼다운에서 좌석별 홀 카드가 공개되는 시각 (좌석 번호 순, 0이면 없음).
    #[serde(default)]
    pub showdown_reveal_at: Vec<i64>,
}

impl Round {
    /// 새 연출을 예약할 기준 시각: 진행 중인 연출이 있으면 그 끝, 아니면 지금.
    pub fn reveal_base(&self, now: i64) -> i64 {
        now.max(self.reveal_until)
    }

    pub(super) fn schedule_defaults(now: i64) -> (i64, Vec<i64>, Vec<i64>, i64, Vec<i64>) {
        (now, Vec::new(), Vec::new(), 0, vec![0; SEAT_COUNT])
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    /// 테이블 안에서 단조 증가하는 번호. 중계가 어디까지 보냈는지 추적한다.
    pub seq: u64,
    pub name: String,
    pub text: String,
    pub at: i64,
    pub dealer: bool,
    /// 보낸 사람의 Discord ID (딜러 안내는 None).
    #[serde(default)]
    pub user_id: Option<u64>,
    /// Discord 채널에서 들어온 메시지 (채널로 다시 중계하지 않는다).
    #[serde(default)]
    pub from_discord: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Payout {
    pub name: String,
    pub amount: i64,
    pub label: String,
}

/// 라운드에서 좌석별 손익 (건 칩 대비 순증감).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeatResult {
    pub user_id: u64,
    pub name: String,
    pub seat: usize,
    /// 이번 라운드에 건 칩.
    pub wagered: i64,
    /// 순손익 (+면 딴 것, -면 잃은 것).
    pub net: i64,
    /// 족보·결과 이름 ("원 페어", "폴드", "승리" 등).
    pub label: String,
    /// 사이드베팅·인슈어런스 결과 ("퍼펙트 페어 12:1 +1,200" 등).
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandResult {
    pub id: String,
    pub game: GameKind,
    pub at: i64,
    pub summary: String,
    pub board: Vec<String>,
    pub payouts: Vec<Payout>,
    /// 참가한 모든 좌석의 손익 (승자뿐 아니라 잃은 사람도 포함).
    #[serde(default)]
    pub results: Vec<SeatResult>,
}

/// 명령. JSON은 `{"action":"raise","amount":300}` 꼴이다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum CasinoCommand {
    Join {
        seat: usize,
        amount: i64,
        name: String,
    },
    Leave,
    Start,
    Resume,
    Chat {
        message: String,
    },
    Fold,
    Check,
    Call,
    Raise {
        amount: i64,
    },
    Bet {
        amount: i64,
        /// 퍼펙트 페어 사이드베팅 (0이면 없음).
        #[serde(default)]
        pairs: i64,
        /// 21+3 사이드베팅 (0이면 없음).
        #[serde(default)]
        plus3: i64,
    },
    /// 인슈어런스 받기/거절 (딜러 에이스일 때만).
    Insure {
        accept: bool,
    },
    Hit,
    Stand,
    Double,
    Split,
    Surrender,
}

impl CasinoCommand {
    pub fn is_chat(&self) -> bool {
        matches!(self, Self::Chat { .. })
    }
}

/// 명령·시간 초과 처리로 생긴, 엔진 밖에서 처리해야 할 일.
#[derive(Debug, Clone, PartialEq)]
pub enum CasinoEvent {
    /// 좌석에 앉았다 (바이인 칩은 호출자가 이미 코인에서 뺐다).
    Joined {
        user_id: u64,
        name: String,
        seat: usize,
        amount: i64,
    },
    /// 좌석을 떠났고 남은 칩을 코인으로 돌려줘야 한다.
    CashOut {
        user_id: u64,
        name: String,
        amount: i64,
    },
    /// 라운드가 끝났다. 블랙잭은 하우스 손익(플레이어 기준 반대 부호)도 같이 준다.
    RoundSettled {
        result: HandResult,
        house_delta: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CasinoTable {
    pub id: String,
    pub kind: GameKind,
    pub name: String,
    pub version: u64,
    pub button: i32,
    pub big_blind_seat: i32,
    pub seats: Vec<Option<Seat>>,
    pub round: Option<Round>,
    pub messages: Vec<ChatMessage>,
    pub history: Vec<HandResult>,
    pub narration: String,
    #[serde(default)]
    pub next_message_seq: u64,
    #[serde(default)]
    pub created_by: u64,
    #[serde(default)]
    pub created_at: i64,
    /// 사용자별 마지막 채팅 시각 (도배 방지).
    #[serde(default)]
    pub last_chat_at: std::collections::HashMap<u64, i64>,
    /// 현재 딜러 id (DEALERS 참고).
    #[serde(default = "default_dealer_id")]
    pub dealer: String,
    /// 현재 딜러가 진행한 판 수 (교대 계산용).
    #[serde(default)]
    pub dealer_hands: u32,
}

fn default_dealer_id() -> String {
    DEALERS[0].id.to_string()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// "12,345" 꼴의 칩 표기 (통화 단위 없이).
pub fn format_chips(amount: i64) -> String {
    let digits = amount.unsigned_abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    if amount < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// 부호를 항상 붙인 칩 표기 (+1,200 / -300 / +0).
pub fn signed_chips(amount: i64) -> String {
    if amount < 0 {
        format_chips(amount)
    } else {
        format!("+{}", format_chips(amount))
    }
}

impl CasinoTable {
    pub fn new(
        id: impl Into<String>,
        kind: GameKind,
        name: impl Into<String>,
        created_by: u64,
        now: i64,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            name: name.into(),
            version: 0,
            button: -1,
            big_blind_seat: -1,
            seats: vec![None; SEAT_COUNT],
            round: None,
            messages: Vec::new(),
            history: Vec::new(),
            narration: "반가워요. 자리가 준비되어 있어요.".to_string(),
            next_message_seq: 0,
            created_by,
            created_at: now,
            last_chat_at: std::collections::HashMap::new(),
            dealer: default_dealer_id(),
            dealer_hands: 0,
        }
    }

    /// 현재 딜러.
    pub fn dealer_profile(&self) -> DealerProfile {
        dealer_profile(&self.dealer)
    }

    /// 판이 시작될 때 부른다. 정해진 판 수를 채우면 초상이 있는 다음 딜러로 교대한다.
    fn rotate_dealer_if_due(&mut self, now: i64) {
        self.dealer_hands += 1;
        if self.dealer_hands < DEALER_SHIFT_HANDS {
            return;
        }
        let available = DEALERS
            .iter()
            .filter(|dealer| dealer.has_portrait)
            .collect::<Vec<_>>();
        if available.len() < 2 {
            self.dealer_hands = 0;
            return;
        }
        let position = available
            .iter()
            .position(|dealer| dealer.id == self.dealer)
            .unwrap_or(0);
        let next = available[(position + 1) % available.len()];
        let leaving = self.dealer_profile().name;
        self.say(
            format!(
                "{leaving}가 잠시 쉬러 갑니다. 이제 {}가 테이블을 맡을게요.",
                next.name
            ),
            now,
        );
        self.dealer = next.id.to_string();
        self.dealer_hands = 0;
    }

    /// 라운드가 진행 중인가 (완료 전).
    pub fn playing(&self) -> bool {
        self.round
            .as_ref()
            .is_some_and(|round| round.phase != Phase::Complete)
    }

    pub fn seat_index(&self, user_id: u64) -> Option<usize> {
        self.seats
            .iter()
            .position(|seat| seat.as_ref().is_some_and(|seat| seat.user_id == user_id))
    }

    pub fn seat(&self, index: i32) -> Option<&Seat> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.seats.get(index))
            .and_then(Option::as_ref)
    }

    pub fn seat_mut(&mut self, index: i32) -> Option<&mut Seat> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.seats.get_mut(index))
            .and_then(Option::as_mut)
    }

    pub fn occupied_seats(&self) -> impl Iterator<Item = (usize, &Seat)> {
        self.seats
            .iter()
            .enumerate()
            .filter_map(|(index, seat)| seat.as_ref().map(|seat| (index, seat)))
    }

    /// `from` 다음 좌석부터 시계 방향으로 조건에 맞는 첫 좌석 (-1이면 없음).
    pub(super) fn next_seat(&self, from: i32, eligible: impl Fn(&Seat) -> bool) -> i32 {
        for n in 1..=SEAT_COUNT as i32 {
            let index = (from + n).rem_euclid(SEAT_COUNT as i32);
            if let Some(seat) = self.seat(index) {
                if eligible(seat) {
                    return index;
                }
            }
        }
        -1
    }

    /// 딜러 안내. 안내문을 갱신하고 채팅에도 남긴다 (이름은 현재 딜러).
    pub(super) fn say(&mut self, text: impl Into<String>, now: i64) {
        let text = text.into();
        self.narration = text.clone();
        let dealer = self.dealer_profile().name.to_string();
        self.push_message(dealer, text, now, true, None);
    }

    fn push_message(
        &mut self,
        name: String,
        text: String,
        at: i64,
        dealer: bool,
        user_id: Option<u64>,
    ) {
        self.next_message_seq += 1;
        self.messages.push(ChatMessage {
            id: new_id(),
            seq: self.next_message_seq,
            name,
            text,
            at,
            dealer,
            user_id,
            from_discord: false,
        });
        let overflow = self.messages.len().saturating_sub(MESSAGE_LIMIT);
        if overflow > 0 {
            self.messages.drain(..overflow);
        }
    }

    /// 봇 쪽(Discord 채널)에서 온 채팅을 테이블 채팅에 넣는다. 좌석 여부와 무관.
    pub fn relay_chat(&mut self, user_id: u64, name: &str, text: &str, now: i64) {
        let text = text.trim().chars().take(240).collect::<String>();
        if text.is_empty() {
            return;
        }
        self.push_message(name.to_string(), text, now, false, Some(user_id));
        if let Some(last) = self.messages.last_mut() {
            last.from_discord = true;
        }
    }

    pub(super) fn remember(&mut self, result: HandResult) {
        self.history.insert(0, result);
        self.history.truncate(HISTORY_LIMIT);
    }

    /// 좌석의 칩을 돌려주고 좌석을 비운다.
    fn cash_out(&mut self, index: usize, events: &mut Vec<CasinoEvent>) {
        if let Some(seat) = self.seats.get_mut(index).and_then(Option::take) {
            events.push(CasinoEvent::CashOut {
                user_id: seat.user_id,
                name: seat.name,
                amount: seat.stack,
            });
        }
    }

    /// 라운드가 막 끝났으면 결과 이벤트를 만들고, 퇴장 대기 좌석을 정리한다.
    fn reconcile(&mut self, was_playing: bool, events: &mut Vec<CasinoEvent>) {
        let just_completed = was_playing && !self.playing() && self.round.is_some();
        if !just_completed {
            return;
        }
        // 베팅 없이 끝난 블랙잭 라운드처럼 기록이 없으면 이벤트도 없다 (예전 기록을 다시 보내지 않게).
        let round_id = self.round.as_ref().map(|round| round.id.clone());
        let settled = self
            .history
            .first()
            .filter(|result| Some(&result.id) == round_id.as_ref())
            .cloned();
        if let Some(result) = settled {
            // 하우스 손익 = 플레이어 순손익의 반대 (사이드베팅·인슈어런스 포함).
            let house_delta = if self.kind == GameKind::Blackjack {
                -result.results.iter().map(|entry| entry.net).sum::<i64>()
            } else {
                0
            };
            events.push(CasinoEvent::RoundSettled {
                result,
                house_delta,
            });
        }
        for index in 0..SEAT_COUNT {
            if self.seats[index].as_ref().is_some_and(|seat| seat.leaving) {
                self.cash_out(index, events);
            }
        }
    }

    /// 시간 초과·유휴 처리. 바뀐 게 있으면 버전이 오른다.
    pub fn tick(&mut self, now: i64) -> Result<Vec<CasinoEvent>, CasinoError> {
        let mut events = Vec::new();
        let was_playing = self.playing();
        if let Some(round) = self
            .round
            .as_ref()
            .filter(|round| round.phase != Phase::Complete)
        {
            if round.deadline <= now {
                let turn = round.turn;
                let phase = round.phase;
                if self.kind == GameKind::Blackjack && phase == Phase::Betting {
                    deal_blackjack(self, now)?;
                } else if self.kind == GameKind::Blackjack && phase == Phase::Insurance {
                    resolve_insurance(self, now)?;
                } else if let Some(seat) = self.seat(turn) {
                    let actor = seat.user_id;
                    if self.kind == GameKind::Holdem {
                        let action = if poker_legal(self, turn).is_some_and(|legal| legal.can_check)
                        {
                            PokerAction::Check
                        } else {
                            PokerAction::Fold
                        };
                        poker_action(self, actor, action, now, true)?;
                    } else {
                        blackjack_action(self, actor, BjAction::Stand, now, true)?;
                    }
                }
                self.version += 1;
                self.reconcile(was_playing, &mut events);
            }
        }
        if !self.playing() {
            for index in 0..SEAT_COUNT {
                let idle = self.seats[index]
                    .as_ref()
                    .is_some_and(|seat| now - seat.last_seen > IDLE_CASHOUT_MS);
                if idle {
                    self.cash_out(index, &mut events);
                    self.version += 1;
                }
            }
        }
        Ok(events)
    }

    /// 테이블을 닫는다: 모든 좌석의 칩을 돌려준다 (진행 중 라운드는 무효).
    pub fn close(&mut self) -> Vec<CasinoEvent> {
        let mut events = Vec::new();
        for (index, seat) in self.seats.iter_mut().enumerate() {
            if let Some(seat) = seat.as_mut() {
                // 진행 중이던 베팅(total)은 정산되지 않으므로 함께 돌려준다.
                seat.stack += seat.total;
                seat.total = 0;
            }
            let _ = index;
        }
        for index in 0..SEAT_COUNT {
            self.cash_out(index, &mut events);
        }
        self.round = None;
        self.version += 1;
        events
    }

    /// 명령을 적용한다. `expected_version`이 있고 현재 버전과 다르면(채팅 제외)
    /// STALE_STATE로 거부한다. 바이인 코인 차감·좌석 중복(다른 테이블) 검사는
    /// 호출자가 먼저 한다.
    pub fn apply_command(
        &mut self,
        actor: u64,
        actor_name: &str,
        command: &CasinoCommand,
        expected_version: Option<u64>,
        now: i64,
    ) -> Result<Vec<CasinoEvent>, CasinoError> {
        let mut events = Vec::new();
        if !command.is_chat() {
            if let Some(expected) = expected_version {
                if expected != self.version {
                    return Err(CasinoError::new(
                        "STALE_STATE",
                        "테이블 상태가 바뀌었습니다. 현재 상태를 확인한 후 다시 선택해 주세요.",
                    ));
                }
            }
        }
        let was_playing = self.playing();
        let seat_index = self.seat_index(actor);
        match command {
            CasinoCommand::Join { seat, amount, name } => {
                if seat_index.is_some() {
                    return Err(CasinoError::invalid("이미 이 테이블에 앉아 있습니다."));
                }
                if *seat >= SEAT_COUNT || self.seats[*seat].is_some() {
                    return Err(CasinoError::invalid("이미 선택된 좌석입니다."));
                }
                if *amount < MIN_BUY_IN || *amount > MAX_BUY_IN || amount % BUY_IN_STEP != 0 {
                    return Err(CasinoError::invalid(format!(
                        "바이인은 {MIN_BUY_IN}~{MAX_BUY_IN} 칩, {BUY_IN_STEP} 단위입니다."
                    )));
                }
                let name = name.trim();
                if name.chars().count() < 2
                    || name.chars().count() > 16
                    || name.chars().any(char::is_control)
                {
                    return Err(CasinoError::invalid("닉네임은 2~16자로 입력해 주세요."));
                }
                self.seats[*seat] = Some(Seat::new(actor, name.to_string(), *amount, now));
                let greeting = if self.playing() {
                    " 다음 핸드부터 참여할 수 있어요."
                } else {
                    " 테이블이 준비되어 있어요."
                };
                self.say(format!("{name}님, 어서 오세요.{greeting}"), now);
                events.push(CasinoEvent::Joined {
                    user_id: actor,
                    name: name.to_string(),
                    seat: *seat,
                    amount: *amount,
                });
            }
            CasinoCommand::Chat { message } => {
                let Some(index) = seat_index else {
                    return Err(CasinoError::invalid(
                        "채팅은 테이블 참가자만 사용할 수 있습니다.",
                    ));
                };
                let last = self
                    .last_chat_at
                    .get(&actor)
                    .copied()
                    .unwrap_or(i64::MIN / 2);
                if now - last < CHAT_COOLDOWN_MS {
                    return Err(CasinoError::invalid("잠시 후 다시 전송해 주세요."));
                }
                let text = message.trim();
                if text.is_empty() || text.chars().count() > 240 {
                    return Err(CasinoError::invalid("메시지는 1~240자로 입력해 주세요."));
                }
                let name = self.seats[index]
                    .as_ref()
                    .map(|seat| seat.name.clone())
                    .unwrap_or_else(|| actor_name.to_string());
                self.push_message(name, text.to_string(), now, false, Some(actor));
                self.last_chat_at.insert(actor, now);
                if text.contains(self.dealer_profile().name)
                    || text.contains("딜러")
                    || text.contains("규칙")
                    || text.contains("안녕")
                    || text.contains("고마")
                {
                    let reply = if text.contains("규칙") {
                        "상단의 게임 규칙에서 테이블 규칙을 확인할 수 있어요. 현재 차례와 가능한 액션은 아래에 표시됩니다."
                    } else {
                        "함께해 주셔서 반가워요. 편하게 플레이해 주세요!"
                    };
                    self.say(reply, now);
                }
                return Ok(events);
            }
            _ => {
                let Some(index) = seat_index else {
                    return Err(CasinoError::invalid("먼저 좌석에 앉아 주세요."));
                };
                if let Some(seat) = self.seats[index].as_mut() {
                    seat.last_seen = now;
                }
                match command {
                    CasinoCommand::Leave => {
                        let in_hand = self.playing()
                            && self.seats[index].as_ref().is_some_and(|seat| seat.in_hand);
                        if in_hand {
                            if let Some(seat) = self.seats[index].as_mut() {
                                seat.leaving = true;
                            }
                            let turn = self.round.as_ref().map_or(-1, |round| round.turn);
                            let phase = self.round.as_ref().map(|round| round.phase);
                            if self.kind == GameKind::Blackjack && phase == Some(Phase::Playing) {
                                if let Some(seat) = self.seats[index].as_mut() {
                                    for hand in &mut seat.hands {
                                        if hand.status == HandStatus::Playing {
                                            hand.status = HandStatus::Stand;
                                        }
                                    }
                                }
                                if turn == index as i32 {
                                    let hand_index =
                                        self.round.as_ref().map_or(0, |round| round.hand);
                                    if let Some(hand) = self.seats[index]
                                        .as_mut()
                                        .and_then(|seat| seat.hands.get_mut(hand_index))
                                    {
                                        hand.status = HandStatus::Playing;
                                    }
                                    blackjack_action(self, actor, BjAction::Stand, now, false)?;
                                }
                            }
                            if self.kind == GameKind::Holdem && turn == index as i32 {
                                poker_action(self, actor, PokerAction::Fold, now, false)?;
                            }
                            self.say("핸드가 끝나면 남은 칩과 함께 퇴장합니다.", now);
                        } else {
                            let name = self.seats[index]
                                .as_ref()
                                .map(|seat| seat.name.clone())
                                .unwrap_or_default();
                            self.cash_out(index, &mut events);
                            self.say(format!("{name}님, 다음 테이블에서 만나요."), now);
                        }
                    }
                    CasinoCommand::Resume => {
                        let seat = self.seats[index].as_mut().expect("seat exists");
                        if seat.leaving {
                            return Err(CasinoError::invalid("퇴장 대기 중입니다."));
                        }
                        seat.sit_out = false;
                        seat.missed = 0;
                    }
                    CasinoCommand::Start => {
                        if self.playing() {
                            return Err(CasinoError::invalid("이미 진행 중인 라운드입니다."));
                        }
                        self.rotate_dealer_if_due(now);
                        let minimum = if self.kind == GameKind::Holdem {
                            1
                        } else {
                            BJ_MIN_BET
                        };
                        let ready = self.seats[index].as_ref().is_some_and(|seat| {
                            !seat.sit_out && !seat.leaving && seat.stack >= minimum
                        });
                        if !ready {
                            return Err(CasinoError::invalid("참여 가능한 칩이 필요합니다."));
                        }
                        match self.kind {
                            GameKind::Holdem => start_poker(self, now, None)?,
                            GameKind::Blackjack => start_blackjack(self, now, None)?,
                        }
                    }
                    CasinoCommand::Bet {
                        amount,
                        pairs,
                        plus3,
                    } => {
                        if self.kind != GameKind::Blackjack {
                            return Err(CasinoError::invalid(
                                "블랙잭 테이블에서만 베팅할 수 있습니다.",
                            ));
                        }
                        place_bet(self, index, *amount, *pairs, *plus3, now)?;
                    }
                    CasinoCommand::Insure { accept } => {
                        if self.kind != GameKind::Blackjack {
                            return Err(CasinoError::invalid(
                                "블랙잭 테이블에서만 쓸 수 있습니다.",
                            ));
                        }
                        insurance_decision(self, index, *accept, now)?;
                    }
                    CasinoCommand::Fold
                    | CasinoCommand::Check
                    | CasinoCommand::Call
                    | CasinoCommand::Raise { .. } => {
                        if self.kind != GameKind::Holdem {
                            return Err(CasinoError::invalid("허용되지 않는 블랙잭 액션입니다."));
                        }
                        self.ensure_reveal_done(now)?;
                        let action = match command {
                            CasinoCommand::Fold => PokerAction::Fold,
                            CasinoCommand::Check => PokerAction::Check,
                            CasinoCommand::Call => PokerAction::Call,
                            CasinoCommand::Raise { amount } => PokerAction::Raise(*amount),
                            _ => unreachable!(),
                        };
                        poker_action(self, actor, action, now, false)?;
                    }
                    CasinoCommand::Hit
                    | CasinoCommand::Stand
                    | CasinoCommand::Double
                    | CasinoCommand::Split
                    | CasinoCommand::Surrender => {
                        if self.kind != GameKind::Blackjack {
                            return Err(CasinoError::invalid("허용되지 않는 홀덤 액션입니다."));
                        }
                        self.ensure_reveal_done(now)?;
                        let action = match command {
                            CasinoCommand::Hit => BjAction::Hit,
                            CasinoCommand::Stand => BjAction::Stand,
                            CasinoCommand::Double => BjAction::Double,
                            CasinoCommand::Split => BjAction::Split,
                            CasinoCommand::Surrender => BjAction::Surrender,
                            _ => unreachable!(),
                        };
                        blackjack_action(self, actor, action, now, false)?;
                    }
                    CasinoCommand::Join { .. } | CasinoCommand::Chat { .. } => unreachable!(),
                }
            }
        }
        self.reconcile(was_playing, &mut events);
        self.version += 1;
        Ok(events)
    }

    /// 카드 연출이 끝나기 전에는 액션을 받지 않는다.
    fn ensure_reveal_done(&self, now: i64) -> Result<(), CasinoError> {
        if self
            .round
            .as_ref()
            .is_some_and(|round| now < round.reveal_until)
        {
            return Err(CasinoError::new(
                "REVEALING",
                "카드를 여는 중이에요. 잠시만 기다려 주세요.",
            ));
        }
        Ok(())
    }

    /// 연출(카드 열기)이 진행 중인지.
    pub fn is_revealing(&self, now: i64) -> bool {
        self.round
            .as_ref()
            .is_some_and(|round| now < round.reveal_until)
    }

    /// 현재 차례인 좌석의 합법 액션 (홀덤).
    pub fn poker_legal_for(&self, index: i32) -> Option<super::holdem::PokerLegal> {
        poker_legal(self, index)
    }

    /// 현재 차례인 좌석의 합법 액션 (블랙잭).
    pub fn blackjack_legal_for(&self, index: i32) -> Option<super::blackjack::BjLegal> {
        blackjack_legal(self, index)
    }

    /// 테스트·재현용: 정해진 덱으로 라운드를 시작한다.
    pub fn start_with_deck(
        &mut self,
        actor: u64,
        deck: Vec<String>,
        now: i64,
    ) -> Result<Vec<CasinoEvent>, CasinoError> {
        let Some(index) = self.seat_index(actor) else {
            return Err(CasinoError::invalid("먼저 좌석에 앉아 주세요."));
        };
        if self.playing() {
            return Err(CasinoError::invalid("이미 진행 중인 라운드입니다."));
        }
        let _ = index;
        match self.kind {
            GameKind::Holdem => start_poker(self, now, Some(deck))?,
            GameKind::Blackjack => start_blackjack(self, now, Some(deck))?,
        }
        self.version += 1;
        Ok(Vec::new())
    }
}
