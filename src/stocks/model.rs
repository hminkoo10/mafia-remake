// stocks/model.rs — 주식 시장의 자료형: 업종, 회사, 계좌, 주문, 청약, 뉴스, 장부, 운영 규칙

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// 가격 엔진이 한 번 도는 간격.
pub const TICK_MS: i64 = 5_000;
pub const MINUTE_MS: i64 = 60_000;
pub const HOUR_MS: i64 = 60 * MINUTE_MS;
pub const DAY_MS: i64 = 24 * HOUR_MS;
/// 분기 = 실제 1주.
pub const WEEK_MS: i64 = 7 * DAY_MS;
/// 액면가 (플레이어 회사 설립 때 발행 주식 수를 정한다).
pub const PAR_VALUE: i64 = 5_000;
/// 호가창에 보여 주는 단계 수.
pub const BOOK_LEVELS: usize = 10;
/// 시장가 주문이 먹을 수 있는 최대 호가 단계.
pub const MAX_WALK_LEVELS: usize = 40;
/// 뉴스는 이만큼만 남긴다.
pub const NEWS_LIMIT: usize = 300;
/// 회사별 분기 실적 기록.
pub const QUARTER_LIMIT: usize = 8;
/// 사용자별 체결 기록.
pub const FILL_HISTORY_LIMIT: usize = 50;
/// 비율 단위: 만분율, 백만분율.
pub const BP: i64 = 10_000;
pub const PPM: i64 = 1_000_000;

/// 업종.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sector {
    Semiconductor,
    Battery,
    Auto,
    Internet,
    Game,
    Bio,
    Bank,
    Leisure,
    Retail,
    Telecom,
    Shipbuilding,
    Entertainment,
}

impl Sector {
    pub const ALL: [Sector; 12] = [
        Sector::Semiconductor,
        Sector::Battery,
        Sector::Auto,
        Sector::Internet,
        Sector::Game,
        Sector::Bio,
        Sector::Bank,
        Sector::Leisure,
        Sector::Retail,
        Sector::Telecom,
        Sector::Shipbuilding,
        Sector::Entertainment,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Sector::Semiconductor => "반도체",
            Sector::Battery => "2차전지",
            Sector::Auto => "자동차",
            Sector::Internet => "인터넷",
            Sector::Game => "게임",
            Sector::Bio => "바이오",
            Sector::Bank => "은행",
            Sector::Leisure => "레저",
            Sector::Retail => "유통",
            Sector::Telecom => "통신",
            Sector::Shipbuilding => "조선",
            Sector::Entertainment => "엔터",
        }
    }

    /// 업종 이름으로 찾는다 ("반도체", "바이오" …).
    pub fn parse(text: &str) -> Option<Sector> {
        let text = text.trim();
        Sector::ALL.into_iter().find(|sector| sector.name() == text)
    }

    /// 업종의 기준 PBR (주가 / 주당 순자산). 성장 업종일수록 높다.
    pub fn pbr(self) -> f64 {
        match self {
            Sector::Semiconductor => 2.0,
            Sector::Battery => 3.0,
            Sector::Auto => 0.8,
            Sector::Internet => 2.5,
            Sector::Game => 2.2,
            Sector::Bio => 4.0,
            Sector::Bank => 0.5,
            Sector::Leisure => 1.2,
            Sector::Retail => 0.9,
            Sector::Telecom => 0.9,
            Sector::Shipbuilding => 1.1,
            Sector::Entertainment => 2.5,
        }
    }

    /// 업종 팩터의 주간 변동성.
    pub fn weekly_vol(self) -> f64 {
        match self {
            Sector::Semiconductor => 0.045,
            Sector::Battery => 0.06,
            Sector::Auto => 0.03,
            Sector::Internet => 0.045,
            Sector::Game => 0.05,
            Sector::Bio => 0.07,
            Sector::Bank => 0.02,
            Sector::Leisure => 0.035,
            Sector::Retail => 0.025,
            Sector::Telecom => 0.015,
            Sector::Shipbuilding => 0.055,
            Sector::Entertainment => 0.06,
        }
    }

    /// 분기 실적(자기자본이익률)의 종목 고유 변동성.
    pub fn earnings_vol(self) -> f64 {
        match self {
            Sector::Semiconductor => 0.04,
            Sector::Battery => 0.06,
            Sector::Auto => 0.03,
            Sector::Internet => 0.05,
            Sector::Game => 0.06,
            Sector::Bio => 0.12,
            Sector::Bank => 0.015,
            Sector::Leisure => 0.035,
            Sector::Retail => 0.02,
            Sector::Telecom => 0.012,
            Sector::Shipbuilding => 0.05,
            Sector::Entertainment => 0.07,
        }
    }
}

