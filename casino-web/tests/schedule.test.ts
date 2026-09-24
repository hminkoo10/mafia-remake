import assert from "node:assert/strict";
import test from "node:test";
import {
  cardsLeftShoe,
  cardTimes,
  cardTiming,
  dealClipAt,
  dealClipRate,
  DEAL_CLIP_MS,
  RETURN_IDLE_RATE,
  RETURN_REST_MS,
  dealClipKey,
  dealerMoodAt,
  faceDown,
  FLIP_LEAD_MS,
  inPlayCount,
  landedCount,
  parseDealClipKey,
  revealStage,
  secondsLeft,
  showdownAt,
  SHUFFLE_MS,
  tableEvents,
} from "../src/schedule.ts";
import type { RoundView, SeatView, TableView } from "../src/types.ts";

const round = (patch: Partial<RoundView> = {}): RoundView => ({
  id: "r1",
  phase: "playing",
  phase_text: "",
  board: [],
  dealer: [],
  dealer_total: null,
  turn: 0,
  hand: 0,
  deadline: 0,
  current_bet: 0,
  pot: 0,
  reveal: false,
  reveal_until: 0,
  board_reveal_at: [],
  dealer_reveal_at: [],
  ...patch,
});

const seat = (patch: Partial<SeatView> = {}): SeatView => ({
  seat: 0,
  user_id: 1,
  name: "A",
  stack: 1000,
  mine: false,
  bet: 0,
  total: 0,
  folded: false,
  in_hand: true,
  sit_out: false,
  leaving: false,
  cards: [],
  cards_reveal_at: [],
  hands: [],
  side_pairs: 0,
  side_plus3: 0,
  insurance: 0,
  insurance_decided: false,
  side_notes: [],
  hand_name: null,
  hand_cards: [],
  ...patch,
});

// 블랙잭 1인 딜: 계약의 실제 값 (첫 착지 +650, 600ms 간격, 비행 420ms).
const T0 = 10_000;
const deal = round({
  dealer: ["Kd", "??"],
  dealer_reveal_at: [T0 + 1250, T0 + 2450],
  reveal_until: T0 + 2450 + 250,
});
const player = seat({ hands: [{ id: "h", cards: ["As", "9c"], reveal_at: [T0 + 650, T0 + 1850], bet: 100, total: 20, soft: true, status: "playing", result: null, payout: null }] });
const timing = cardTiming({ card_flight_ms: 420, card_flip_ms: 600 }, true);

test("older servers without timing fields fall back to the default flight and flip", () => {
  assert.deepEqual(cardTiming({}, true), { flight: 420, flip: 600 });
  assert.deepEqual(cardTiming(undefined, true), { flight: 420, flip: 600 });
  assert.deepEqual(cardTiming({ card_flight_ms: 400, card_flip_ms: 500 }, false), { flight: 0, flip: 500 }, "연출을 끄면 날지 않는다");
});

test("a card leaves the shoe one flight before it lands", () => {
  const times = [T0 + 650, T0 + 1250];
  assert.equal(inPlayCount(times, 2, T0 + 229, 420), 0);
  assert.equal(inPlayCount(times, 2, T0 + 230, 420), 1, "첫 카드가 슈를 떠난다");
  assert.equal(landedCount(times, 2, T0 + 649), 0, "아직 날아가는 중");
  assert.equal(landedCount(times, 2, T0 + 650), 1, "놓였다");
  assert.equal(inPlayCount(times, 2, T0 + 830, 420), 2);
  assert.equal(inPlayCount(times, 1, T0 + 5000, 420), 1, "카드 수를 넘지 않는다");
  assert.equal(inPlayCount(undefined, 2, 0, 420), 2, "시각이 없으면 이미 놓인 카드");
  assert.equal(inPlayCount([T0 + 5000, T0 + 100], 2, T0 + 1000, 0), 0, "앞 카드가 나가기 전에는 뒤 카드도 나가지 않는다");
});

