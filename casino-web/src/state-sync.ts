import type { StateResponse } from "./types";

/** 서버가 현재 화면을 확인한 가장 늦은 시각. 서버 시각은 절대 뒤로 가지 않는다. */
export class ConfirmedTime {
  private current = 0;

  get value() {
    return this.current;
  }

  confirm(serverTime: number) {
    if (!Number.isFinite(serverTime) || serverTime <= this.current) return false;
    this.current = serverTime;
    return true;
  }
}

// 버전이 같아도 채팅이나 시간차 공개 카드가 달라질 수 있다.
export const snapshotKey = (state: StateResponse) => JSON.stringify([state.me, state.table, state.tables]);

const SERVER_TIME_PREFIX = /^\{"server_time":(-?\d+)(,|\})/;

/** 받은 원문에서 server_time만 읽는다 (JSON.parse 전에 시계 표본을 잡는다). 형식이 다르면 null. */
export function readServerTime(raw: string): number | null {
  const match = SERVER_TIME_PREFIX.exec(raw);
  return match ? Number(match[1]) : null;
}

/**
 * 원문에서 server_time을 뺀 비교용 키. 같은 키면 화면에 보일 내용이 같아 파싱도 건너뛴다.
 * 하트비트({"server_time":N})는 "{}". 서버 형식이 다르면 null (그때는 파싱해서 snapshotKey로 비교한다).
 */
export function rawStateKey(raw: string): string | null {
  const match = SERVER_TIME_PREFIX.exec(raw);
  if (!match) return null;
  return match[2] === "}" ? "{}" : `{${raw.slice(match[0].length)}`;
}

/** 보이는 테이블이 같을 때 늦게 도착한 옛 상태인지. 버전이 낮으면 옛 상태다 (다시 만든 테이블은 5초 뒤 받아들인다). */
export const isStaleSnapshot = (current: StateResponse | null, incoming: StateResponse) => {
  if (!current || current.table?.id !== incoming.table?.id) return false;
  const now = current.table?.version ?? 0,
    next = incoming.table?.version ?? 0;
  if (next < now) return incoming.server_time - current.server_time < 5000;
  if (next > now) return false;
  return incoming.server_time < current.server_time;
};

/**
 * 다음 틱까지 기다릴 시간: 가장 가까운 예정 사건(카드 출발·도착, 뒤집기, 연출 끝) 또는
 * 마감 카운트다운의 다음 초 경계. 기다릴 것이 없으면 null.
 */
export function nextTickDelay(now: number, events: readonly number[], deadline: number): number | null {
  let best = Infinity;
  for (const at of events) {
    if (at > now && at - now < best) best = at - now;
  }
  if (deadline > now) best = Math.min(best, (deadline - now) % 1000 || 1000);
  return best === Infinity ? null : Math.max(1, Math.ceil(best));
}

export const OFFSET_WINDOW = 20;
export const OFFSET_DECAY_MS = 2;
export const OFFSET_STEP_MS = 1500;
export const OFFSET_STEP_SAMPLES = 15;

type MonotonicSource = { timeOrigin?: number; now(): number };
const monotonicSource: MonotonicSource | null =
  typeof performance !== "undefined" && typeof performance.now === "function" ? performance : null;
/** 단조 시계의 기준점. timeOrigin이 없는 브라우저는 처음 한 번 잰 벽시계로 맞춘다. */
const monotonicOrigin =
  monotonicSource && Number.isFinite(monotonicSource.timeOrigin)
    ? (monotonicSource.timeOrigin as number)
    : Date.now() - (monotonicSource?.now() ?? 0);

/**
 * 기기 시각 (epoch ms). 벽시계(Date.now)가 아니라 단조 시계(performance.now)로 흘러서, 사용자가 기기 시계를
 * 바꾸거나 NTP가 시계를 당겨도 거꾸로 가거나 튀지 않는다. 시계 표본의 받은 시각과 서버 시각 계산에 함께 쓴다.
 */
export const deviceNow = (): number => (monotonicSource ? monotonicOrigin + monotonicSource.now() : Date.now());

/**
 * 서버 시계와의 차이(server_time − 받은 시각). 표본은 전달 지연만큼 늘 작게 나오므로 최근 표본의 최댓값을 쓴다.
 * 늦게 온 메시지로는 내려가지 않고, 표본당 2ms씩만 천천히 내려간다 (시계 흐름 차이 보정).
 * 받은 시각은 단조 시계(deviceNow)로 재므로 기기 시계가 바뀌어도 표본이 튀지 않는다. 그래서 크게 다시 잡는
 * 규칙이 없다: 지연이 한동안 길어져 표본이 모두 낮아져도 추정값은 천천히만 내려가 서버 시각이 되감기지 않는다.
 */
export class OffsetEstimator {
  samples: number[] = [];
  value = 0;
  ready = false;
  private stepSamples = 0;

  /** 표본을 넣는다. 추정값이 바뀌었으면 true. */
  add(sample: number): boolean {
    if (!Number.isFinite(sample)) return false;
    this.samples.push(sample);
    if (this.samples.length > OFFSET_WINDOW) this.samples.shift();
    const before = this.value;
    if (!this.ready) {
      this.ready = true;
      this.value = sample;
      return true;
    }
    if (sample < this.value - OFFSET_STEP_MS) this.stepSamples++;
    else this.stepSamples = 0;
    if (this.stepSamples >= OFFSET_STEP_SAMPLES) {
      this.samples = [sample];
      this.value = sample;
      this.stepSamples = 0;
      return this.value !== before;
    }
    const best = Math.max(...this.samples);
    this.value = best >= this.value ? best : Math.max(best, this.value - OFFSET_DECAY_MS);
    return this.value !== before;
  }
}

const sameIds = (a: readonly { id: string }[], b: readonly { id: string }[]) =>
  a.length === b.length && a.every((item, index) => item.id === b[index].id);
const sameJson = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/**
 * 새 상태에서 내용이 그대로인 부분(채팅·기록·테이블 목록·규칙·내 정보)은 이전 객체를 다시 쓴다.
 * 그래야 memo한 컴포넌트(채팅 목록 등)가 상태가 올 때마다 다시 그려지지 않는다.
 * 채팅과 기록은 한 번 생기면 바뀌지 않아 id만 비교한다.
 */
export function shareStable(previous: StateResponse | null, next: StateResponse): StateResponse {
  if (!previous) return next;
  const me = sameJson(previous.me, next.me) ? previous.me : next.me;
  const tables = sameJson(previous.tables, next.tables) ? previous.tables : next.tables;
  let table = next.table;
  const before = previous.table;
  if (table && before && before.id === table.id) {
    table = {
      ...table,
      messages: sameIds(before.messages, table.messages) ? before.messages : table.messages,
      history: sameIds(before.history, table.history) ? before.history : table.history,
      rules: sameJson(before.rules, table.rules) ? before.rules : table.rules,
      dealer: sameJson(before.dealer, table.dealer) ? before.dealer : table.dealer,
    };
  }
  return { ...next, me, tables, table };
}