/// 회사 종류.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompanyKind {
    /// 시뮬레이션이 운영하는 회사 (자본은 가상).
    System,
    /// 플레이어가 세운 회사. 자본은 실제 코인이다.
    Player { founder: u64, founder_name: String },
}

/// 회사 상태.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompanyStatus {
    /// 비상장 (플레이어 회사, 상장 전).
    Private,
    /// 공모 청약 중.
    Subscription(IpoOffering),
    /// 상장 (거래 가능).
    Listed,
    /// 상장폐지 전 정리매매·청산 대기. `trading`이 거짓이면 거래정지 상태로 기다린다.
    Liquidating {
        until: i64,
        reason: String,
        trading: bool,
    },
    /// 상장폐지·해산됨 (기록으로만 남는다).
    Delisted { at: i64, reason: String },
}

impl CompanyStatus {
    pub fn is_tradable(&self) -> bool {
        matches!(
            self,
            CompanyStatus::Listed | CompanyStatus::Liquidating { trading: true, .. }
        )
    }

    pub fn is_active(&self) -> bool {
        !matches!(self, CompanyStatus::Delisted { .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            CompanyStatus::Private => "비상장",
            CompanyStatus::Subscription(_) => "공모 청약",
            CompanyStatus::Listed => "상장",
            CompanyStatus::Liquidating { trading: true, .. } => "정리매매",
            CompanyStatus::Liquidating { trading: false, .. } => "청산 대기",
            CompanyStatus::Delisted { .. } => "상장폐지",
        }
    }
}

/// 공모 (IPO).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpoOffering {
    /// 공모가.
    pub price: i64,
    /// 공모 주식 수.
    pub shares: i64,
    pub opens_at: i64,
    pub closes_at: i64,
    /// 청약이 공모 주식의 이 만분율보다 적으면 공모가 무산된다 (플레이어 회사).
    pub min_fill_bp: i64,
}

/// 분기 실적.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuarterResult {
    pub at: i64,
    /// 몇 번째 분기 (상장·설립 뒤 1부터).
    pub quarter: u32,
    /// 순이익 (코인).
    pub profit: i64,
    /// 예상치 (컨센서스).
    pub consensus: i64,
    /// 주당 배당금.
    pub dividend: i64,
    /// 발표 뒤 자본총계.
    pub equity: i64,
}

/// 진행 중인 자사주 매입.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Buyback {
    pub budget: i64,
    pub until: i64,
    pub bought: i64,
}

/// 진행 중인 유상증자 (주주배정).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RightsOffering {
    pub price: i64,
    pub shares: i64,
    pub until: i64,
    /// 사용자별 받은 신주인수권 수.
    pub rights: BTreeMap<u64, i64>,
    /// 사용자별 행사한 수 (행사 대금은 이미 받았다).
    pub exercised: BTreeMap<u64, i64>,
}

/// 예약된 배당 (플레이어 회사, 다음 게임일 시작에 지급).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDividend {
    pub per_share: i64,
    pub pay_at: i64,
}

