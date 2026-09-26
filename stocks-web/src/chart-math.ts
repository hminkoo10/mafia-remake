import type { Candle, CandleRange } from "./types.ts";

const NICE_STEPS = [1, 2, 2.5, 5];

export function niceTicks(min: number, max: number, maxCount: number): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max) || maxCount < 1) return [];
  if (min > max) [min, max] = [max, min];
  if (min === max) return [min];

  const target = Math.max(1, maxCount - 1);
  const rawStep = (max - min) / target;
  const power = 10 ** Math.floor(Math.log10(rawStep));
  let step = (NICE_STEPS.find((candidate) => candidate * power >= rawStep) ?? 10) * power;
  let ticks: number[] = [];
  for (;;) {
    const first = Math.ceil((min - Number.EPSILON * Math.max(1, Math.abs(min))) / step) * step;
    ticks = [];
    for (let value = first; value <= max + step * 1e-9; value += step) {
      ticks.push(Number(value.toPrecision(12)));
    }
    if (ticks.length <= maxCount || step >= 10 * power) break;
    step *= 2;
  }
  return ticks;
}

export function movingAverage(values: number[], period: number): (number | null)[] {
  if (!Number.isInteger(period) || period <= 0) return values.map(() => null);
  let sum = 0;
  return values.map((value, index) => {
    sum += value;
    if (index >= period) sum -= values[index - period];
    return index + 1 >= period ? sum / period : null;
  });
}

export function visibleWindow(count: number, width: number, minSlot: number): { start: number; slot: number } {
  if (count <= 0 || width <= 0 || minSlot <= 0) return { start: 0, slot: 0 };
  const visible = Math.min(count, Math.max(1, Math.floor(width / minSlot)));
  return { start: count - visible, slot: width / visible };
}

export function priceRange(candles: Candle[], extra: number[] = []): { min: number; max: number } {
  const values = candles.flatMap((candle) => [candle.h, candle.l]).concat(extra.filter(Number.isFinite));
  if (values.length === 0) return { min: -1, max: 1 };
  const low = Math.min(...values);
  const high = Math.max(...values);
  // 위아래로 폭의 8%를 띄운다. 가격이 한 값뿐이면 1%(최소 1)를 띄워 0으로 나누지 않게 한다.
  const padding = high > low ? (high - low) * 0.08 : Math.max(Math.abs(high) * 0.01, 1);
  return { min: low - padding, max: high + padding };
}

export function formatPrice(value: number, decimals: number): string {
  const digits = Math.max(0, Math.min(20, Math.trunc(decimals)));
  return value.toLocaleString("en-US", { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

export function formatTime(t: number, range: CandleRange): string {
  const date = new Date(t);
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  return range === "minute" ? `${hh}:${mm}` : `${date.getMonth() + 1}/${date.getDate()} ${hh}:${mm}`;
}