test("with the contract spacing only one card is ever in the air", () => {
  const times = cardTimes(deal, [player]).sort((a, b) => a - b);
  for (let now = T0; now < T0 + 3000; now += 10) {
    const inAir = times.filter((time) => time - timing.flight <= now && now < time).length;
    assert.ok(inAir <= 1, `${now - T0}ms에 ${inAir}장이 날고 있다`);
  }
});

test("the dealer keeps dealing until the last card leaves the shoe, then idles", () => {
  assert.equal(dealerMoodAt(deal, [player], timing, T0), "deal", "첫 카드 전 슈에 손을 뻗는다");
  assert.equal(dealerMoodAt(deal, [player], timing, T0 + 1400), "deal");
  assert.equal(dealerMoodAt(deal, [player], timing, T0 + 2450 - 420), "deal", "마지막 카드의 클립은 비행 중 계속 재생한다");
  assert.equal(dealerMoodAt(null, [], timing, T0), "idle");
  assert.equal(cardsLeftShoe(deal, [player], timing, T0 + 230), 1);
  assert.equal(cardsLeftShoe(deal, [player], timing, T0 + 2030), 4);
});

test("deal clips pull the card as it leaves the shoe and place it as it lands", () => {
  const clip = dealClipAt(deal, [player], timing, T0 + 300);
  assert.ok(clip);
  // 꺼내기(0.50초)→내려놓기(1.167초) 구간이 비행 420ms와 같아지는 속도.
  assert.ok(Math.abs(clip.rate - (1167 - 500) / 420) < 0.01);
  const departure = T0 + 230;
  // 12프레임(꺼내기)이 출발 순간에, 28프레임(내려놓기)이 착지 순간에 온다.
  assert.equal(Math.round(clip.startAt + 500 / clip.rate), departure);
  assert.equal(Math.round(clip.startAt + 1167 / clip.rate), departure + timing.flight);
  assert.equal(dealClipAt(deal, [player], timing, T0 + 450)?.key, clip.key);
  // 모든 카드가 같은 속도: 따로 나가는 카드(히트 등)도 착지와 맞는다.
  const isolated = round({ board: ["As"], board_reveal_at: [T0 + 1000] });
  const solo = dealClipAt(isolated, [], timing, T0 + 600);
  assert.ok(solo);
  assert.equal(solo.rate, clip.rate);
  assert.equal(dealClipAt(isolated, [], timing, T0 + 700)?.key, solo.key);
  // 연출을 끄면(비행 0) 1배속.
  assert.equal(dealClipRate({ flight: 0, flip: 600 }), 1);
});

test("the dealer turns cards over around each flip time", () => {
  const settled = round({ ...deal, dealer: ["Kd", "7h"], dealer_flip_at: T0 + 5000, reveal: true, phase: "complete" });
  assert.equal(dealerMoodAt(settled, [player], timing, T0 + 5000 - FLIP_LEAD_MS - 1), "idle");
  assert.equal(dealerMoodAt(settled, [player], timing, T0 + 5000 - FLIP_LEAD_MS), "flip");
  assert.equal(dealerMoodAt(settled, [player], timing, T0 + 5599), "flip");
  assert.equal(dealerMoodAt(settled, [player], timing, T0 + 5600), "idle");
  const showdown = round({ phase: "complete", reveal: true });
  const rival = seat({ showdown_at: T0 + 800 });
  const me = seat({ mine: true, showdown_at: T0 + 9000 });
  assert.equal(dealerMoodAt(showdown, [rival, me], timing, T0 + 800), "flip");
  assert.equal(dealerMoodAt(showdown, [rival, me], timing, T0 + 9000), "idle", "내 카드는 이미 보인다");
});

