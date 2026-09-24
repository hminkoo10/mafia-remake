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
/// 실제 연출 시간 (ms). 카드의 `reveal_at`은 카드가 펠트에 착지하는 시각이고, 클라이언트는
/// 그보다 `CARD_FLIGHT_MS` 먼저 슈에서 카드를 날리기 시작한다. 착지 간격이 비행 시간보다 길어
/// 공중에는 항상 카드가 한 장뿐이다. 웹 규칙(`TableRules`)과 불변식 테스트는 이 실제 값을 쓴다.
pub mod live_timing {
    /// 슈에서 펠트까지 카드가 나는 시간.
    pub const CARD_FLIGHT_MS: i64 = 420;
    /// 딜·히트를 시작한 뒤 첫 카드가 착지하기까지. 첫 비행은 +530ms에 시작한다: 웹의 딜러 영상은 카드가
    /// 슈를 떠나기 0.33~0.5초 전부터 카드를 집는 동작을 시작하므로, 그 전에 상태가 도착할 여유를 둔다.
    pub const DEAL_LEAD_MS: i64 = 950;
    /// 블랙잭 딜의 착지 간격. 웹의 딜러 영상이 카드 한 장을 꺼내 내려놓고(약 0.81초) 손을 슈 위로
    /// 되돌리는(최대 2.5배속으로 약 0.44초) 동작이 다음 카드 전에 끝나는 간격 (실제 딜러와 비슷한 카드당 1.25초).
    pub const DEAL_CARD_MS: i64 = 1_250;
    /// 홀덤 카드(홀 카드·번 카드·보드)의 착지 간격 (블랙잭과 같은 이유로 같은 값).
    pub const HOLE_CARD_MS: i64 = 1_250;
    /// 마지막 카드가 착지한 뒤 액션을 다시 받기까지.
    pub const LAND_SETTLE_MS: i64 = 250;
    /// 딜러 드로 간격 (뒤집기 600ms + 비행).
    pub const DEALER_DRAW_MS: i64 = 1_000;
    /// 카드 뒤집기 애니메이션 길이 (클라이언트도 같은 값을 쓴다).
    pub const CARD_FLIP_MS: i64 = 600;
    /// 베팅이 끝난 뒤 다음 스트리트의 번 카드가 착지하기까지.
    pub const STREET_PAUSE_MS: i64 = 900;
    /// 쇼다운에서 좌석별 카드를 뒤집는 간격.
    pub const SHOWDOWN_STEP_MS: i64 = 800;
    /// 정산 전후의 뜸 (딜러가 홀 카드를 뒤집기 전, 마지막 공개 뒤 결과까지).
    pub const SETTLE_PAUSE_MS: i64 = 700;
}

/// 테스트에서는 연출 시간을 0으로 두어 게임 흐름을 즉시 검증한다.
const fn staged(live: i64) -> i64 {
    if cfg!(test) { 0 } else { live }
}

pub const CARD_FLIGHT_MS: i64 = staged(live_timing::CARD_FLIGHT_MS);
pub const DEAL_LEAD_MS: i64 = staged(live_timing::DEAL_LEAD_MS);
pub const DEAL_CARD_MS: i64 = staged(live_timing::DEAL_CARD_MS);
pub const HOLE_CARD_MS: i64 = staged(live_timing::HOLE_CARD_MS);
pub const LAND_SETTLE_MS: i64 = staged(live_timing::LAND_SETTLE_MS);
pub const DEALER_DRAW_MS: i64 = staged(live_timing::DEALER_DRAW_MS);
pub const CARD_FLIP_MS: i64 = staged(live_timing::CARD_FLIP_MS);
pub const STREET_PAUSE_MS: i64 = staged(live_timing::STREET_PAUSE_MS);
pub const SHOWDOWN_STEP_MS: i64 = staged(live_timing::SHOWDOWN_STEP_MS);
pub const SETTLE_PAUSE_MS: i64 = staged(live_timing::SETTLE_PAUSE_MS);
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
/// 방 설정으로 고를 수 있는 한도.
pub const SETTINGS_MAX_CHIPS: i64 = 10_000_000;
pub const SETTINGS_MIN_TURN_SECS: i64 = 10;
pub const SETTINGS_MAX_TURN_SECS: i64 = 120;

