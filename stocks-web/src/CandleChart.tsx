import { useEffect, useRef, useState } from "react";
import type { Candle, CandleRange } from "./types.ts";
import { formatPrice, formatTime, movingAverage, niceTicks, priceRange, visibleWindow } from "./chart-math.ts";

const AXIS = 64;
const TIME_AXIS = 20;
// 밝은 화면용 색 (한국 시장처럼 오르면 빨강, 내리면 파랑).
const TEXT = "#8b95a1";
const UP = "#f04452";
const DOWN = "#3182f6";
const GRID = "rgba(25,31,40,0.06)";
const INK = "#191f28";
/** 봉이 적을 때 한 봉이 차지하는 최대 폭 (나머지는 비우고 최근 봉을 오른쪽에 붙인다). */
const MAX_SLOT = 14;

type Pointer = { x: number; y: number } | null;

export function CandleChart({ candles, range, scale = 1, decimals = 0, prevClose, height = 320 }: {
  candles: Candle[];
  range: CandleRange;
  scale?: number;
  decimals?: number;
  prevClose?: number;
  height?: number;
}): JSX.Element {
  const wrapperRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const pointerRef = useRef<Pointer>(null);
  const frameRef = useRef<number | null>(null);
  const [width, setWidth] = useState(0);
  const [pointer, setPointer] = useState<Pointer>(null);

  useEffect(() => {
    const wrapper = wrapperRef.current;
    if (!wrapper) return;
    const resize = () => setWidth(wrapper.clientWidth);
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(wrapper);
    return () => observer.disconnect();
  }, []);

  useEffect(() => () => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
  }, []);

  const schedulePointer = (next: Pointer) => {
    pointerRef.current = next;
    if (frameRef.current !== null) return;
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      setPointer(pointerRef.current);
    });
  };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || width <= 0) return;
    const dpr = window.devicePixelRatio || 1;
    const cssHeight = Math.max(1, height);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(cssHeight * dpr);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${cssHeight}px`;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, width, cssHeight);
    ctx.font = '11px Arial, "Malgun Gothic", sans-serif';
    ctx.textBaseline = "middle";

    if (candles.length === 0) {
      ctx.fillStyle = TEXT;
      ctx.textAlign = "center";
      ctx.fillText("아직 거래 기록이 없습니다", width / 2, cssHeight / 2);
      return;
    }

    const safeScale = scale || 1;
    const display = candles.map((candle) => ({ ...candle, o: candle.o / safeScale, h: candle.h / safeScale, l: candle.l / safeScale, c: candle.c / safeScale }));
    const displayPrev = prevClose === undefined ? undefined : prevClose / safeScale;
    const plotWidth = Math.max(1, width - AXIS);
    const priceHeight = Math.max(1, cssHeight - TIME_AXIS);
    const volumeHeight = priceHeight * 0.24;
    const candleHeight = priceHeight - volumeHeight - 1;
    const { start, slot: fitSlot } = visibleWindow(display.length, plotWidth, 4);
    const slot = Math.min(MAX_SLOT, fitSlot);
    // 가격 범위는 화면에 보이는 봉으로만 잡는다.
    const prices = priceRange(display.slice(start), displayPrev === undefined ? [] : [displayPrev]);
    const ticks = niceTicks(prices.min, prices.max, 5);
    const yPrice = (value: number) => candleHeight - ((value - prices.min) / (prices.max - prices.min)) * candleHeight;
    const xCenter = (index: number) => plotWidth - (display.length - index - 0.5) * slot;

    ctx.strokeStyle = GRID;
    ctx.lineWidth = 1;
    ctx.textAlign = "right";
    // 현재가 표시와 겹치는 눈금 글자는 쓰지 않는다.
    const lastY = yPrice(display[display.length - 1].c);
    for (const tick of ticks) {
      const y = yPrice(tick) + 0.5;
      if (y < 0 || y > candleHeight) continue;
      ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(plotWidth, y); ctx.stroke();
      if (Math.abs(y - lastY) < 15 || y < 7 || y > candleHeight - 7) continue;
      ctx.fillStyle = TEXT; ctx.fillText(formatPrice(tick, decimals), width - 5, y);
    }
    ctx.beginPath(); ctx.moveTo(0, candleHeight + 0.5); ctx.lineTo(plotWidth, candleHeight + 0.5); ctx.stroke();

    const maxVolume = Math.max(...display.slice(start).map((candle) => candle.v), 1);
    for (let index = start; index < display.length; index += 1) {
      const candle = display[index];
      const x = xCenter(index);
      const color = candle.c >= candle.o ? UP : DOWN;
      const wickTop = yPrice(candle.h);
      const wickBottom = yPrice(candle.l);
      const bodyTop = Math.min(yPrice(candle.o), yPrice(candle.c));
      const bodyBottom = Math.max(yPrice(candle.o), yPrice(candle.c));
      const bodyWidth = Math.max(1, slot * 0.7);
      ctx.strokeStyle = color; ctx.lineWidth = Math.max(1, Math.min(2, slot * 0.15));
      ctx.beginPath(); ctx.moveTo(x, wickTop); ctx.lineTo(x, wickBottom); ctx.stroke();
      ctx.fillStyle = color;
      ctx.fillRect(x - bodyWidth / 2, bodyTop, bodyWidth, Math.max(1, bodyBottom - bodyTop));
      ctx.globalAlpha = 0.4;
      ctx.fillRect(x - bodyWidth / 2, candleHeight + 1 + (volumeHeight - (candle.v / maxVolume) * volumeHeight), bodyWidth, (candle.v / maxVolume) * volumeHeight);
      ctx.globalAlpha = 1;
    }

    const ma5 = movingAverage(display.map((candle) => candle.c), 5);
    const ma20 = movingAverage(display.map((candle) => candle.c), 20);
    const drawMa = (values: (number | null)[], color: string) => {
      ctx.strokeStyle = color; ctx.lineWidth = 1.25; ctx.beginPath();
      let drawing = false;
      for (let index = start; index < values.length; index += 1) {
        if (values[index] === null) { drawing = false; continue; }
        const x = xCenter(index); const y = yPrice(values[index] as number);
        if (!drawing) ctx.moveTo(x, y); else ctx.lineTo(x, y);
        drawing = true;
      }
      ctx.stroke();
    };
    drawMa(ma5, "#f59f00"); drawMa(ma20, "#7048e8");

    const hoveredIndex = pointer && pointer.x < plotWidth
      ? Math.min(display.length - 1, Math.max(start, display.length - 1 - Math.floor((plotWidth - pointer.x) / slot)))
      : display.length - 1;
    const hoveredMa5 = ma5[hoveredIndex]; const hoveredMa20 = ma20[hoveredIndex];
    ctx.fillStyle = TEXT; ctx.textAlign = "left";
    ctx.fillText(`MA5 ${hoveredMa5 === null ? "-" : formatPrice(hoveredMa5, decimals)}  MA20 ${hoveredMa20 === null ? "-" : formatPrice(hoveredMa20, decimals)}`, 6, 10);

    const last = display[display.length - 1];
    const drawDashed = (value: number, color: string) => { ctx.strokeStyle = color; ctx.setLineDash([4, 4]); ctx.beginPath(); ctx.moveTo(0, yPrice(value)); ctx.lineTo(plotWidth, yPrice(value)); ctx.stroke(); ctx.setLineDash([]); };
    if (displayPrev !== undefined) drawDashed(displayPrev, "rgba(139,149,161,0.6)");
    drawDashed(last.c, "rgba(25,31,40,0.35)");
    const lastColor = displayPrev === undefined ? (last.c >= last.o ? UP : DOWN) : (last.c >= displayPrev ? UP : DOWN);
    ctx.fillStyle = lastColor; ctx.fillRect(plotWidth, Math.max(0, Math.min(candleHeight - 16, yPrice(last.c) - 8)), AXIS, 16);
    ctx.fillStyle = "white"; ctx.textAlign = "left"; ctx.fillText(formatPrice(last.c, decimals), plotWidth + 4, Math.max(8, Math.min(candleHeight - 8, yPrice(last.c))));

    // 시간 글자가 겹치지 않게 폭에 맞춰 개수를 정하고, 양 끝에서 잘리지 않게 안쪽으로 붙인다.
    const labelWidth = range === "minute" ? 44 : 84;
    const firstX = Math.max(0, xCenter(start));
    const labels = Math.max(1, Math.min(6, display.length - start, Math.floor((plotWidth - firstX) / (labelWidth + 16))));
    ctx.fillStyle = TEXT; ctx.textAlign = "center";
    for (let label = 0; label < labels; label += 1) {
      const index = labels === 1 ? display.length - 1 : start + Math.round(label * (display.length - start - 1) / (labels - 1));
      const x = Math.max(labelWidth / 2, Math.min(plotWidth - labelWidth / 2, xCenter(index)));
      ctx.fillText(formatTime(display[index].t, range), x, cssHeight - TIME_AXIS / 2);
    }

    if (pointer && pointer.x < plotWidth && pointer.y < candleHeight) {
      const index = hoveredIndex; const x = xCenter(index); const candle = display[index];
      const pointerY = Math.max(0, Math.min(candleHeight, pointer.y));
      ctx.strokeStyle = "rgba(25,31,40,0.35)"; ctx.setLineDash([3, 3]); ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, candleHeight); ctx.moveTo(0, pointerY); ctx.lineTo(plotWidth, pointerY); ctx.stroke(); ctx.setLineDash([]);
      ctx.fillStyle = "#4e5968"; ctx.fillRect(plotWidth, pointerY - 8, AXIS, 16); ctx.fillStyle = "white"; ctx.textAlign = "left"; ctx.fillText(formatPrice(prices.min + (candleHeight - pointerY) / candleHeight * (prices.max - prices.min), decimals), plotWidth + 4, pointerY);
      const previous = index > 0 ? display[index - 1].c : null;
      const change = previous && previous !== 0 ? ((candle.c - previous) / previous) * 100 : null;
      const lines = [formatTime(candle.t, range), `시가 ${formatPrice(candle.o, decimals)}  고가 ${formatPrice(candle.h, decimals)}`, `저가 ${formatPrice(candle.l, decimals)}  종가 ${formatPrice(candle.c, decimals)}`, `거래량 ${formatPrice(candle.v, 0)}${change === null ? "" : `  ${change >= 0 ? "+" : ""}${change.toFixed(2)}%`}`];
      const boxWidth = 174; const boxHeight = lines.length * 16 + 10; const boxX = x > plotWidth - boxWidth - 8 ? Math.max(4, x - boxWidth - 8) : Math.min(plotWidth - boxWidth - 4, x + 8); const boxY = Math.max(4, Math.min(candleHeight - boxHeight - 4, pointerY - boxHeight - 8));
      ctx.fillStyle = "rgba(255,255,255,0.97)"; ctx.fillRect(boxX, boxY, boxWidth, boxHeight); ctx.strokeStyle = "#e5e8eb"; ctx.lineWidth = 1; ctx.strokeRect(boxX + 0.5, boxY + 0.5, boxWidth - 1, boxHeight - 1); ctx.fillStyle = INK; ctx.textAlign = "left";
      lines.forEach((line, lineIndex) => ctx.fillText(line, boxX + 6, boxY + 9 + lineIndex * 16));
    }
  }, [candles, range, scale, decimals, prevClose, height, width, pointer]);

  const onPointerMove = (event: React.PointerEvent<HTMLCanvasElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    schedulePointer({ x: event.clientX - rect.left, y: event.clientY - rect.top });
  };
  return <div ref={wrapperRef} style={{ width: "100%", position: "relative", height }}>
    <canvas ref={canvasRef} style={{ display: "block", touchAction: "pan-y" }} onPointerMove={onPointerMove} onPointerLeave={() => schedulePointer(null)} onPointerUp={() => schedulePointer(null)} onPointerCancel={() => schedulePointer(null)} />
  </div>;
}
