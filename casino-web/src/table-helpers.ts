// 테이블 화면의 순수 계산: 칩 단위, 블랙잭 핸드 표기, 팟 비율 레이즈. (node 테스트에서 바로 불러 쓴다)
import type { HandView, TableRules } from "./types";

export const fmt = (n: number) => n.toLocaleString("en-US");
export const signed = (n: number) => `${n < 0 ? "−" : "+"}${fmt(Math.abs(n))}`;

/** 칩 단위 전체. 테이블 한도에 맞는 것 6개까지 트레이에 올린다. */
export const ALL_DENOMS = [100, 500, 1000, 2500, 5000, 10000, 25000, 50000, 100000, 250000, 500000, 1000000];

export function tableDenoms(rules: Pick<TableRules, "min_bet" | "max_bet" | "bet_step">): number[] {
  const usable = ALL_DENOMS.filter((value) => value <= Math.max(rules.max_bet, rules.bet_step) && value % rules.bet_step === 0);
  // 최소 베팅을 칩으로 정확히 맞출 수 있게, 최소 베팅을 나누어떨어지게 하는 가장 큰 칩부터 보여 준다.
  let start = 0;
  usable.forEach((value, index) => {
    if (value <= rules.min_bet && rules.min_bet % value === 0) start = index;
  });
  return usable.slice(start, start + 6);
}

export const chipLabel = (v: number) => (v >= 1_000_000 ? `${v / 1_000_000}M` : v >= 1000 ? `${v / 1000}K` : String(v));

/** 블랙잭 핸드 결과 색 (좌석 위 결과 표시). */
export const handTone = (result: string | null) =>
  !result ? "" : /블랙잭|승리/.test(result) ? "hand-won" : result === "푸시" ? "hand-push" : "hand-lost";

/** 좌석 위 핸드 결과. 이긴 핸드는 돌려받는 총액(원금 포함), 진 핸드는 잃은 금액. */
export const handResultText = (hand: Pick<HandView, "result" | "payout" | "bet">) => {
  const gain = (hand.payout ?? 0) - hand.bet;
  if (!hand.result) return "";
  if (hand.result.includes("블랙잭")) return `BLACKJACK ${fmt(hand.payout ?? 0)}`;
  if (hand.result === "승리") return `WIN ${fmt(hand.payout ?? 0)}`;
  if (hand.result === "푸시") return "PUSH";
  if (hand.result === "버스트") return `BUST ${signed(gain)}`;
  if (hand.result === "서렌더") return `SURRENDER ${signed(gain)}`;
  return `LOSE ${signed(gain)}`;
};

/** 핸드 점수: 아직 진행 중인 소프트 핸드는 "7/17"처럼 두 값을 보여 준다. */
export const handScore = (hand: Pick<HandView, "total" | "soft" | "status" | "result">, visible: number, single: boolean) =>
  hand.total > 21
    ? hand.result
      ? String(hand.total)
      : "BUST"
    : single && visible === 2 && hand.total === 21
      ? "BJ"
      : hand.soft && hand.status === "playing" && hand.total < 21
        ? `${hand.total - 10}/${hand.total}`
        : String(hand.total);

/** 팟 비율 레이즈의 총액: 콜한 뒤의 팟 × 비율을 현재 베팅 위에 얹고, 가능한 범위로 맞춘다. */
export function potRaiseTo(
  round: { current_bet: number; pot: number },
  legal: { to_call: number; min_raise_to: number; max_raise_to: number },
  fraction: number,
): number {
  const target = round.current_bet + Math.round((round.pot + legal.to_call) * fraction);
  return Math.max(Math.min(legal.min_raise_to, legal.max_raise_to), Math.min(legal.max_raise_to, target));
}
