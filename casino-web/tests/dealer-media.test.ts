import assert from "node:assert/strict";
import test from "node:test";
import { chooseDealerLayer, type DealerLayerState } from "../src/dealer-media.ts";

const all = { idle: true, deal: true, deal2: true, flip: true };
const state = (patch: Partial<DealerLayerState> = {}): DealerLayerState => ({
  current: null,
  progress: 0,
  ended: false,
  newCard: false,
  usable: all,
  ...patch,
});

test("a new deal or flip starts its gesture from the first frame", () => {
  assert.deepEqual(chooseDealerLayer("deal", state({ current: "idle" })), { layer: "deal", restart: true });
  assert.deepEqual(chooseDealerLayer("flip", state({ current: "deal", progress: 0.3 })), { layer: "flip", restart: true });
  assert.deepEqual(chooseDealerLayer("idle", state({ current: null })), { layer: "idle", restart: false });
});

test("the dealer's hands keep moving while cards are still leaving the shoe", () => {
  // 동작 중간에 온 카드는 지금 동작을 이어 간다.
  assert.deepEqual(chooseDealerLayer("deal", state({ current: "deal", progress: 0.4, newCard: true })), { layer: "deal", restart: false });
  // 동작이 70% 넘게 진행됐으면 다음 카드는 다른 딜 층에서 새로 시작해 겹쳐 바꾼다.
  assert.deepEqual(chooseDealerLayer("deal", state({ current: "deal", progress: 0.75, newCard: true })), { layer: "deal2", restart: true });
  assert.deepEqual(chooseDealerLayer("deal", state({ current: "deal2", progress: 0.9, newCard: true })), { layer: "deal", restart: true });
  // 동작이 끝났는데 아직 딜 중이면 멈춘 채 두지 않는다.
  assert.deepEqual(chooseDealerLayer("deal", state({ current: "deal", ended: true, progress: 1 })), { layer: "deal2", restart: true });
  // 두 번째 층이 없으면 같은 층을 처음부터.
  assert.deepEqual(chooseDealerLayer("deal", state({ current: "deal", ended: true, usable: { idle: true, deal: true } })), { layer: "deal", restart: true });
});

test("a finished gesture falls back to idle instead of freezing, and a playing one is not cut", () => {
  assert.deepEqual(chooseDealerLayer("idle", state({ current: "deal", progress: 0.5 })), { layer: "deal", restart: false });
  assert.deepEqual(chooseDealerLayer("idle", state({ current: "flip", progress: 0.5 })), { layer: "flip", restart: false });
  assert.deepEqual(chooseDealerLayer("idle", state({ current: "deal", ended: true })), { layer: "idle", restart: false });
  assert.deepEqual(chooseDealerLayer("flip", state({ current: "flip", ended: true })), { layer: "idle", restart: false });
  assert.deepEqual(chooseDealerLayer("idle", state({ current: "idle" })), { layer: "idle", restart: false });
});

test("without an idle clip a finished gesture returns to the still photo", () => {
  const noIdle = { deal: true, deal2: true, flip: true };
  assert.deepEqual(chooseDealerLayer("idle", state({ current: "deal", ended: true, usable: noIdle })), { layer: null, restart: false });
  assert.deepEqual(chooseDealerLayer("idle", state({ current: null, usable: noIdle })), { layer: null, restart: false });
  assert.deepEqual(chooseDealerLayer("deal", state({ current: null, usable: noIdle })), { layer: "deal", restart: true });
});

test("loading or missing clips keep a usable frame and never invent an unrelated move", () => {
  assert.deepEqual(chooseDealerLayer("flip", state({ current: "deal", usable: { deal: true } })), { layer: "deal", restart: false });
  assert.deepEqual(chooseDealerLayer("deal", state({ current: "deal", usable: { idle: true } })), { layer: "idle", restart: false }, "딜 영상이 실패");
  assert.deepEqual(chooseDealerLayer("idle", state({ current: null, usable: { flip: true } })), { layer: null, restart: false });
  assert.deepEqual(chooseDealerLayer("deal", state({ current: null, usable: {} })), { layer: null, restart: false });
});

test("a long showdown keeps the dealer turning cards while flips are still coming", () => {
  const both = { idle: true, deal: true, deal2: true, flip: true, flip2: true };
  // 4명 쇼다운: 뒤집기 동작이 끝났는데 아직 뒤집을 카드가 있으면 다른 뒤집기 층에서 처음부터.
  assert.deepEqual(chooseDealerLayer("flip", state({ current: "flip", ended: true, flipsAhead: true, usable: both })), { layer: "flip2", restart: true });
  assert.deepEqual(chooseDealerLayer("flip", state({ current: "flip2", ended: true, flipsAhead: true, usable: both })), { layer: "flip", restart: true });
  // 두 번째 층이 없으면 같은 층을 처음부터.
  assert.deepEqual(chooseDealerLayer("flip", state({ current: "flip", ended: true, flipsAhead: true })), { layer: "flip", restart: true });
  // 하는 중인 동작은 끊지 않는다.
  assert.deepEqual(chooseDealerLayer("flip", state({ current: "flip2", progress: 0.5, flipsAhead: true, usable: both })), { layer: "flip2", restart: false });
  assert.deepEqual(chooseDealerLayer("idle", state({ current: "flip2", progress: 0.5, usable: both })), { layer: "flip2", restart: false });
  // 마지막 카드까지 뒤집었으면 대기로 돌아간다.
  assert.deepEqual(chooseDealerLayer("flip", state({ current: "flip2", ended: true, usable: both })), { layer: "idle", restart: false });
  // 새 쇼다운은 첫 뒤집기 층부터 (없으면 두 번째 층).
  assert.deepEqual(chooseDealerLayer("flip", state({ current: "deal", progress: 0.4, usable: both })), { layer: "flip", restart: true });
  assert.deepEqual(chooseDealerLayer("flip", state({ current: "idle", usable: { idle: true, flip2: true } })), { layer: "flip2", restart: true });
});
