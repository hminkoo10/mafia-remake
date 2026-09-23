import assert from "node:assert/strict";
import test from "node:test";
import { handResultText, handScore, handTone, potRaiseTo, tableDenoms } from "../src/table-helpers.ts";

test("the chip tray follows the table limits", () => {
  assert.deepEqual(tableDenoms({ min_bet: 100, max_bet: 5000, bet_step: 100 }), [100, 500, 1000, 2500, 5000]);
  assert.deepEqual(tableDenoms({ min_bet: 500, max_bet: 25000, bet_step: 100 }), [500, 1000, 2500, 5000, 10000, 25000]);
  assert.deepEqual(tableDenoms({ min_bet: 1000, max_bet: 50000, bet_step: 100 }), [1000, 2500, 5000, 10000, 25000, 50000]);
  // 최소 베팅을 칩으로 정확히 만들 수 있어야 한다 (700 = 500 + 100 + 100).
  assert.deepEqual(tableDenoms({ min_bet: 700, max_bet: 3000, bet_step: 100 }), [100, 500, 1000, 2500]);
  assert.deepEqual(tableDenoms({ min_bet: 2000, max_bet: 100000, bet_step: 100 }), [1000, 2500, 5000, 10000, 25000, 50000]);
});

test("blackjack hand scores read like a live table", () => {
  const hand = { total: 17, soft: true, status: "playing" as const, result: null };
  assert.equal(handScore(hand, 2, true), "7/17");
  assert.equal(handScore({ ...hand, status: "stand" as const }, 2, true), "17");
  assert.equal(handScore({ total: 21, soft: true, status: "stand" as const, result: null }, 2, true), "BJ");
  assert.equal(handScore({ total: 21, soft: true, status: "stand" as const, result: null }, 2, false), "21", "스플릿 뒤 21은 블랙잭이 아니다");
  assert.equal(handScore({ total: 24, soft: false, status: "bust" as const, result: null }, 3, true), "BUST");
  assert.equal(handScore({ total: 24, soft: false, status: "bust" as const, result: "버스트" }, 3, true), "24");
});

test("hand results show the win or loss of that hand", () => {
  // 이긴 핸드는 돌려받는 총액: 블랙잭 5,000은 2.5배인 12,500.
  assert.equal(handResultText({ result: "승리", payout: 2000, bet: 1000 }), "WIN 2,000");
  assert.equal(handResultText({ result: "블랙잭 3:2", payout: 12500, bet: 5000 }), "BLACKJACK 12,500");
  assert.equal(handResultText({ result: "버스트", payout: 0, bet: 500 }), "BUST −500");
  assert.equal(handResultText({ result: "푸시", payout: 500, bet: 500 }), "PUSH");
  assert.equal(handResultText({ result: "패배", payout: 0, bet: 500 }), "LOSE −500");
  assert.equal(handTone("블랙잭 3:2"), "hand-won");
  assert.equal(handTone("푸시"), "hand-push");
  assert.equal(handTone("버스트"), "hand-lost");
  assert.equal(handTone(null), "");
});

test("pot-sized raises count the call first and stay within the legal range", () => {
  const round = { current_bet: 100, pot: 250 };
  const legal = { to_call: 50, min_raise_to: 200, max_raise_to: 19_950 };
  assert.equal(potRaiseTo(round, legal, 0.5), 250);
  assert.equal(potRaiseTo(round, legal, 0.75), 325);
  assert.equal(potRaiseTo(round, legal, 1), 400);
  assert.equal(potRaiseTo(round, { ...legal, max_raise_to: 300 }, 1), 300, "스택보다 크면 올인 금액");
  assert.equal(potRaiseTo({ current_bet: 100, pot: 150 }, { to_call: 0, min_raise_to: 200, max_raise_to: 5000 }, 0.5), 200, "최소 레이즈보다 작으면 최소");
});
