// 서버 시계: React 밖의 작은 저장소. 루트 컴포넌트가 시계 때문에 다시 그려지지 않도록,
// 시간에 따라 바뀌는 칸(카드, 카운트다운, 딜러 동작)만 useServerValue로 구독한다.
// 시계는 폴링하지 않고 다음 예정 사건(카드 출발·도착, 뒤집기, 마감 초 경계)에 맞춰 한 번씩 깬다.
import { useSyncExternalStore } from "react";
import { ConfirmedTime, deviceNow, OffsetEstimator, nextTickDelay } from "./state-sync";

type Listener = () => void;

const listeners = new Set<Listener>();
const estimator = new OffsetEstimator();
/** 마지막 틱의 서버 시각. 틱 사이에는 바뀌지 않아 구독한 값이 흔들리지 않는다. */
let snapshot = deviceNow();
/** 서버 시각이 거꾸로 가지 않게 묶는 단위 (테이블·라운드). 바뀌면 다시 맞춘다. */
let epoch = "";
let events: readonly number[] = [];
let deadline = 0;
let timer: ReturnType<typeof setTimeout> | undefined;

const confirmedTime = new ConfirmedTime();

// 서버 시각 = 단조 기기 시각 + 시계 차이. 기기 시계를 바꿔도 거꾸로 가지 않는다.
const live = () => deviceNow() + estimator.value;

function emit() {
  listeners.forEach((listener) => listener());
}

function schedule() {
  if (timer !== undefined) clearTimeout(timer);
  timer = undefined;
  const delay = nextTickDelay(Math.max(snapshot, live()), events, deadline);
  if (delay !== null) timer = setTimeout(tick, delay);
}

/** 틱 시각을 지금으로 옮긴다 (라운드 안에서는 거꾸로 가지 않는다). 바뀌었으면 true. */
function advance(): boolean {
  const next = Math.max(snapshot, live());
  if (next === snapshot) return false;
  snapshot = next;
  return true;
}

function tick() {
  timer = undefined;
  if (advance()) emit();
  schedule();
}

/** 틱 사이에 쓰는 서버 시각 (구독한 값과 같은 시각). */
export const serverNow = () => snapshot;
/** 지금 이 순간의 서버 시각: 카드가 날아갈 남은 시간을 잴 때 쓴다. */
export const liveServerNow = () => Math.max(snapshot, live());
export const confirmedServerTime = () => confirmedTime.value;

export function confirmServerState(serverTime: number) {
  if (confirmedTime.confirm(serverTime)) emit();
}

/**
 * 받은 메시지의 server_time으로 시계 차이를 잰다. receivedAt은 JSON을 풀기 전에 deviceNow()로 잰 받은 시각.
 * 추정값이 바뀌면 바로 틱한다. 추정값이 내려가도(지연이 길어져도) 라운드 안에서는 서버 시각이 거꾸로 가지 않는다.
 */
export function sampleServerTime(serverTime: number, receivedAt: number) {
  const first = !estimator.ready;
  if (!estimator.add(serverTime - receivedAt)) return;
  // 첫 표본은 페이지를 연 뒤 처음 맞추는 것이라 묶어 둔 시각도 새로 잡는다.
  if (first) {
    snapshot = live();
    emit();
  }
  tick();
}

/** 새 상태를 그리기 직전에 틱 시각만 조용히 지금으로 옮긴다 (그 렌더가 최신 시각을 쓰도록). */
export function touchServerClock() {
  advance();
}

/** 새 상태의 예정 사건과 마감을 알려 준다. 라운드가 바뀌면 거꾸로 가지 않게 하는 기준도 새로 잡는다. */
export function planServerClock(nextEvents: readonly number[], nextDeadline: number, nextEpoch: string) {
  events = nextEvents;
  deadline = nextDeadline;
  if (nextEpoch !== epoch) {
    epoch = nextEpoch;
    snapshot = live();
    emit();
  }
  tick();
}

function subscribe(listener: Listener) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * 서버 시각에서 뽑은 값(숫자·문자·불리언)을 구독한다. 값이 바뀔 때만 다시 그린다.
 * 객체는 넘기지 않는다: 부를 때마다 새 객체면 React가 매번 바뀐 것으로 보고 끝없이 다시 그린다.
 */
export function useServerValue<T extends string | number | boolean | null>(select: (now: number, confirmedAt: number) => T): T {
  return useSyncExternalStore(subscribe, () => select(snapshot, confirmedTime.value));
}

if (typeof document !== "undefined") {
  // 백그라운드 탭에서는 타이머가 늦어진다. 돌아오면 바로 맞춘다.
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") tick();
  });
}