/// 회사.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Company {
    /// 6자리 종목코드.
    pub code: String,
    pub name: String,
    pub sector: Sector,
    pub kind: CompanyKind,
    pub status: CompanyStatus,
    #[serde(default)]
    pub description: String,
    /// 발행 주식 수.
    pub shares: i64,
    /// 납입 자본 (자본잠식 판단 기준).
    pub paid_in: i64,
    /// 자본총계. 플레이어 회사는 실제 코인(회사 현금)이고, 시스템 회사는 가상이다.
    pub equity: i64,
    pub founded_at: i64,
    #[serde(default)]
    pub listed_at: i64,

    // --- 시세
    /// 현재가 (마지막 체결가).
    pub price: i64,
    /// 시장조성자 호가의 중심 (모형 가격). 체결 직후에는 현재가와 다를 수 있다.
    #[serde(default)]
    pub mid: i64,
    /// 전 게임일 종가 (가격제한폭·VI 기준).
    pub prev_close: i64,
    pub open: i64,
    pub high: i64,
    pub low: i64,
    pub volume: i64,
    /// 거래대금.
    pub turnover: i64,
    /// 지금 시세가 속한 게임일.
    pub day: i64,
    /// 상장 첫날의 가격 범위 (공모가의 60~400%). 그날이 지나면 없어진다.
    #[serde(default)]
    pub first_day_band: Option<(i64, i64)>,

    // --- 가격 모형 (로그 단위)
    pub beta: f64,
    /// 게임 하루 변동성 (잡음 성분).
    pub daily_vol: f64,
    /// 투자 심리 (뉴스·실적·수급이 바꾸고, 몇 주에 걸쳐 0으로 돌아간다).
    pub sentiment: f64,
    /// 아직 반영되지 않은 뉴스 충격 (몇 분에 걸쳐 투자 심리로 옮겨 간다).
    #[serde(default)]
    pub pending_news: f64,
    /// 하루 안의 잡음 (몇 게임일에 걸쳐 0으로 돌아간다).
    pub noise: f64,
    /// 주문이 호가를 먹어 생긴 일시 충격 (몇 분에 걸쳐 사라진다).
    pub impact: f64,
    /// 변동성 군집 (GARCH, 1이 평균).
    pub garch: f64,
    /// 지난 틱의 표준화 충격 (GARCH 갱신용).
    #[serde(default)]
    pub last_shock: f64,

    // --- 유동성
    /// 게임 하루 평균 거래량 (주).
    pub adv: i64,
    /// 시장조성자 호가 한 단계의 수량 (주).
    pub depth: i64,
    /// 시장조성자 스프레드 (만분율, 최소 1호가).
    pub spread_bp: i64,
    /// 플레이어 회사: 시장조성자가 가진 주식 (팔 수 있는 만큼). 시스템 회사는 없음(무제한 유통 물량).
    #[serde(default)]
    pub lp_inventory: Option<i64>,

    // --- 실적·배당
    /// 연 배당수익률 (만분율, 시스템 회사).
    #[serde(default)]
    pub dividend_yield_bp: i64,
    /// 다음 실적 발표 시각.
    pub next_earnings_at: i64,
    /// 다음 실적 예상치 (발표 하루 전에 공개).
    #[serde(default)]
    pub consensus: Option<i64>,
    #[serde(default)]
    pub quarter: u32,
    #[serde(default)]
    pub quarters: VecDeque<QuarterResult>,
    /// 사업 위험도 1~5 (플레이어 회사, 분기 실적의 변동 폭).
    #[serde(default = "default_risk")]
    pub risk: u8,
    #[serde(default)]
    pub risk_changed_at: i64,
    /// 관리종목으로 지정된 시각.
    #[serde(default)]
    pub managed_since: Option<i64>,
    /// 거래정지(변동성 완화장치·관리자) 끝나는 시각.
    #[serde(default)]
    pub halted_until: i64,
    /// 변동성 완화장치 기준가.
    #[serde(default)]
    pub vi_ref: i64,
    #[serde(default)]
    pub admin_halt: bool,

    // --- 플레이어 회사의 경영
    #[serde(default)]
    pub buyback: Option<Buyback>,
    #[serde(default)]
    pub rights: Option<RightsOffering>,
    #[serde(default)]
    pub pending_dividend: Option<PendingDividend>,
    /// 게임 연동 종목: 지난 실적 발표 때의 서버 활동 누적값 (마피아 판, 카지노 핸드, 하우스 손익).
    #[serde(default)]
    pub activity_mark: Activity,
}

