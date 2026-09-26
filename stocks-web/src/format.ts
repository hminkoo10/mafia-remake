// 증권 화면의 순수 계산: 호가 단위, 수수료·세금, 금액·비율·시간 표기. (node 테스트에서 바로 불러 쓴다)
// 호가 단위·수수료 계산은 봇의 src/stocks/price.rs, model.rs(StockRules::fee/tax)와 같아야 한다.

const numberFormat = new Intl.NumberFormat("en-US");

/** 12,345 */
export const won = (n: number) => numberFormat.format(Math.round(n));

/** +12,345 / −12,345 / 0 */
export const signedWon = (n: number) => (n > 0 ? `+${won(n)}` : n < 0 ? `−${won(-n)}` : "0");

/** 만분율 → +1.23% (0이면 0.00%). */
export function pctText(bp: number, withSign = true): string {
  const text = `${(Math.abs(bp) / 100).toFixed(2)}%`;
  if (!withSign || bp === 0) return text;
  return bp > 0 ? `+${text}` : `−${text}`;
}

/** 등락 색: 한국 시장처럼 오르면 빨강(up), 내리면 파랑(down). */
export const tone = (delta: number): "up" | "down" | "flat" => (delta > 0 ? "up" : delta < 0 ? "down" : "flat");

/** ▲ / ▼ / – */
export const arrow = (delta: number) => (delta > 0 ? "▲" : delta < 0 ? "▼" : "–");

/** 큰 금액 줄임: 1.2조, 3.4억, 5,600만. */
export function compactWon(n: number): string {
  const sign = n < 0 ? "−" : "";
  const abs = Math.abs(n);
  if (abs >= 1e13) return `${sign}${won(Math.round(abs / 1e12))}조`;
  if (abs >= 1e12) return `${sign}${(abs / 1e12).toFixed(1)}조`;
  if (abs >= 1e10) return `${sign}${won(Math.round(abs / 1e8))}억`;
  if (abs >= 1e8) return `${sign}${(abs / 1e8).toFixed(1)}억`;
  if (abs >= 1e4) return `${sign}${won(Math.floor(abs / 1e4))}만`;
  return `${sign}${won(abs)}`;
}

// ------------------------------------------------------------ 호가 단위

/** 한국거래소 호가 가격 단위 (2023년 개편 기준). */
export function tickSize(price: number): number {
  if (price < 2_000) return 1;
  if (price < 5_000) return 5;
  if (price < 20_000) return 10;
  if (price < 50_000) return 50;
  if (price < 200_000) return 100;
  if (price < 500_000) return 500;
  return 1_000;
}

/** 호가 단위에 맞게 내림 (최소 1). */
export function floorTick(price: number): number {
  const value = Math.max(1, Math.floor(price));
  const tick = tickSize(value);
  return Math.max(1, Math.floor(value / tick) * tick);
}

/** 호가 단위에 맞게 올림 (올린 값이 다음 구간으로 넘어가면 그 구간의 단위로 다시 맞춘다). */
export function ceilTick(price: number): number {
  const value = Math.max(1, Math.ceil(price));
  const down = floorTick(value);
  if (down === value) return value;
  return Math.max(floorTick(down + tickSize(down)), down + 1);
}

/** 한 호가 위. */
export function tickUp(price: number): number {
  const value = floorTick(price);
  return value + tickSize(value);
}

/** 한 호가 아래 (최소 1). */
export function tickDown(price: number): number {
  const value = floorTick(price);
  return value <= 1 ? 1 : floorTick(value - 1);
}

// ------------------------------------------------------------ 수수료·세금

export interface FeeRules {
  fee_ppm: number;
  tax_ppm: number;
}

/** 매매 수수료: 올림, 최소 1 (수수료율이 0이면 0). */
export function feeOf(notional: number, feePpm: number): number {
  if (feePpm <= 0 || notional <= 0) return 0;
  return Math.max(1, Math.ceil((notional * feePpm) / 1_000_000));
}

