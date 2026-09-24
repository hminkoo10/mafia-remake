// 카드 딜 일정의 순수 계산. 서버가 정한 시각(reveal_at = 카드가 테이블에 놓이는 시각)을
// 화면 시각으로 옮긴다: 카드는 놓이기 card_flight_ms 전에 슈를 떠나 날아간다. (node 테스트에서 바로 불러 쓴다)
import type { DealerMood } from "./dealer-media";
import type { RoundView, SeatView, TableRules, TableView } from "./types";

/** 서버가 비행 시간을 안 보내면(예전 서버) 쓰는 값. */
export const DEFAULT_FLIGHT_MS = 420;
export const DEFAULT_FLIP_MS = 600;
/** 남은 비행이 이보다 짧으면 날리지 않고 이미 놓인 채로 그린다. */
export const MIN_FLIGHT_MS = 120;
/** 뒤집기 조금 전부터 딜러가 카드를 여는 동작을 시작한다. */
export const FLIP_LEAD_MS = 300;
/** 새 슈를 섞는 연출 길이. */
export const SHUFFLE_MS = 2500;
export const DEAL_CLIP_MS = 917;
export const DEAL_RELEASE_MS = 500;

export interface CardTiming {
  /** 슈에서 테이블까지 날아가는 시간. 연출을 끄면 0 (카드가 놓이는 순간 나타난다). */
  flight: number;
  flip: number;
}

export function cardTiming(rules: Partial<Pick<TableRules, "card_flight_ms" | "card_flip_ms">> | null | undefined, motion: boolean): CardTiming {
  const flight = rules?.card_flight_ms ?? DEFAULT_FLIGHT_MS;
  const flip = rules?.card_flip_ms ?? DEFAULT_FLIP_MS;
  return {
    flight: motion && Number.isFinite(flight) ? Math.max(0, flight) : 0,
    flip: Number.isFinite(flip) ? Math.max(0, flip) : DEFAULT_FLIP_MS,
  };
}

const at = (times: readonly number[] | undefined, index: number) => times?.[index] ?? 0;

/** 슈를 떠난(화면에 있는) 카드 수. 카드는 순서대로 나가므로 앞에서부터 센다. */
export function inPlayCount(times: readonly number[] | undefined, count: number, now: number, flight: number): number {
  let n = 0;
  while (n < count && at(times, n) - flight <= now) n++;
  return n;
}

/** 테이블에 놓인 카드 수 (합계·족보·내 카드 칸은 놓인 카드만 센다). */
export function landedCount(times: readonly number[] | undefined, count: number, now: number): number {
  return inPlayCount(times, count, now, 0);
}

/** 뒤집기 전이면 true (flipAt이 0이면 뒤집을 일이 없다). */
export const faceDown = (flipAt: number | undefined, now: number) => !!flipAt && flipAt > 0 && now < flipAt;

/** 초 단위 남은 시간 (카운트다운 표시). */
export const secondsLeft = (deadline: number | undefined, now: number) =>
  deadline ? Math.max(0, Math.ceil((deadline - now) / 1000)) : 0;

type Seats = readonly (SeatView | null)[];

/** 이번 라운드에 놓이는 모든 카드(보드·딜러·번 카드·좌석)의 도착 시각. */
export function cardTimes(round: RoundView | null | undefined, seats: Seats): number[] {
  if (!round) return [];
  const out: number[] = [];
  const push = (times: readonly number[] | undefined, count: number) => {
    for (let i = 0; i < count; i++) out.push(at(times, i));
  };
  push(round.board_reveal_at, round.board.length);
  push(round.dealer_reveal_at, round.dealer.length);
  push(round.burn_at, round.burn_at?.length ?? 0);
  for (const seat of seats) {
    if (!seat) continue;
    push(seat.cards_reveal_at, seat.cards.length);
    for (const hand of seat.hands) push(hand.reveal_at, hand.cards.length);
  }
  return out;
}