fn default_risk() -> u8 {
    2
}

impl Company {
    pub fn founder(&self) -> Option<u64> {
        match &self.kind {
            CompanyKind::Player { founder, .. } => Some(*founder),
            CompanyKind::System => None,
        }
    }

    pub fn is_player(&self) -> bool {
        matches!(self.kind, CompanyKind::Player { .. })
    }

    /// 주당 순자산.
    pub fn bvps(&self) -> f64 {
        if self.shares <= 0 {
            0.0
        } else {
            self.equity.max(0) as f64 / self.shares as f64
        }
    }

    /// 시가총액.
    pub fn market_cap(&self) -> i64 {
        self.price.saturating_mul(self.shares)
    }

    /// 호가 중심 (없으면 현재가).
    pub fn quote_mid(&self) -> i64 {
        if self.mid > 0 {
            self.mid
        } else {
            self.price.max(1)
        }
    }

    pub fn change_bp(&self) -> i64 {
        if self.prev_close <= 0 {
            0
        } else {
            (self.price - self.prev_close) * BP / self.prev_close
        }
    }
}

/// 보유 종목.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    /// 보유 주식 수 (매도 주문에 묶인 수 포함).
    pub qty: i64,
    /// 매도 주문에 묶인 수.
    #[serde(default)]
    pub locked: i64,
    /// 매수 원가 합계 (수수료 포함).
    pub cost: i64,
    /// 보호예수 수량과 풀리는 시각 (설립자 지분).
    #[serde(default)]
    pub lockup_qty: i64,
    #[serde(default)]
    pub lockup_until: i64,
}

impl Position {
    /// 지금 팔 수 있는 수.
    pub fn available(&self, now: i64) -> i64 {
        let lockup = if now < self.lockup_until {
            self.lockup_qty
        } else {
            0
        };
        (self.qty - self.locked - lockup).max(0)
    }
}

/// 체결 한 건 (사용자 기록).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FillRecord {
    pub at: i64,
    pub code: String,
    pub side: Side,
    pub qty: i64,
    pub price: i64,
    /// 수수료 + 세금.
    pub cost: i64,
}

/// 증권 계좌 (코인은 통계 파일에 있고, 여기에는 주식과 기록만 둔다).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    #[serde(default)]
    pub positions: BTreeMap<String, Position>,
    /// 실현 손익 누적.
    #[serde(default)]
    pub realized: i64,
    /// 낸 수수료·세금 누적.
    #[serde(default)]
    pub fees: i64,
    #[serde(default)]
    pub fills: VecDeque<FillRecord>,
    /// 누적 체결 수 (체결 기록은 최근 것만 남기므로 따로 센다).
    #[serde(default)]
    pub trades: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn label(self) -> &'static str {
        match self {
            Side::Buy => "매수",
            Side::Sell => "매도",
        }
    }
}

/// 지정가 주문 (체결되지 않은 부분이 호가에 남는다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub id: u64,
    pub user: u64,
    pub name: String,
    pub code: String,
    pub side: Side,
    pub limit: i64,
    /// 남은 수량.
    pub remaining: i64,
    pub original: i64,
    /// 매수: 아직 쓰지 않은 증거금 (코인).
    #[serde(default)]
    pub reserved: i64,
    pub created_at: i64,
    pub expires_at: i64,
}

/// 공모 청약 한 건.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub user: u64,
    pub name: String,
    pub code: String,
    /// 청약 수량.
    pub qty: i64,
    /// 낸 증거금 (공모가 × 수량).
    pub deposit: i64,
    pub at: i64,
}

/// 뉴스 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewsKind {
    News,
    Rumor,
    Disclosure,
    Earnings,
    Dividend,
    Listing,
    Delisting,
    Halt,
    Macro,
    Market,
}