test("after settlement the dealer turns the hole card over before drawing, without a cut-off deal gesture", () => {
  const flipAt = T0 + 5000;
  const settled = round({ ...deal, dealer: ["6c", "Td", "4s"], dealer_reveal_at: [T0 + 1250, T0 + 2450, flipAt + 1000], dealer_flip_at: flipAt, reveal: true, phase: "complete" });
  assert.equal(dealerMoodAt(settled, [player], timing, flipAt - 700), "idle", "다음 카드 클립 전에는 대기한다");
  assert.equal(dealerMoodAt(settled, [player], timing, flipAt - FLIP_LEAD_MS), "flip");
  // 뽑는 카드는 뒤집기 동작이 끝나 갈 때(+580) 슈를 떠난다: 동작을 끊지 않고 이어서 대기로 돌아간다.
  assert.equal(dealerMoodAt(settled, [player], timing, flipAt + 590), "flip");
  assert.equal(dealerMoodAt(settled, [player], timing, flipAt + 600), "deal");
  // 두 장 이상 뽑으면 두 번째 카드 전에는 다시 딜 동작이다.
  const twoDraws = round({ ...settled, dealer: ["6c", "5d", "2s", "9h"], dealer_reveal_at: [...settled.dealer_reveal_at, flipAt + 2000] });
  assert.equal(dealerMoodAt(twoDraws, [player], timing, flipAt + 700), "deal");
});

test("flop cards stay face down until the flip time", () => {
  assert.equal(faceDown(undefined, T0), false, "예전 서버");
  assert.equal(faceDown(0, T0), false);
  assert.equal(faceDown(T0 + 1, T0), true);
  assert.equal(faceDown(T0, T0), false);
});

test("the clock is told every moment the table changes", () => {
  const table = {
    id: "t",
    round: { ...deal, burn_at: [T0 + 300], board_flip_at: T0 + 4000 },
    seats: [player, null],
    shoe: { remaining: 400, total: 416, cut_at: 60, shuffled_at: T0, reshuffle_due: false },
  } as unknown as TableView;
  const events = tableEvents(table, timing);
  for (const expected of [
    T0 + 230, // 첫 카드 출발
    T0 + 650, // 첫 카드 도착
    T0 + 300 - 420, // 번 카드 출발
    T0 + 4000 - FLIP_LEAD_MS, // 뒤집기 동작 시작
    T0 + 4000, // 플롭이 뒤집힌다
    T0 + 4600, // 뒤집기 끝
    T0 + 2700, // 연출 끝 (reveal_until)
    T0 + SHUFFLE_MS, // 셔플 끝
  ]) {
    assert.ok(events.includes(expected), `${expected - T0} 빠짐`);
  }
  assert.deepEqual(tableEvents(null, timing), []);
});

test("table wake-ups include every seat showdown time, including mine", () => {
  const table = {
    id: "t",
    round: round({ phase: "complete", reveal: true, reveal_until: T0 + 10_000, dealer_flip_at: T0 + 5000 }),
    seats: [seat({ showdown_at: T0 + 800 }), seat({ mine: true, showdown_at: T0 + 900 })],
  } as unknown as TableView;
  const events = tableEvents(table, timing);
  assert.ok(events.includes(T0 + 800));
  assert.ok(events.includes(T0 + 900));
  assert.equal(showdownAt(table.round, table.seats), T0 + 800);
});

test("countdowns round up to whole seconds and stop at zero", () => {
  assert.equal(secondsLeft(0, 5000), 0);
  assert.equal(secondsLeft(15000, 5000), 10);
  assert.equal(secondsLeft(15000, 5001), 10);
  assert.equal(secondsLeft(15000, 14001), 1);
  assert.equal(secondsLeft(15000, 16000), 0);
});