export interface DealClip {
  key: string;
  startAt: number;
  rate: number;
}

interface DealCard {
  key: string;
  departure: number;
}

function dealCards(round: RoundView | null | undefined, seats: Seats, timing: CardTiming): DealCard[] {
  if (!round) return [];
  const out: DealCard[] = [];
  const push = (prefix: string, times: readonly number[] | undefined, count: number) => {
    for (let i = 0; i < count; i++) {
      const reveal = at(times, i);
      if (reveal > 0) out.push({ key: `${prefix}:${i}:${reveal}`, departure: reveal - timing.flight });
    }
  };
  push("board", round.board_reveal_at, round.board.length);
  push("dealer", round.dealer_reveal_at, round.dealer.length);
  push("burn", round.burn_at, round.burn_at?.length ?? 0);
  seats.forEach((seat, seatIndex) => {
    if (!seat) return;
    push(`seat:${seatIndex}`, seat.cards_reveal_at, seat.cards.length);
    seat.hands.forEach((hand, handIndex) => push(`hand:${seatIndex}:${handIndex}`, hand.reveal_at, hand.cards.length));
  });
  return out.sort((a, b) => a.departure - b.departure || a.key.localeCompare(b.key));
}

/** 현재 시각에 화면에 있어야 하는 딜러 deal 클립과 재생 속도. */
export function dealClipAt(round: RoundView | null | undefined, seats: Seats, timing: CardTiming, now: number): DealClip | null {
  const cards = dealCards(round, seats, timing);
  let current: DealClip | null = null;
  cards.forEach((card, index) => {
    const next = cards[index + 1];
    const gap = next ? next.departure - card.departure : 0;
    const rate = gap > 0 ? Math.min(1.8, Math.max(1, DEAL_CLIP_MS / gap)) : 1;
    const startAt = card.departure - DEAL_RELEASE_MS / rate;
    const endAt = startAt + DEAL_CLIP_MS / rate;
    if (startAt <= now && now < endAt) current = { key: card.key, startAt, rate };
  });
  return current;
}

/** 구독용 문자열 키 (useServerValue는 원시값만 받는다). */
export const dealClipKey = (clip: DealClip | null): string | null =>
  clip ? `${clip.startAt}|${clip.rate}|${clip.key}` : null;

/** dealClipKey의 반대. 잘못된 키면 null. */
export function parseDealClipKey(value: string | null): DealClip | null {
  if (!value) return null;
  const [startAt, rate, ...key] = value.split("|");
  const start = Number(startAt),
    speed = Number(rate);
  if (!Number.isFinite(start) || !Number.isFinite(speed) || key.length === 0) return null;
  return { key: key.join("|"), startAt: start, rate: speed };
}

function dealClipStarts(round: RoundView, seats: Seats, timing: CardTiming): number[] {
  const cards = dealCards(round, seats, timing);
  return cards.map((card, index) => {
    const gap = cards[index + 1] ? cards[index + 1].departure - card.departure : 0;
    const rate = gap > 0 ? Math.min(1.8, Math.max(1, DEAL_CLIP_MS / gap)) : 1;
    return card.departure - DEAL_RELEASE_MS / rate;
  });
}

/** 이번 라운드의 뒤집기 시각 (플롭, 딜러 홀 카드, 쇼다운). */
export function flipTimes(round: RoundView | null | undefined, seats: Seats): number[] {
  if (!round) return [];
  const out = [round.board_flip_at ?? 0, round.dealer_flip_at ?? 0];
  for (const seat of seats) if (seat && !seat.mine) out.push(seat.showdown_at ?? 0);
  return out.filter((time) => time > 0);
}

/**
 * 라운드 결과가 드러나기 시작하는 시각: 블랙잭은 딜러 홀 카드를 뒤집는 시각, 홀덤은 첫 쇼다운 공개.
 * 정산 전이거나 뒤집을 일이 없으면 0.
 */