impl NewsKind {
    pub fn label(self) -> &'static str {
        match self {
            NewsKind::News => "뉴스",
            NewsKind::Rumor => "루머",
            NewsKind::Disclosure => "공시",
            NewsKind::Earnings => "실적",
            NewsKind::Dividend => "배당",
            NewsKind::Listing => "상장",
            NewsKind::Delisting => "상장폐지",
            NewsKind::Halt => "거래정지",
            NewsKind::Macro => "경제",
            NewsKind::Market => "시황",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewsItem {
    pub id: u64,
    pub at: i64,
    pub kind: NewsKind,
    #[serde(default)]
    pub code: Option<String>,
    pub headline: String,
    #[serde(default)]
    pub body: String,
    /// 좋은 소식 +1, 나쁜 소식 -1, 중립 0.
    #[serde(default)]
    pub tone: i8,
}

/// 결과가 나중에 밝혀지는 루머.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingRumor {
    pub code: String,
    pub resolves_at: i64,
    pub truth: bool,
    /// 사실이면 더할 충격, 거짓이면 되돌릴 충격 (로그 단위).
    pub jump: f64,
    pub applied: f64,
    pub topic: String,
}

/// 코인 이동. 사용자 0은 복지 금고다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transfer {
    pub id: u64,
    pub user: u64,
    pub name: String,
    /// + 는 사용자에게 주고, - 는 사용자에게서 가져온다 (금고는 + 만).
    pub amount: i64,
    pub reason: String,
}

pub const TREASURY_USER: u64 = 0;

/// 시장 국면.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Regime {
    /// 완만한 상승·낮은 변동성.
    Calm,
    /// 하락 압력·높은 변동성.
    Turbulent,
}

/// 시장 전체의 가격 요인.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacroState {
    /// 시장 팩터 (로그).
    pub market: f64,
    pub regime: Regime,
    /// 업종 팩터 (로그).
    pub sectors: BTreeMap<Sector, f64>,
}

/// 마피아 종합지수.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexState {
    /// 지수 = 시가총액 합 / divisor.
    pub divisor: f64,
    pub value: f64,
    pub prev_close: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub day: i64,
    /// 서킷브레이커가 오늘 이미 걸렸는지.
    #[serde(default)]
    pub breaker_day: i64,
}

/// 시장 누적 통계 (운영자용).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketStats {
    /// 시장조성자에게서 산 금액 (코인이 사라짐).
    pub lp_bought: i64,
    /// 시장조성자에게 판 금액 (코인이 생김).
    pub lp_sold: i64,
    /// 플레이어끼리 체결된 금액.
    pub p2p: i64,
    pub fees: i64,
    pub taxes: i64,
    /// 시스템 회사 배당으로 생긴 코인.
    pub dividends: i64,
    /// 시스템 회사 공모로 사라진 코인.
    pub ipo_burned: i64,
    /// 플레이어 회사 실적으로 생기거나(+) 사라진(-) 코인.
    pub company_earnings: i64,
}

/// 서버 활동 누적 (게임 연동 종목의 실적 재료). 봇이 게임·카지노가 끝날 때마다 더한다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    #[serde(default)]
    pub mafia_games: i64,
    #[serde(default)]
    pub casino_hands: i64,
    #[serde(default)]
    pub casino_house: i64,
}

/// 봉 하나 (시각은 구간 시작).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candle {
    pub t: i64,
    pub o: i64,
    pub h: i64,
    pub l: i64,
    pub c: i64,
    pub v: i64,
}

/// 분봉(실제 1분, 최근 6시간)과 일봉(게임 하루, 최근 30일치).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandleSeries {
    #[serde(default)]
    pub minute: VecDeque<Candle>,
    #[serde(default)]
    pub day: VecDeque<Candle>,
}

pub const MINUTE_CANDLES: usize = 360;
pub const DAY_CANDLES: usize = 720;