/// 방 설정: 관리자가 테이블을 만들 때 정한다. 저장 파일에 없던 항목은 기본값을 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TableSettings {
    /// 홀덤 블라인드.
    pub small_blind: i64,
    pub big_blind: i64,
    /// 바이인 범위 (BUY_IN_STEP 단위).
    pub min_buy_in: i64,
    pub max_buy_in: i64,
    /// 블랙잭 메인 베팅 범위 (BJ_BET_STEP 단위).
    pub min_bet: i64,
    pub max_bet: i64,
    /// 블랙잭 사이드베팅(퍼펙트 페어·21+3) 최대. 0이면 사이드베팅을 받지 않는다.
    pub side_bet_max: i64,
    /// 액션 제한 시간 (ms).
    pub turn_ms: i64,
}

impl Default for TableSettings {
    fn default() -> Self {
        Self {
            small_blind: HOLDEM_SMALL_BLIND,
            big_blind: HOLDEM_BIG_BLIND,
            min_buy_in: MIN_BUY_IN,
            max_buy_in: MAX_BUY_IN,
            min_bet: BJ_MIN_BET,
            max_bet: BJ_MAX_BET,
            side_bet_max: SIDE_BET_MAX,
            turn_ms: TURN_MS,
        }
    }
}

/// 관리자가 고른 방 설정. 비운 항목은 게임 종류와 다른 값에 맞춰 자동으로 정한다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettingsRequest {
    pub min_bet: Option<i64>,
    pub max_bet: Option<i64>,
    pub big_blind: Option<i64>,
    pub min_buy_in: Option<i64>,
    pub max_buy_in: Option<i64>,
    pub side_bet_max: Option<i64>,
    pub turn_secs: Option<i64>,
}

fn round_down(value: i64, step: i64) -> i64 {
    value / step * step
}