export function showdownAt(round: RoundView | null | undefined, seats: Seats): number {
  if (!round) return 0;
  let first = round.dealer_flip_at ?? 0;
  for (const seat of seats) {
    const at = seat?.showdown_at ?? 0;
    if (at > 0 && (first <= 0 || at < first)) first = at;
  }
  return first > 0 ? first : 0;
}

/** 카드 연출 중 안내: 카드를 나누는 중(dealing)인지, 뒤집어 확인하는 중(showdown)인지. 연출이 끝났으면 null. */
export type RevealStage = "dealing" | "showdown" | null;

/**
 * 지금 화면에 띄울 연출 단계. 결과를 드러내는 말(SHOWDOWN, 라운드 종료)은 뒤집는 시각 전에는 쓰지 않는다:
 * 서버가 이미 정산한 라운드를 보내도(예전 서버) 뒤집기 전까지는 카드를 나누는 중으로 본다.
 */
export function revealStage(round: RoundView | null | undefined, seats: Seats, now: number): RevealStage {
  if (!round) return null;
  const flip = showdownAt(round, seats);
  if (flip > now) return "dealing";
  if (now >= round.reveal_until) return null;
  return flip > 0 ? "showdown" : "dealing";
}

/** 슈를 떠난 카드 수: 딜러 영상이 카드마다 동작을 이어 가게 한다. */
export function cardsLeftShoe(round: RoundView | null | undefined, seats: Seats, timing: CardTiming, now: number): number {
  return cardTimes(round, seats).filter((time) => time - timing.flight <= now).length;
}

/**
 * 딜러 영상의 동작: 카드를 뒤집는 순간 앞뒤로는 'flip', 아직 슈를 떠나지 않은 카드가 있으면 'deal', 아니면 'idle'.
 * 다음 일이 뒤집기라면(정산 직후 홀 카드, 쇼다운) 그 전에 딜 동작을 시작했다 끊지 않는다.
 */
export function dealerMoodAt(round: RoundView | null | undefined, seats: Seats, timing: CardTiming, now: number): DealerMood {
  if (!round) return "idle";
  const flips = flipTimes(round, seats);
  if (flips.some((time) => now >= time - FLIP_LEAD_MS && now < time + timing.flip)) return "flip";
  if (dealClipAt(round, seats, timing, now)) return "deal";
  const nextCard = Math.min(...cardTimes(round, seats).map((time) => time - timing.flight).filter((time) => time > now));
  if (!Number.isFinite(nextCard)) return "idle";
  const nextFlip = Math.min(...flips.map((time) => time - FLIP_LEAD_MS).filter((time) => time > now));
  return nextFlip <= nextCard ? "idle" : "deal";
}

/** 화면이 바뀌는 모든 예정 시각: 카드 출발·도착, 뒤집기 앞뒤, 연출 끝, 셔플 끝. 시계는 이 시각에만 깬다. */
export function tableEvents(table: TableView | null | undefined, timing: CardTiming): number[] {
  const out: number[] = [];
  const round = table?.round;
  if (round && table) {
    for (const time of cardTimes(round, table.seats)) {
      if (time <= 0) continue;
      out.push(time - timing.flight, time);
    }
    out.push(...dealClipStarts(round, table.seats, timing));
    for (const time of flipTimes(round, table.seats)) out.push(time - FLIP_LEAD_MS, time, time + timing.flip);
    for (const seat of table.seats) if (seat?.showdown_at && seat.showdown_at > 0) out.push(seat.showdown_at);
    const showdown = showdownAt(round, table.seats);
    if (showdown > 0) out.push(showdown);
    if (round.reveal_until > 0) out.push(round.reveal_until);
  }
  const shoe = table?.shoe;
  if (shoe && shoe.total > 0) out.push(shoe.shuffled_at + SHUFFLE_MS);
  return out;
}