impl CandleSeries {
    /// 가격·거래량을 분봉과 일봉에 넣는다.
    pub fn record(&mut self, now: i64, day_ms: i64, price: i64, volume: i64) {
        push_candle(
            &mut self.minute,
            now.div_euclid(MINUTE_MS) * MINUTE_MS,
            price,
            volume,
            MINUTE_CANDLES,
        );
        push_candle(
            &mut self.day,
            now.div_euclid(day_ms) * day_ms,
            price,
            volume,
            DAY_CANDLES,
        );
    }
}

fn push_candle(list: &mut VecDeque<Candle>, bucket: i64, price: i64, volume: i64, limit: usize) {
    match list.back_mut() {
        Some(last) if last.t == bucket => {
            last.h = last.h.max(price);
            last.l = last.l.min(price);
            last.c = price;
            last.v = last.v.saturating_add(volume);
        }
        _ => {
            list.push_back(Candle {
                t: bucket,
                o: price,
                h: price,
                l: price,
                c: price,
                v: volume,
            });
            while list.len() > limit {
                list.pop_front();
            }
        }
    }
}

/// 종목별 봉과 지수 봉 (지수는 100배 정수). 저장 파일을 따로 둔다 (stocks-candles.json).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandleStore {
    #[serde(default)]
    pub series: BTreeMap<String, CandleSeries>,
    #[serde(default)]
    pub index: CandleSeries,
}

/// 주식 시장 전체 상태 (stocks.json).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StockMarket {
    #[serde(default)]
    pub version: u64,
    pub companies: BTreeMap<String, Company>,
    #[serde(default)]
    pub accounts: BTreeMap<u64, Account>,
    #[serde(default)]
    pub orders: Vec<Order>,
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
    #[serde(default)]
    pub news: VecDeque<NewsItem>,
    #[serde(default)]
    pub rumors: Vec<PendingRumor>,
    /// 아직 통계 파일(코인)에 반영하지 않은 코인 이동.
    #[serde(default)]
    pub journal: Vec<Transfer>,
    #[serde(default)]
    pub next_transfer_id: u64,
    #[serde(default)]
    pub next_order_id: u64,
    #[serde(default)]
    pub next_news_id: u64,
    pub macro_state: MacroState,
    pub index: IndexState,
    #[serde(default)]
    pub stats: MarketStats,
    /// 서킷브레이커로 모든 거래가 멈추는 시각.
    #[serde(default)]
    pub halted_until: i64,
    /// 마지막으로 가격을 갱신한 시각.
    pub last_tick: i64,
    /// 마지막으로 시스템 회사를 새로 상장시킨 게임일.
    #[serde(default)]
    pub last_system_ipo_day: i64,
    #[serde(default)]
    pub activity: Activity,
    /// 관리자가 게임일을 넘긴 만큼 시장 시계가 실제 시각보다 앞선 시간 (ms). 시장의 모든 시각
    /// (시세·주문·청약·배당·보호예수·뉴스·봉)은 이 시계를 쓴다.
    #[serde(default)]
    pub time_shift_ms: i64,
    /// 운영 기록 (체결·주문·청약·회사 작업·배당 등). 봇이 모아 로그 채널로 보낸다. 저장하지 않는다.
    #[serde(skip)]
    pub logs: Vec<String>,
    /// 아직 뉴스 채널에 올리지 않은 뉴스·공시 (틱에서 생긴 것과 회사 설립·공모·배당 결정처럼 명령으로
    /// 생긴 것 모두). 저장하지 않는다.
    #[serde(skip)]
    pub news_outbox: Vec<NewsItem>,
    /// 봉 (파일을 따로 저장한다).
    #[serde(skip)]
    pub candles: CandleStore,
}