test("the dock says dealing until the first flip and never announces the showdown early", () => {
  // 딜 중: 뒤집을 일이 없으니 연출이 끝날 때까지 "나누는 중".
  assert.equal(revealStage(deal, [player], T0), "dealing");
  assert.equal(revealStage(deal, [player], T0 + 2699), "dealing");
  assert.equal(revealStage(deal, [player], T0 + 2700), null);
  assert.equal(revealStage(null, [], T0), null);
  // 블랙잭 정산: 서버가 정산 전 단계를 보내든(새 서버) 완료를 보내든(예전 서버) 홀 카드를 뒤집기 전에는 "나누는 중".
  const flipAt = T0 + 5000;
  for (const phase of ["playing", "complete"] as const) {
    const settled = round({ ...deal, phase, dealer: ["6c", "Td"], dealer_flip_at: flipAt, reveal: true, reveal_until: flipAt + 1600 });
    assert.equal(showdownAt(settled, [player]), flipAt);
    assert.equal(revealStage(settled, [player], flipAt - 1), "dealing", phase);
    assert.equal(revealStage(settled, [player], flipAt), "showdown", phase);
    assert.equal(revealStage(settled, [player], flipAt + 1599), "showdown", phase);
    assert.equal(revealStage(settled, [player], flipAt + 1600), null, phase);
  }
  // 연출이 먼저 끝난 것처럼 보여도 뒤집기 전에는 결과를 말하지 않는다.
  const early = round({ phase: "complete", dealer_flip_at: flipAt, reveal: true, reveal_until: flipAt - 500 });
  assert.equal(revealStage(early, [], flipAt - 100), "dealing");
  assert.equal(revealStage(early, [], flipAt), null);
  // 홀덤 올인 런아웃: 보드를 나누는 동안은 "나누는 중", 첫 쇼다운 공개부터 "확인하는 중" (내 좌석 시각도 센다).
  const runout = round({ phase: "complete", reveal: true, reveal_until: T0 + 9000 });
  const first = seat({ mine: true, showdown_at: T0 + 7000 });
  const second = seat({ seat: 1, showdown_at: T0 + 7800 });
  const folded = seat({ seat: 2, folded: true });
  assert.equal(showdownAt(runout, [second, folded, first]), T0 + 7000);
  assert.equal(revealStage(runout, [second, folded, first], T0 + 6999), "dealing");
  assert.equal(revealStage(runout, [second, folded, first], T0 + 7000), "showdown");
  assert.equal(showdownAt(round(), [seat()]), 0, "정산 전에는 뒤집기 시각이 없다");
});

test("the deal clip is subscribed by a string key that round-trips", () => {
  const clip = { key: "hand:0:0:1234", startAt: 1_000.5, rate: 1.53, kind: "deal" as const };
  const key = dealClipKey(clip);
  assert.equal(typeof key, "string");
  // 같은 클립은 늘 같은 키 (구독 값이 바뀌지 않아 다시 그리지 않는다).
  assert.equal(dealClipKey({ ...clip }), key);
  assert.deepEqual(parseDealClipKey(key), clip);
  assert.equal(dealClipKey(null), null);
  assert.equal(parseDealClipKey(null), null);
  assert.equal(parseDealClipKey("garbage"), null);
});

test("after placing each card the dealer's hand returns to the shoe before the next card", () => {
  const rate = dealClipRate(timing);
  const departures = cardTimes(deal, [player]).map((time) => time - timing.flight).sort((a, b) => a - b);
  const starts = departures.map((time) => time - 500 / rate);
  const firstEnd = starts[0] + DEAL_CLIP_MS / rate;
  // 첫 카드를 내려놓은 직후는 되돌리기, 다음 카드의 딜 영상이 시작하면 딜.
  const back = dealClipAt(deal, [player], timing, firstEnd + 1);
  if (firstEnd < starts[1]) {
    assert.equal(back?.kind, "return");
    assert.equal(back?.startAt, firstEnd);
    // 다음 딜이 시작하기 전에 손이 슈에 닿는 속도 (범위 안에서).
    assert.ok(firstEnd + RETURN_REST_MS / back!.rate <= starts[1] + 1 || back!.rate === 2.5);
  }
  assert.equal(dealClipAt(deal, [player], timing, starts[1] + 1)?.kind, "deal");
  // 마지막 카드 뒤에는 자연스러운 속도로 되돌리고, 손이 슈에 닿으면 끝난다 (대기 영상으로).
  const lastEnd = starts[starts.length - 1] + DEAL_CLIP_MS / rate;
  const last = dealClipAt(deal, [player], timing, lastEnd + 1);
  assert.equal(last?.kind, "return");
  assert.equal(last?.rate, RETURN_IDLE_RATE);
  assert.equal(dealClipAt(deal, [player], timing, lastEnd + RETURN_REST_MS / RETURN_IDLE_RATE + 1), null);
  // 구독 키는 종류까지 담는다.
  assert.equal(parseDealClipKey(dealClipKey(last))?.kind, "return");
});
