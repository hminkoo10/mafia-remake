// 웹 증권 화면(HTS) API 응답 타입. 봇의 src/stock_web.rs, src/stocks/view.rs 직렬화와 맞춘다.

/** 봉 하나 (t = 구간 시작 unix ms). 지수 봉은 100배 정수다. */
export interface Candle {
  t: number;
  o: number;
  h: number;
  l: number;
  c: number;
  v: number;
}

export type CandleRange = "minute" | "day";

export interface CandleResponse {
  code: string;
  range: CandleRange;
  candles: Candle[];
}

export type Side = "buy" | "sell";

export type NewsKind =
  | "news"
  | "rumor"
  | "disclosure"
  | "earnings"
  | "dividend"
  | "listing"
  | "delisting"
  | "halt"
  | "macro"
  | "market";

export interface NewsItem {
  id: number;
  at: number;
  kind: NewsKind;
  code: string | null;
  headline: string;
  body: string;
  /** 좋은 소식 +1, 나쁜 소식 -1, 중립 0. */
  tone: number;
}

export interface CompanySummary {
  code: string;
  name: string;
  sector: string;
  player: boolean;
  founder_name: string | null;
  /** 비상장 · 공모 청약 · 상장 · 정리매매 · 청산 대기 · 상장폐지 */
  status: string;
  price: number;
  prev_close: number;
  change_bp: number;
  volume: number;
  market_cap: number;
  managed: boolean;
  halted: boolean;
}

export interface MarketSummary {
  index: number;
  index_prev: number;
  index_open: number;
  index_high: number;
  index_low: number;
  halted_until: number;
  day_ms: number;
  companies: CompanySummary[];
}

export interface BookLevel {
  price: number;
  qty: number;
  /** 그중 플레이어 주문 수량. */
  players: number;
}

export interface Book {
  /** 매도 호가 (낮은 값부터). */
  asks: BookLevel[];
  /** 매수 호가 (높은 값부터). */
  bids: BookLevel[];
}

export interface QuarterResult {
  at: number;
  quarter: number;
  profit: number;
  consensus: number;
  dividend: number;
  equity: number;
}

export interface IpoOffering {
  price: number;
  shares: number;
  opens_at: number;
  closes_at: number;
  min_fill_bp: number;
}

export interface CompanyDetail {
  summary: CompanySummary;
  description: string;
  shares: number;
  equity: number;
  paid_in: number;
  bvps: number;
  open: number;
  high: number;
  low: number;
  turnover: number;
  lower_limit: number;
  upper_limit: number;
  dividend_yield_bp: number;
  next_earnings_at: number;
  consensus: number | null;
  quarters: QuarterResult[];
  risk: number;
  book: Book;
  ipo: IpoOffering | null;
  ipo_requested: number;
  rights: { price: number; shares: number; until: number } | null;
  buyback: { budget: number; until: number; bought: number } | null;
  pending_dividend: { per_share: number; pay_at: number } | null;
  /** [끝나는 시각, 사유] */
  liquidation: [number, string] | null;
  delisted: [number, string] | null;
  holders: { name: string; qty: number; share_bp: number }[];
  founded_at: number;
  listed_at: number;
}

export interface PositionView {
  code: string;
  name: string;
  qty: number;
  available: number;
  avg_price: number;
  price: number;
  value: number;
  pnl: number;
  pnl_bp: number;
  lockup_qty: number;
  lockup_until: number;
  status: string;
}

export interface OrderView {
  id: number;
  code: string;
  name: string;
  side: Side;
  limit: number;
  remaining: number;
  original: number;
  reserved: number;
  created_at: number;
  expires_at: number;
}

export interface SubscriptionView {
  code: string;
  name: string;
  qty: number;
  deposit: number;
  closes_at: number;
}

export interface RightsClaimView {
  code: string;
  name: string;
  granted: number;
  exercised: number;
  price: number;
  until: number;
}

export interface FillRecord {
  at: number;
  code: string;
  side: Side;
  qty: number;
  price: number;
  /** 수수료 + 세금. */
  cost: number;
}

export interface AccountView {
  positions: PositionView[];
  orders: OrderView[];
  subscriptions: SubscriptionView[];
  rights: RightsClaimView[];
  stock_value: number;
  /** 주문·청약 증거금, 유상증자 대금 (시장에 묶인 코인). */
  pending: number;
  unrealized: number;
  realized: number;
  fees: number;
  fills: FillRecord[];
  companies: string[];
}

export interface OfferingView {
  code: string;
  name: string;
  sector: string;
  player: boolean;
  founder_name: string | null;
  price: number;
  shares: number;
  opens_at: number;
  closes_at: number;
  min_fill_bp: number;
  requested: number;
}

export interface SectorView {
  key: string;
  name: string;
}

export interface StockRulesView {
  enabled: boolean;
  day_ms: number;
  fee_ppm: number;
  tax_ppm: number;
  limit_bp: number;
  found_min_capital: number;
  found_fee_bp: number;
  ipo_fee_bp: number;
  listing_min_equity: number;
  lockup_days: number;
  max_companies: number;
  sectors: SectorView[];
}

export interface StockState {
  /** 시장 시각 (관리자가 게임일을 넘기면 실제 시각보다 앞선다). */
  server_time: number;
  /** 시장 시계가 실제 시각보다 앞선 시간 (ms). */
  time_shift_ms: number;
  me: { user_id: string; name: string; coins: number };
  market: MarketSummary;
  selected: CompanyDetail | null;
  selected_news: NewsItem[];
  account: AccountView;
  news: NewsItem[];
  offerings: OfferingView[];
  my_companies: CompanyDetail[];
  rules: StockRulesView;
}

export interface OrderResult {
  filled: number;
  notional: number;
  avg_price: number;
  fee: number;
  tax: number;
  resting: number | null;
  resting_qty: number;
  limit: number | null;
  refund: number;
}

export interface ActionResponse<T> {
  result: T;
  message: string;
  state: StockState;
}

/** POST /casino/api/stocks/company 의 본문. 설립·신주인수 말고는 내가 대표인 회사(code)에 한다. */
export type CompanyAction =
  | { action: "found"; name: string; sector: string; capital: number }
  | { action: "ipo"; code?: string; price: number; shares: number }
  | { action: "dividend"; code?: string; per_share: number }
  | { action: "rights"; code?: string; shares: number; price: number }
  | { action: "exercise"; code: string; qty: number }
  | { action: "buyback"; code?: string; budget: number }
  | { action: "risk"; code?: string; risk: number }
  | { action: "describe"; code?: string; text: string }
  | { action: "dissolve"; code?: string; confirm: string };

/** 코인이 오가는 작업을 돌린다: 응답의 새 상태를 화면에 반영하고 안내를 띄운다 (실패하면 null). */
export type Act = <T>(work: (token: string) => Promise<ActionResponse<T>>) => Promise<ActionResponse<T> | null>;