/// 운영 규칙 (config.json에서 만든다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StockRules {
    pub enabled: bool,
    /// 게임 하루 길이 (ms).
    pub day_ms: i64,
    /// 매매 수수료 (백만분율, 매수·매도).
    pub fee_ppm: i64,
    /// 증권거래세 (백만분율, 매도).
    pub tax_ppm: i64,
    /// 가격제한폭 (만분율).
    pub limit_bp: i64,
    /// 변동성 완화장치 기준 (만분율).
    pub vi_bp: i64,
    /// 시장 기대 수익률 (만분율, 실제 1주당).
    pub drift_bp_week: i64,
    /// 변동성 배율 (%).
    pub vol_pct: i64,
    /// 뉴스 빈도 배율 (%).
    pub news_pct: i64,
    /// 시스템 회사 1인 보유 한도 (발행 주식의 만분율).
    pub holding_limit_bp: i64,
    /// 주문 1회 최대 수량 (게임 하루 평균 거래량의 %).
    pub order_limit_pct: i64,
    /// 플레이어끼리 체결할 수 있는 가격 범위 (현재가 대비 만분율).
    pub p2p_band_bp: i64,
    /// 지정가 주문을 받는 범위 (현재가 대비 만분율).
    pub order_band_bp: i64,
    /// 지정가 주문 유효 시간 (게임일).
    pub order_days: i64,
    /// 회사 설립 최소 자본금, 설립 수수료(만분율, 금고로).
    pub found_min_capital: i64,
    pub found_fee_bp: i64,
    /// 공모 수수료 (만분율, 공모 대금에서 금고로).
    pub ipo_fee_bp: i64,
    /// 상장 최소 자본총계.
    pub listing_min_equity: i64,
    /// 설립자 보호예수 (게임일).
    pub lockup_days: i64,
    /// 한 사람이 운영할 수 있는 회사 수.
    pub max_companies: i64,
    /// 시스템 회사 수 목표 (모자라면 새로 상장시킨다).
    pub system_companies: i64,
    /// 플레이어 회사 시장조성자 보유 한도 (발행 주식의 만분율).
    pub lp_inventory_bp: i64,
}

impl Default for StockRules {
    fn default() -> Self {
        Self {
            enabled: true,
            day_ms: HOUR_MS,
            fee_ppm: 150,
            tax_ppm: 2_000,
            limit_bp: 3_000,
            vi_bp: 1_000,
            drift_bp_week: 10,
            vol_pct: 100,
            news_pct: 100,
            holding_limit_bp: 500,
            order_limit_pct: 20,
            p2p_band_bp: 200,
            order_band_bp: 1_000,
            order_days: 24,
            found_min_capital: 1_000_000,
            found_fee_bp: 100,
            ipo_fee_bp: 100,
            listing_min_equity: 1_000_000,
            lockup_days: 24,
            max_companies: 1,
            system_companies: 12,
            lp_inventory_bp: 1_000,
        }
    }
}

impl StockRules {
    /// 게임일 번호.
    pub fn day_of(&self, now: i64) -> i64 {
        now.div_euclid(self.day_ms.max(MINUTE_MS))
    }

    pub fn day_ms(&self) -> i64 {
        self.day_ms.max(MINUTE_MS)
    }

    /// 매매 수수료 (최소 1원, 수수료율이 0이면 0).
    pub fn fee(&self, notional: i64) -> i64 {
        if self.fee_ppm <= 0 || notional <= 0 {
            return 0;
        }
        let raw = (i128::from(notional) * i128::from(self.fee_ppm) + i128::from(PPM - 1))
            / i128::from(PPM);
        i64::try_from(raw).unwrap_or(i64::MAX).max(1)
    }

    /// 증권거래세 (내림).
    pub fn tax(&self, notional: i64) -> i64 {
        if self.tax_ppm <= 0 || notional <= 0 {
            return 0;
        }
        let raw = i128::from(notional) * i128::from(self.tax_ppm) / i128::from(PPM);
        i64::try_from(raw).unwrap_or(i64::MAX)
    }
}

/// 만분율 계산 (내림, 음수·0은 0).
pub fn bp_part(amount: i64, bp: i64) -> i64 {
    if amount <= 0 || bp <= 0 {
        return 0;
    }
    let raw = i128::from(amount) * i128::from(bp) / i128::from(BP);
    i64::try_from(raw).unwrap_or(i64::MAX)
}