impl TableSettings {
    /// 요청을 검증해 방 설정을 만든다. 게임 종류와 상관없는 항목은 무시한다.
    pub fn build(kind: GameKind, request: SettingsRequest) -> Result<Self, String> {
        let mut settings = Self::default();
        let turn_secs = request.turn_secs.unwrap_or(TURN_MS / 1000);
        if !(SETTINGS_MIN_TURN_SECS..=SETTINGS_MAX_TURN_SECS).contains(&turn_secs) {
            return Err(format!(
                "제한 시간은 {SETTINGS_MIN_TURN_SECS}~{SETTINGS_MAX_TURN_SECS}초로 정해 주세요."
            ));
        }
        settings.turn_ms = turn_secs * 1000;
        let (default_min_buy_in, default_max_buy_in, floor_buy_in) = match kind {
            GameKind::Holdem => {
                let big = request.big_blind.unwrap_or(HOLDEM_BIG_BLIND);
                if big < 10 || big % 2 != 0 || big > SETTINGS_MAX_CHIPS / 200 {
                    return Err(format!(
                        "빅 블라인드는 10~{} 사이의 짝수로 정해 주세요.",
                        format_chips(SETTINGS_MAX_CHIPS / 200)
                    ));
                }
                settings.big_blind = big;
                settings.small_blind = big / 2;
                // 바이인 기본값: 빅 블라인드 50~200개. 최소 10개는 있어야 한다.
                (big * 50, big * 200, big * 10)
            }
            GameKind::Blackjack => {
                let min_bet = request.min_bet.unwrap_or(BJ_MIN_BET);
                let max_bet = request
                    .max_bet
                    .unwrap_or_else(|| (min_bet * 50).max(BJ_MAX_BET));
                if min_bet < BJ_MIN_BET
                    || min_bet % BJ_BET_STEP != 0
                    || min_bet > SETTINGS_MAX_CHIPS
                {
                    return Err(format!(
                        "최소 베팅은 {}~{} 사이, {} 단위로 정해 주세요.",
                        format_chips(BJ_MIN_BET),
                        format_chips(SETTINGS_MAX_CHIPS),
                        format_chips(BJ_BET_STEP)
                    ));
                }
                if max_bet < min_bet || max_bet % BJ_BET_STEP != 0 || max_bet > SETTINGS_MAX_CHIPS {
                    return Err(format!(
                        "최대 베팅은 최소 베팅({}) 이상 {} 이하, {} 단위로 정해 주세요.",
                        format_chips(min_bet),
                        format_chips(SETTINGS_MAX_CHIPS),
                        format_chips(BJ_BET_STEP)
                    ));
                }
                let side_max = request
                    .side_bet_max
                    .unwrap_or_else(|| round_down(max_bet / 2, BJ_BET_STEP).max(SIDE_BET_MIN));
                if side_max != 0
                    && (side_max < SIDE_BET_MIN
                        || side_max > max_bet
                        || side_max % BJ_BET_STEP != 0)
                {
                    return Err(format!(
                        "사이드베팅 최대는 0(사용 안 함) 또는 {}~{}(최대 베팅) 사이, {} 단위로 정해 주세요.",
                        format_chips(SIDE_BET_MIN),
                        format_chips(max_bet),
                        format_chips(BJ_BET_STEP)
                    ));
                }
                settings.min_bet = min_bet;
                settings.max_bet = max_bet;
                settings.side_bet_max = side_max;
                // 바이인 기본값: 최소 베팅 50번, 최대 베팅 4번. 최소 베팅 한 번은 걸 수 있어야 한다.
                let min_buy_in = (min_bet * 50).max(MIN_BUY_IN);
                (min_buy_in, (max_bet * 4).max(min_buy_in), min_bet)
            }
        };
        let min_buy_in = request.min_buy_in.unwrap_or_else(|| {
            let base = round_down(default_min_buy_in.min(SETTINGS_MAX_CHIPS), BUY_IN_STEP);
            // 최대 바이인만 낮게 정했으면 최소 바이인도 거기에 맞춘다 (하한 검사는 아래에서).
            request.max_buy_in.map_or(base, |max| base.min(max))
        });
        let max_buy_in = request.max_buy_in.unwrap_or_else(|| {
            round_down(default_max_buy_in.min(SETTINGS_MAX_CHIPS), BUY_IN_STEP).max(min_buy_in)
        });
        if min_buy_in < floor_buy_in.max(BUY_IN_STEP)
            || min_buy_in % BUY_IN_STEP != 0
            || min_buy_in > SETTINGS_MAX_CHIPS
        {
            return Err(format!(
                "최소 바이인은 {} 이상 {} 이하, {} 단위로 정해 주세요.",
                format_chips(round_up(floor_buy_in.max(BUY_IN_STEP), BUY_IN_STEP)),
                format_chips(SETTINGS_MAX_CHIPS),
                format_chips(BUY_IN_STEP)
            ));
        }
        if max_buy_in < min_buy_in
            || max_buy_in % BUY_IN_STEP != 0
            || max_buy_in > SETTINGS_MAX_CHIPS
        {
            return Err(format!(
                "최대 바이인은 최소 바이인({}) 이상 {} 이하, {} 단위로 정해 주세요.",
                format_chips(min_buy_in),
                format_chips(SETTINGS_MAX_CHIPS),
                format_chips(BUY_IN_STEP)
            ));
        }
        settings.min_buy_in = min_buy_in;
        settings.max_buy_in = max_buy_in;
        Ok(settings)
    }

    /// 판돈 한 줄 ("블라인드 50/100" 또는 "베팅 100~5,000").
    pub fn stakes(&self, kind: GameKind) -> String {
        match kind {
            GameKind::Holdem => format!(
                "블라인드 {}/{}",
                format_chips(self.small_blind),
                format_chips(self.big_blind)
            ),
            GameKind::Blackjack => format!(
                "베팅 {}~{}",
                format_chips(self.min_bet),
                format_chips(self.max_bet)
            ),
        }
    }

    /// 한 줄 요약 ("베팅 100~5,000 · 사이드 최대 2,500 · 바이인 5,000~20,000 · 제한 30초").
    pub fn summary(&self, kind: GameKind) -> String {
        let mut stakes = self.stakes(kind);
        if kind == GameKind::Blackjack {
            if self.side_bet_max > 0 {
                stakes.push_str(&format!(
                    " · 사이드 최대 {}",
                    format_chips(self.side_bet_max)
                ));
            } else {
                stakes.push_str(" · 사이드베팅 없음");
            }
        }
        format!(
            "{stakes} · 바이인 {}~{} · 제한 {}초",
            format_chips(self.min_buy_in),
            format_chips(self.max_buy_in),
            self.turn_ms / 1000
        )
    }
}

