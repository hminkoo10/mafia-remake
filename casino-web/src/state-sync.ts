import type { StateResponse } from "./types";

// 버전이 같아도 채팅이나 시간차 공개 카드가 달라질 수 있다.
export const snapshotKey = (state: StateResponse) => JSON.stringify([state.me, state.table, state.tables]);

export const isStaleSnapshot = (current: StateResponse | null, incoming: StateResponse) =>
  current?.table?.id === incoming.table?.id && !!current && incoming.server_time < current.server_time;

// 연출 중에만 빠르게 갱신하고, 턴 중에는 초 경계에 맞춘다. 대기 중에는 타이머가 필요 없다.
export function nextClockDelay(now: number, deadline: number, revealUntil: number, shuffleUntil: number): number | null {
  const animationEnd = Math.max(revealUntil, shuffleUntil);
  if (animationEnd > now) return Math.min(100, animationEnd - now);
  if (deadline > now) return Math.max(16, (deadline - now) % 1000 || 1000);
  return null;
}