/** 증권거래세 (매도): 내림. */
export function taxOf(notional: number, taxPpm: number): number {
  if (taxPpm <= 0 || notional <= 0) return 0;
  return Math.floor((notional * taxPpm) / 1_000_000);
}

export interface OrderEstimate {
  notional: number;
  fee: number;
  tax: number;
  /** 매수: 낼 코인, 매도: 받을 코인. */
  total: number;
}

export function orderEstimate(side: "buy" | "sell", price: number, qty: number, rules: FeeRules): OrderEstimate {
  const notional = Math.max(0, price) * Math.max(0, qty);
  const fee = feeOf(notional, rules.fee_ppm);
  const tax = side === "sell" ? taxOf(notional, rules.tax_ppm) : 0;
  return { notional, fee, tax, total: side === "buy" ? notional + fee : notional - fee - tax };
}

/** `budget` 안에서 수수료까지 내고 살 수 있는 수량. */
export function affordableQty(budget: number, price: number, feePpm: number): number {
  if (price <= 0 || budget <= 0) return 0;
  const total = (qty: number) => qty * price + feeOf(qty * price, feePpm);
  let qty = Math.floor((budget * 1_000_000) / (1_000_000 + Math.max(0, feePpm)) / price);
  while (qty > 0 && total(qty) > budget) qty -= 1;
  return Math.max(0, qty);
}

// ------------------------------------------------------------ 시간

const pad = (n: number) => String(n).padStart(2, "0");

/** 14:05 */
export function timeText(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 9/26 14:05 */
export function dateTimeText(ms: number): string {
  const d = new Date(ms);
  return `${d.getMonth() + 1}/${d.getDate()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 남은 시간: 2일 3시간, 1시간 5분, 12분, 40초. */
export function durationText(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  if (days > 0) return hours > 0 ? `${days}일 ${hours}시간` : `${days}일`;
  if (hours > 0) return minutes > 0 ? `${hours}시간 ${minutes}분` : `${hours}시간`;
  if (minutes > 0) return `${minutes}분`;
  return `${seconds}초`;
}

/** 12분 뒤 / 3시간 전 */
export function relativeText(target: number, now: number): string {
  const diff = target - now;
  if (Math.abs(diff) < 1000) return "지금";
  return diff > 0 ? `${durationText(diff)} 뒤` : `${durationText(-diff)} 전`;
}

/** 게임일 마감(다음 게임일 시작)까지 남은 ms. 게임일 경계는 unix 시각을 하루 길이로 나눈 값이다. */
export function msToNextGameDay(now: number, dayMs: number): number {
  const day = Math.max(60_000, dayMs);
  return (Math.floor(now / day) + 1) * day - now;
}

/** 남은 시간을 초까지: 16분 05초 (1시간 넘으면 1시간 02분). 시각(16:05)과 헷갈리지 않게 단위를 붙인다. */
export function countdownText(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  if (h > 0) return `${h}시간 ${pad(m)}분`;
  return m > 0 ? `${m}분 ${pad(s)}초` : `${s}초`;
}

// ------------------------------------------------------------ 문구

export const RISK_TEXT = ["", "1 · 매우 안정", "2 · 안정", "3 · 보통", "4 · 공격적", "5 · 매우 공격적"];

export const NEWS_KIND_TEXT: Record<string, string> = {
  news: "뉴스",
  rumor: "루머",
  disclosure: "공시",
  earnings: "실적",
  dividend: "배당",
  listing: "상장",
  delisting: "상장폐지",
  halt: "거래정지",
  macro: "경제",
  market: "시황",
};

/** 수수료율 표기: 150ppm → 0.015% */
export const ppmText = (ppm: number) => `${(ppm / 10_000).toFixed(3).replace(/0+$/, "").replace(/\.$/, "")}%`;

/** 만분율 표기: 100bp → 1% */
export const bpText = (bp: number) => `${(bp / 100).toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}%`;

/** 입력칸의 숫자 읽기: 쉼표·공백을 빼고 정수로 (못 읽으면 0). */
export function parseAmount(text: string): number {
  const value = Number(text.replace(/[,\s]/g, ""));
  return Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0;
}
