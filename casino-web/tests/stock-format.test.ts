import assert from "node:assert/strict";
import test from "node:test";
import {
  affordableQty,
  ceilTick,
  compactWon,
  feeOf,
  floorTick,
  msToNextGameDay,
  orderEstimate,
  parseAmount,
  pctText,
  ppmText,
  taxOf,
  tickDown,
  tickSize,
  tickUp,
} from "../src/stocks/format.ts";

test("tick sizes follow the KRX table like the bot", () => {
  assert.equal(tickSize(1_999), 1);
  assert.equal(tickSize(2_000), 5);
  assert.equal(tickSize(19_990), 10);
  assert.equal(tickSize(20_000), 50);
  assert.equal(tickSize(199_900), 100);
  assert.equal(tickSize(200_000), 500);
  assert.equal(tickSize(500_000), 1_000);
  assert.equal(floorTick(12_347), 12_340);
  assert.equal(floorTick(0), 1);
  assert.equal(ceilTick(4_032), 4_035);
  assert.equal(ceilTick(19_991), 20_000);
  assert.equal(ceilTick(5_000), 5_000);
  // 구간 경계를 넘을 때는 새 구간의 단위를 쓴다 (봇의 tick_up / tick_down).
  assert.equal(tickUp(1_999), 2_000);
  assert.equal(tickUp(4_995), 5_000);
  assert.equal(tickDown(2_000), 1_999);
  assert.equal(tickDown(5_000), 4_995);
  assert.equal(tickDown(1), 1);
});

test("fees round up to at least 1 and the sell tax rounds down, like StockRules", () => {
  assert.equal(feeOf(0, 150), 0);
  assert.equal(feeOf(100, 150), 1, "0.015원이어도 최소 1");
  assert.equal(feeOf(1_000_000, 150), 150);
  assert.equal(feeOf(1_000_001, 150), 151, "올림");
  assert.equal(feeOf(1_000_000, 0), 0);
  assert.equal(taxOf(1_000_000, 2_000), 2_000);
  assert.equal(taxOf(999, 2_000), 1, "내림");
  const rules = { fee_ppm: 150, tax_ppm: 2_000 };
  assert.deepEqual(orderEstimate("buy", 10_000, 10, rules), { notional: 100_000, fee: 15, tax: 0, total: 100_015 });
  assert.deepEqual(orderEstimate("sell", 10_000, 10, rules), { notional: 100_000, fee: 15, tax: 200, total: 99_785 });
});

test("the max buy quantity leaves room for the fee", () => {
  assert.equal(affordableQty(100_015, 10_000, 150), 10);
  assert.equal(affordableQty(100_014, 10_000, 150), 9);
  assert.equal(affordableQty(0, 10_000, 150), 0);
  assert.equal(affordableQty(5_000, 0, 150), 0);
});

test("amounts and rates read like a Korean brokerage", () => {
  assert.equal(pctText(123), "+1.23%");
  assert.equal(pctText(-5), "−0.05%");
  assert.equal(pctText(0), "0.00%");
  assert.equal(compactWon(123_456_789), "1.2억");
  assert.equal(compactWon(56_780_000), "5,678만");
  assert.equal(compactWon(3_200_000_000_000), "3.2조");
  assert.equal(compactWon(335_512_345_678), "3,355억", "큰 억 단위도 쉼표를 붙인다");
  assert.equal(compactWon(9_999), "9,999");
  assert.equal(ppmText(150), "0.015%");
  assert.equal(ppmText(2_000), "0.2%");
  assert.equal(parseAmount("1,234,567"), 1_234_567);
  assert.equal(parseAmount("abc"), 0);
});

test("the game day closes on the next multiple of its length", () => {
  const hour = 3_600_000;
  assert.equal(msToNextGameDay(hour * 10 + 1_000, hour), hour - 1_000);
  assert.equal(msToNextGameDay(hour * 10, hour), hour);
});
