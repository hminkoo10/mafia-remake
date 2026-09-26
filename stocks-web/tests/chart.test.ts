import assert from "node:assert/strict";
import test from "node:test";
import { niceTicks, movingAverage, visibleWindow, priceRange, formatPrice } from "../src/chart-math.ts";

test("nice ticks are round and inside the range", () => {
  const ticks = niceTicks(103, 987, 5);
  assert.ok(ticks.length <= 5);
  assert.ok(ticks.every((tick) => tick >= 103 && tick <= 987));
  assert.ok(ticks.every((tick) => tick % 100 === 0 || tick % 50 === 0 || tick % 25 === 0));
});

test("moving averages stay null until enough values exist", () => {
  assert.deepEqual(movingAverage([2, 4, 6, 8], 3), [null, null, 4, 6]);
});

test("visible window keeps the newest candles", () => {
  assert.deepEqual(visibleWindow(20, 40, 4), { start: 10, slot: 4 });
});

test("flat prices get a non-zero range", () => {
  const result = priceRange([{ t: 0, o: 100, h: 100, l: 100, c: 100, v: 1 }]);
  assert.ok(result.min < 100);
  assert.ok(result.max > 100);
});

test("formatPrice respects decimals and thousands separators", () => {
  assert.equal(formatPrice(12345.678, 2), "12,345.68");
  assert.equal(formatPrice(-1000.5, 1), "-1,000.5");
});

test("the price range pads by the visible span, not the price level", () => {
  const result = priceRange([
    { t: 0, o: 50_000, h: 50_500, l: 49_500, c: 50_200, v: 1 },
    { t: 60_000, o: 50_200, h: 50_400, l: 50_000, c: 50_100, v: 1 },
  ]);
  assert.equal(result.min, 49_500 - 80);
  assert.equal(result.max, 50_500 + 80);
});
