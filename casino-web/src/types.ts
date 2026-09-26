// 봇의 /casino/api 응답 타입 (src/casino/view.rs, src/casino_hub.rs, src/casino_web.rs 와 맞춘다)

export type GameKind = "holdem" | "blackjack";
export type Phase = "preflop" | "flop" | "turn" | "river" | "betting" | "insurance" | "playing" | "complete";
export type HandStatus = "playing" | "stand" | "bust" | "surrender";

export interface HandView {
  id: string;
  cards: string[];
  reveal_at: number[];
  bet: number;
  total: number;
  soft: boolean;
  status: HandStatus;
  result: string | null;
  payout: number | null;
}

export interface SeatView {
  seat: number;
  user_id: number;
  name: string;
  stack: number;
  mine: boolean;
  bet: number;
  total: number;
  folded: boolean;
  in_hand: boolean;
  sit_out: boolean;
  leaving: boolean;
  /** 홀덤: 핸드가 끝난 뒤 본인이 골라 카드를 보여 줬다. */
  shown?: boolean;
  cards: string[];
  cards_reveal_at: number[];
  hands: HandView[];
  side_pairs: number;
  side_plus3: number;
  insurance: number;
  insurance_decided: boolean;
  side_notes: string[];
  hand_name: string | null;
  hand_cards: string[];
  /** 쇼다운에서 이 좌석의 카드를 뒤집는 서버 시각 (0 = 없음). 그 전까지 웹은 뒷면으로 둔다. */
  showdown_at?: number;
}

export interface RoundView {
  id: string;
  phase: Phase;
  phase_text: string;
  board: string[];
  dealer: string[];
  dealer_total: number | null;
  turn: number;
  hand: number;
  deadline: number;
  current_bet: number;
  pot: number;
  reveal: boolean;
  reveal_until: number;
  board_reveal_at: number[];
  dealer_reveal_at: number[];
  /** 딜러 홀 카드를 뒤집는 서버 시각 (0 = 없음). 정산 뒤에만 진짜 카드가 온다. */
  dealer_flip_at?: number;
  /** 플롭 세 장을 함께 뒤집는 서버 시각 (0 = 없음). 그 전까지 플롭은 뒷면으로 놓인다. */
  board_flip_at?: number;
  /** 이번 핸드에 버린 카드(번 카드)가 머크에 놓이는 시각. 카드 값은 오지 않는다. */
  burn_at?: number[];
}

export interface PokerLegal {
  to_call: number;
  can_check: boolean;
  can_raise: boolean;
  min_raise_to: number;
  max_raise_to: number;
}

export interface BjLegal {
  can_double: boolean;
  can_split: boolean;
  can_surrender: boolean;
}

export interface LegalView {
  poker: PokerLegal | null;
  blackjack: BjLegal | null;
  can_bet: boolean;
  can_start: boolean;
  can_insure: boolean;
  insurance_cost: number;
  /** 홀덤: 끝난 핸드의 내 카드를 모두에게 보여 줄 수 있다. */
  can_show?: boolean;
}

export interface TableRules {
  small_blind: number;
  big_blind: number;
  min_buy_in: number;
  max_buy_in: number;
  buy_in_step: number;
  min_bet: number;
  max_bet: number;
  bet_step: number;
  side_bet_min: number;
  side_bet_max: number;
  insurance_ms: number;
  seat_count: number;
  turn_ms: number;
  bet_window_ms: number;
  /** 카드가 슈에서 날아가는 시간 (ms). reveal_at은 카드가 놓이는 시각이다. */
  card_flight_ms?: number;
  /** 카드를 뒤집는 연출 시간 (ms). */
  card_flip_ms?: number;
  /** 홀덤 레이크 (만분율, 250 = 2.5%)와 핸드당 상한. 0이면 레이크 없음. */
  rake_bp?: number;
  rake_cap?: number;
}

export interface ChatMessage {
  id: string;
  seq: number;
  name: string;
  text: string;
  at: number;
  dealer: boolean;
  user_id: number | null;
  from_discord: boolean;
}

export interface Payout {
  name: string;
  amount: number;
  label: string;
}

export interface SeatResult {
  /** 이긴 베팅이 돌려준 총액 (원금 포함). 예전 기록은 0. */
  paid?: number;
  user_id: number;
  name: string;
  seat: number;
  wagered: number;
  net: number;
  won: number;
  label: string;
  notes: string[];
}

export interface HandResult {
  id: string;
  game: GameKind;
  at: number;
  summary: string;
  board: string[];
  payouts: Payout[];
  results: SeatResult[];
}

export interface DealerView {
  id: string;
  name: string;
  tagline: string;
}

export interface TableView {
  id: string;
  kind: GameKind;
  name: string;
  version: number;
  button: number;
  my_seat: number;
  narration: string;
  seats: (SeatView | null)[];
  round: RoundView | null;
  legal: LegalView;
  messages: ChatMessage[];
  history: HandResult[];
  rules: TableRules;
  pending_rules?: TableRules | null;
  dealer: DealerView;
  shoe: { remaining: number; total: number; cut_at: number; shuffled_at: number; reshuffle_due: boolean } | null;
}

export interface TableSummary {
  id: string;
  kind: GameKind;
  kind_text: string;
  name: string;
  seated: number;
  seat_count: number;
  playing: boolean;
  phase_text: string | null;
  channel_id: number | null;
  /** 판돈 ("블라인드 50/100", "베팅 100~5,000"). */
  stakes: string;
}

export interface CasinoMe {
  user_id: string;
  name: string;
  coins: number;
  seated_table: string | null;
}

export interface StateResponse {
  server_time: number;
  me: CasinoMe;
  table: TableView | null;
  tables: TableSummary[];
}

/** 바뀐 것이 없을 때 웹소켓이 1초마다 보내는 신호: 시계 맞추기와 연결 확인에만 쓴다. */
export interface Heartbeat {
  server_time: number;
}

export type CasinoCommand =
  | { action: "join"; seat: number; amount: number; name: string }
  | { action: "leave" }
  | { action: "start" }
  | { action: "resume" }
  | { action: "show" }
  | { action: "chat"; message: string }
  | { action: "fold" }
  | { action: "check" }
  | { action: "call" }
  | { action: "raise"; amount: number }
  | { action: "bet"; amount: number; pairs?: number; plus3?: number }
  | { action: "insure"; accept: boolean }
  | { action: "hit" }
  | { action: "stand" }
  | { action: "double" }
  | { action: "split" }
  | { action: "surrender" };

export interface ApiError {
  code: string;
  message: string;
}