fn round_up(value: i64, step: i64) -> i64 {
    (value + step - 1) / step * step
}

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
    /// 적중한 사이드베팅·인슈어런스의 이익 합계 (원금 제외).
    #[serde(default)]
    pub side_won: i64,
    /// 적중한 사이드베팅·인슈어런스가 돌려준 총액 (원금 포함).
    #[serde(default)]
    pub side_paid: i64,
    /// 사이드베팅·인슈어런스 결과 설명 (이번 라운드).
    #[serde(default)]
    pub side_notes: Vec<String>,
    /// 이번 라운드에 딴 칩 중 아직 화면에 보이면 안 되는 몫. 실제 스택(`stack`)에는 이미
    /// 들어 있고, 화면만 `pending_until`까지 이만큼 뺀 스택을 보여 준다 (카드가 다 놓이기 전에
    /// 결과가 새지 않게). 다음 라운드를 시작할 때 비운다.
    #[serde(default)]
    pub pending_credit: i64,
    /// `pending_credit`이 화면에 드러나는 시각 (그 칩을 만든 연출이 끝나는 시각).
    #[serde(default)]
    pub pending_until: i64,
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
            side_won: 0,
            side_paid: 0,
            side_notes: Vec::new(),
            pending_credit: 0,
            pending_until: 0,
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

    /// 딴 칩을 스택에 넣고, `until`까지는 화면에 보이지 않게 보류한다. 이미 드러난 이전
    /// 보류분은 다시 숨기지 않는다 (히트마다 사이드베팅 당첨금이 사라졌다 나타나지 않게).
    pub(super) fn credit_after(&mut self, amount: i64, until: i64, now: i64) {
        if amount <= 0 {
            return;
        }
        self.stack += amount;
        if now >= self.pending_until {
            self.pending_credit = 0;
        }
        self.pending_credit += amount;
        self.pending_until = self.pending_until.max(until);
    }

    /// 라운드를 시작할 때 지난 라운드의 보류분을 비운다.
    pub(super) fn clear_pending_credit(&mut self) {
        self.pending_credit = 0;
        self.pending_until = 0;
    }

    /// `now`에 화면에 보여 줄 스택 (아직 드러나면 안 되는 당첨금을 뺀 값).
    pub fn visible_stack(&self, now: i64) -> i64 {
        if now < self.pending_until {
            (self.stack - self.pending_credit).max(0)
        } else {
            self.stack.max(0)
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

    /// 라운드가 무효가 될 때 돌려줄, 아직 정산되지 않은 베팅.
    /// `phase`는 진행 중인 라운드의 단계 (없거나 끝났으면 None).
    fn unsettled_stake(&self, kind: GameKind, phase: Option<Phase>) -> i64 {
        match (kind, phase) {
            // 끝난 라운드의 total은 이미 정산된 기록일 뿐이다.
            (_, None | Some(Phase::Complete)) => 0,
            // 홀덤 팟은 핸드가 끝날 때 한 번에 정산된다.
            (GameKind::Holdem, Some(_)) => self.total,
            // 딜 전에는 사이드베팅까지 아무것도 정산되지 않았다.
            (GameKind::Blackjack, Some(Phase::Betting)) => self.total,
            // 딜 때 사이드베팅이 정산되었다. 인슈어런스는 딜러 확인 전(Insurance)에만 남아 있다.
            (GameKind::Blackjack, Some(phase)) => {
                let main = self.hands.iter().map(|hand| hand.bet).sum::<i64>();
                let insurance = if phase == Phase::Insurance {
                    self.insurance
                } else {
                    0
                };
                main + insurance
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Round {
    pub id: String,
    pub deck: Vec<String>,
    /// 블랙잭의 실제 슈에서 가져온 덱. 재현용 명시 덱은 슈에 반환하지 않는다.
    #[serde(default)]
    pub uses_shoe: bool,
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
    /// 라운드가 정산될 때의 엔진 단계 (홀덤). 정산 뒤 카드가 놓이는 동안 화면 단계가 이보다 앞으로 되돌아가지 않게 한다.
    #[serde(default)]
    pub settled_from: Option<Phase>,
    /// 보드 카드가 착지하는 시각 (보드와 같은 순서). 플롭 세 장은 뒷면으로 놓였다가
    /// `board_flip_at`에 함께 뒤집히고, 턴·리버는 앞면으로 놓인다.
    #[serde(default)]
    pub board_reveal_at: Vec<i64>,
    /// 플롭 세 장을 함께 뒤집는 시각 (플롭 전에는 0).
    #[serde(default)]
    pub board_flip_at: i64,
    /// 이번 핸드에서 뒷면으로 버린 번 카드가 착지한 시각 (스트리트마다 한 장, 값은 보내지 않는다).
    #[serde(default)]
    pub burn_at: Vec<i64>,
    /// 딜러 카드가 착지하는 시각 (블랙잭).
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
    /// 이긴 베팅의 이익 합계 (원금·다른 베팅의 손실 제외).
    #[serde(default)]
    pub won: i64,
    /// 이긴 베팅이 돌려준 총액 (원금 포함). 결과 헤드라인은 에볼루션처럼 이 값을 보여 준다
    /// (블랙잭 5,000 → 12,500). 예전 기록에는 없어서 0이다.
    #[serde(default)]
    pub paid: i64,
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

    /// 보낸 사람이 본 테이블 상태가 최신이어야 뜻이 맞는 명령. 콜 금액과 레이즈 목표는 그 사이
    /// 다른 사람의 베팅으로 바뀔 수 있다. 나머지(베팅·입장·퇴장·시작·블랙잭 액션 등)는 다른
    /// 사람의 동작과 상관없이 같은 뜻이고 규칙 검사가 막아 주므로, 여러 명이 동시에 누를 때
    /// 늦은 사람의 명령을 STALE_STATE로 거절하지 않는다 (마감 직전 자동 확정 베팅이 사라지던 문제).
    pub fn needs_current_state(&self) -> bool {
        matches!(self, Self::Call | Self::Raise { .. })
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
        reveal_until: i64,
        table_id: String,
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
    /// 블랙잭 슈 (라운드 진행 중에는 round.deck으로 이동).
    #[serde(default)]
    pub shoe: Vec<String>,
    /// 슈 바닥 기준 컷 카드 위치.
    #[serde(default)]
    pub shoe_cut: usize,
    #[serde(default)]
    pub shoe_total: usize,
    #[serde(default)]
    pub shuffled_at: i64,
    /// 방 설정 (베팅·블라인드·바이인 한도, 제한 시간).
    #[serde(default)]
    pub settings: TableSettings,
    #[serde(default)]
    pub pending_settings: Option<TableSettings>,
    /// 딜러 안내 예약: 결과 안내는 카드가 다 놓인 뒤에야 화면에 뜬다. 첫 항목은 지금 보이는
    /// 안내이고, 뒤로 갈수록 나중에 뜰 안내다 (`narration`은 가장 마지막 안내).
    #[serde(default)]
    pub narrations: Vec<Narration>,
}

/// 딜러 안내 한 줄과 그 안내가 화면에 뜨는 시각.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Narration {
    pub at: i64,
    pub text: String,
}

/// 안내 예약이 이보다 길어지면 오래된 것부터 버린다.
const NARRATION_LIMIT: usize = 16;

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
            shoe: Vec::new(),
            shoe_cut: 0,
            shoe_total: 0,
            shuffled_at: 0,
            settings: TableSettings::default(),
            pending_settings: None,
            narrations: Vec::new(),
        }
    }

    /// 방 설정을 바꾼 테이블 (생성 직후에만 쓴다).
    pub fn with_settings(mut self, settings: TableSettings) -> Self {
        self.settings = settings;
        self
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

    /// 방 설정을 바꾼다. 라운드 중에는 블라인드·베팅 한도·마감 시각이 이미 정해져 있으므로
    /// 기다렸다가 그 라운드가 끝날 때(`reconcile`) 적용한다. 지금 적용했으면(또는 기다리던
    /// 변경을 취소해 지금 설정이 그대로 쓰이면) true.
    pub fn change_settings(&mut self, settings: TableSettings, now: i64) -> bool {
        if !self.playing() {
            self.settings = settings;
            self.pending_settings = None;
            self.announce_settings(
                format!("방 설정이 바뀌었습니다: {}", settings.summary(self.kind)),
                now,
            );
            self.version += 1;
            true
        } else if settings == self.settings {
            // 지금 설정으로 되돌렸다: 기다리던 변경만 취소한다.
            if self.pending_settings.take().is_some() {
                self.announce_settings("다음 라운드에 바꾸려던 방 설정을 취소했습니다.", now);
                self.version += 1;
            }
            true
        } else {
            self.pending_settings = Some(settings);
            self.announce_settings("이번 라운드가 끝나면 방 설정이 바뀝니다.", now);
            self.version += 1;
            false
        }
    }

    /// 방 설정 안내는 딜러 채팅으로만 보낸다. 딜러 말풍선(인슈어런스 안내, 결과 등)을 덮으면
    /// 플레이어가 지금 해야 할 일을 놓친다. `at`은 채팅에 뜨는 시각.
    fn announce_settings(&mut self, text: impl Into<String>, at: i64) {
        let dealer = self.dealer_profile().name.to_string();
        self.push_message(dealer, text.into(), at, true, None);
    }

    /// 라운드가 끝났을 때 적용하지 못한 설정(예: 그 전에 저장된 파일)이 남아 있으면
    /// 다음 라운드를 시작하기 전에 적용한다. 보통은 라운드가 끝날 때 `reconcile`이 적용한다.
    fn apply_pending_settings(&mut self, now: i64) {
        if let Some(next) = self.pending_settings.take() {
            self.settings = next;
            self.announce_settings(
                format!("방 설정이 바뀌었습니다: {}", next.summary(self.kind)),
                now,
            );
            self.version += 1;
        }
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
        self.say_at(text, now, now);
    }

    /// `at`에 화면에 뜨는 딜러 안내. 결과처럼 카드가 다 놓인 뒤에야 참이 되는 안내는
    /// 그 시각을 `at`으로 준다. 그때까지 화면(안내·채팅·Discord 중계)은 이전 안내를 유지한다.
    pub(super) fn say_at(&mut self, text: impl Into<String>, now: i64, at: i64) {
        let text = text.into();
        let at = at.max(now);
        if self.narrations.is_empty() {
            // 예전 저장본: 지금 보이는 안내를 첫 항목으로 둔다.
            self.narrations.push(Narration {
                at: i64::MIN,
                text: self.narration.clone(),
            });
        }
        // 화면에 뜨는 시각 순으로 둔다 (같은 시각이면 나중에 한 말이 뒤). 예약된 결과 안내 뒤에
        // 바로 한 말(채팅 답 등)이 있어도, 결과 시각이 되면 결과가 보인다. 정렬은 안정적이라
        // 같은 시각의 순서가 유지된다 (예전 저장본은 넣은 순서일 수 있어 먼저 정렬한다).
        self.narrations.sort_by_key(|entry| entry.at);
        let position = self.narrations.partition_point(|entry| entry.at <= at);
        self.narrations.insert(
            position,
            Narration {
                at,
                text: text.clone(),
            },
        );
        // 지금 직전까지 보이던 안내보다 먼저 뜬 항목은 다시 보일 일이 없다.
        if let Some(visible) = self.narrations.iter().rposition(|entry| entry.at < now) {
            self.narrations.drain(..visible);
        }
        let overflow = self.narrations.len().saturating_sub(NARRATION_LIMIT);
        if overflow > 0 {
            self.narrations.drain(..overflow);
        }
        // 저장되는 안내는 가장 늦게 뜨는 안내다.
        self.narration = self
            .narrations
            .last()
            .map_or_else(|| text.clone(), |entry| entry.text.clone());
        let dealer = self.dealer_profile().name.to_string();
        self.push_message(dealer, text, at, true, None);
    }

    /// `now`에 화면에 보이는 딜러 안내: 이미 시각이 된 안내 중 가장 늦게 뜬 것 (같은 시각이면
    /// 나중에 한 말). 목록 순서에 기대지 않는다 (예전 저장본은 넣은 순서일 수 있다).
    pub fn narration_at(&self, now: i64) -> &str {
        self.narrations
            .iter()
            .filter(|entry| entry.at <= now)
            // max_by_key는 같은 값이면 마지막 항목을 준다.
            .max_by_key(|entry| entry.at)
            .or_else(|| self.narrations.iter().min_by_key(|entry| entry.at))
            .map_or(self.narration.as_str(), |entry| entry.text.as_str())
    }

    /// `now`에 화면에 보이는 핸드 기록. 이번 라운드의 결과는 연출이 끝난 뒤에 보인다.
    pub fn visible_history(&self, now: i64) -> &[HandResult] {
        let hide_latest = self.round.as_ref().is_some_and(|round| {
            now < round.reveal_until
                && self
                    .history
                    .first()
                    .is_some_and(|result| result.id == round.id)
        });
        &self.history[usize::from(hide_latest)..]
    }

    /// `after < t <= until` 사이에 화면에 새로 드러나는 것(연출 끝, 딜러 홀 카드 뒤집기와 함께
    /// 바뀌는 단계, 예약된 안내·채팅, 보류된 당첨금)이 있는지. 허브가 이 시각에 한 번만 화면을
    /// 다시 밀어 준다.
    pub fn reveals_between(&self, after: i64, until: i64) -> bool {
        let crossed = |at: i64| after < at && at <= until;
        self.round.as_ref().is_some_and(|round| {
            crossed(round.reveal_until) || (round.reveal && crossed(round.dealer_flip_at))
        }) || self.messages.iter().any(|message| crossed(message.at))
            || self.narrations.iter().any(|entry| crossed(entry.at))
            || self
                .seats
                .iter()
                .flatten()
                .any(|seat| crossed(seat.pending_until))
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

    /// 퇴장 대기 좌석 중 진행 중인 라운드에 들지 않은 좌석의 칩을 돌려준다. 하나라도 비웠으면
    /// true. 지난 라운드의 카드를 여는 중에는 부르지 않는다 (당첨금이 코인으로 먼저 새지 않게).
    fn cash_out_leavers(&mut self, events: &mut Vec<CasinoEvent>) -> bool {
        let playing = self.playing();
        let mut any = false;
        for index in 0..SEAT_COUNT {
            let leaving = self.seats[index]
                .as_ref()
                .is_some_and(|seat| seat.leaving && !(playing && seat.in_hand));
            if leaving {
                self.cash_out(index, events);
                any = true;
            }
        }
        any
    }

    /// 정산 기록의 하우스 손익 = 플레이어 순손익의 반대 (사이드베팅·인슈어런스 포함).
    /// 홀덤은 플레이어끼리 주고받으므로 0이다.
    fn house_delta(&self, result: &HandResult) -> i64 {
        if self.kind == GameKind::Blackjack {
            -result.results.iter().map(|entry| entry.net).sum::<i64>()
        } else {
            0
        }
    }

    /// 아직 카드를 여는 중인 이번 라운드가 하우스에 더한 손익. 하우스 누적을 보여 줄 때 빼서
    /// 결과가 카드보다 먼저 새지 않게 한다 (실제 누적 값은 그대로다).
    pub fn unrevealed_house_delta(&self, now: i64) -> i64 {
        let Some(round) = self
            .round
            .as_ref()
            .filter(|round| round.phase == Phase::Complete && now < round.reveal_until)
        else {
            return 0;
        };
        self.history
            .first()
            .filter(|result| result.id == round.id)
            .map_or(0, |result| self.house_delta(result))
    }

    /// 라운드가 막 끝났으면 결과 이벤트를 만들고, 퇴장 대기 좌석을 정리한다 (카드를 여는
    /// 중이면 연출이 끝난 뒤 `tick`이 정리한다). 라운드 중에 바꾼 방 설정도 여기서 적용한다.
    fn reconcile(&mut self, was_playing: bool, events: &mut Vec<CasinoEvent>, now: i64) {
        let just_completed = was_playing && !self.playing() && self.round.is_some();
        if !just_completed {
            return;
        }
        let revealed_at = self
            .round
            .as_ref()
            .map_or(now, |round| round.reveal_until.max(now));
        if let Some(next) = self.pending_settings.take() {
            self.settings = next;
            // 결과 안내(딜러 말풍선)는 그대로 두고 채팅으로만 알린다. 결과보다 먼저 뜨지 않게
            // 카드를 다 연 뒤에 보인다.
            self.announce_settings(
                format!("방 설정이 바뀌었습니다: {}", next.summary(self.kind)),
                revealed_at,
            );
        }
        // 베팅 없이 끝난 블랙잭 라운드처럼 기록이 없으면 이벤트도 없다 (예전 기록을 다시 보내지 않게).
        let round_id = self.round.as_ref().map(|round| round.id.clone());
        let settled = self
            .history
            .first()
            .filter(|result| Some(&result.id) == round_id.as_ref())
            .cloned();
        if let Some(result) = settled {
            let house_delta = self.house_delta(&result);
            events.push(CasinoEvent::RoundSettled {
                result,
                house_delta,
                reveal_until: self.round.as_ref().map_or(now, |round| round.reveal_until),
                table_id: self.id.clone(),
            });
        }
        if !self.is_revealing(now) {
            self.cash_out_leavers(events);
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
                self.reconcile(was_playing, &mut events, now);
            }
        }
        // 카드를 여는 동안 미뤄 둔 퇴장: 연출이 끝나면 남은 칩을 돌려준다 (버전이 올라 허브가
        // 알리고 저장한다).
        if !self.playing() && !self.is_revealing(now) && self.cash_out_leavers(&mut events) {
            self.version += 1;
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
        // 진행 중이던 라운드의 아직 정산되지 않은 베팅만 돌려준다.
        // 끝난 라운드의 total까지 돌려주면 이미 정산된 칩이 두 번 나간다.
        let phase = self
            .round
            .as_ref()
            .map(|round| round.phase)
            .filter(|_| self.playing());
        let kind = self.kind;
        for seat in self.seats.iter_mut().flatten() {
            seat.stack += seat.unsettled_stake(kind, phase);
            seat.total = 0;
        }
        for index in 0..SEAT_COUNT {
            self.cash_out(index, &mut events);
        }
        self.round = None;
        self.version += 1;
        events
    }

    /// 명령을 적용한다. 최신 상태가 필요한 명령(`needs_current_state`)은 `expected_version`이
    /// 현재 버전과 다르면 STALE_STATE로 거부한다. 바이인 코인 차감·좌석 중복(다른 테이블)
    /// 검사는 호출자가 먼저 한다.
    pub fn apply_command(
        &mut self,
        actor: u64,
        actor_name: &str,
        command: &CasinoCommand,
        expected_version: Option<u64>,
        now: i64,
    ) -> Result<Vec<CasinoEvent>, CasinoError> {
        let mut events = Vec::new();
        if command.needs_current_state()
            && let Some(expected) = expected_version
            && expected != self.version
        {
            return Err(CasinoError::new(
                "STALE_STATE",
                "테이블 상태가 바뀌었습니다. 현재 상태를 확인한 후 다시 선택해 주세요.",
            ));
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
                let rules = self.settings;
                if *amount < rules.min_buy_in
                    || *amount > rules.max_buy_in
                    || amount % BUY_IN_STEP != 0
                {
                    return Err(CasinoError::invalid(format!(
                        "바이인은 {}~{} 칩, {} 단위입니다.",
                        format_chips(rules.min_buy_in),
                        format_chips(rules.max_buy_in),
                        format_chips(BUY_IN_STEP)
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
                        // 끝난 핸드라도 카드를 여는 중이면 퇴장을 예약한다: 지금 칩을 돌려주면
                        // 아직 가려 둔 당첨금이 코인으로 먼저 샌다 (연출이 끝나면 `tick`이 정리한다).
                        let in_hand = (self.playing() || self.is_revealing(now))
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
                        // 지난 라운드의 카드를 다 열기 전에는 새 라운드를 시작하지 않는다.
                        self.ensure_reveal_done(now)?;
                        self.apply_pending_settings(now);
                        self.rotate_dealer_if_due(now);
                        let minimum = if self.kind == GameKind::Holdem {
                            1
                        } else {
                            self.settings.min_bet
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
                        // 지난 라운드 뒤 아직 정리되지 않은 퇴장 대기 좌석 (새 라운드에는 들지 않는다).
                        self.cash_out_leavers(&mut events);
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
                        self.ensure_reveal_done(now)?;
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
        self.reconcile(was_playing, &mut events, now);
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
        self.ensure_reveal_done(now)?;
        self.apply_pending_settings(now);
        let _ = index;
        match self.kind {
            GameKind::Holdem => start_poker(self, now, Some(deck))?,
            GameKind::Blackjack => start_blackjack(self, now, Some(deck))?,
        }
        let mut events = Vec::new();
        self.cash_out_leavers(&mut events);
        self.version += 1;
        Ok(events)
    }
}
