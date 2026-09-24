import assert from "node:assert/strict";
import test from "node:test";
import {
  ConfirmedTime,
  deviceNow,
  isStaleSnapshot,
  nextTickDelay,
  OffsetEstimator,
  OFFSET_DECAY_MS,
  OFFSET_WINDOW,
  rawStateKey,
  readServerTime,
  shareStable,
  snapshotKey,
} from "../src/state-sync.ts";

const fixture = (server_time = 1000, version = 5) => ({
  server_time, me: { user_id: "1", coins: 10000 }, tables: [],
  table: {
    id: "blackjack",
    version,
    round: { dealer: ["As", "??"] },
    rules: { min_bet: 100 },
    dealer: { id: "sophia" },
    history: [{ id: "h1" }],
    messages: Array.from({ length: 40 }, (_, i) => ({ id: String(i), text: "hello" })),
  },
}) as unknown as Parameters<typeof snapshotKey>[0];

test("heartbeats reuse the view but same-version reveals and capped chat still update", () => {
  const current = fixture();
  const heartbeat = fixture(2000);
  assert.equal(snapshotKey(current), snapshotKey(heartbeat));
  heartbeat.table!.round!.dealer[1] = "Kh";
  assert.notEqual(snapshotKey(current), snapshotKey(heartbeat));
  const chat = fixture(2000);
  chat.table!.messages.shift();
  chat.table!.messages.push({ id: "40", text: "new message" } as typeof chat.table.messages[number]);
  assert.notEqual(snapshotKey(current), snapshotKey(chat));
});

test("raw messages are compared without parsing and heartbeats are recognised", () => {
  const a = JSON.stringify(fixture(1000));
  const b = JSON.stringify(fixture(2500));
  assert.equal(readServerTime(a), 1000);
  assert.equal(readServerTime('{"server_time":1758700000123}'), 1758700000123);
  assert.equal(rawStateKey('{"server_time":1758700000123}'), "{}", "하트비트");
  assert.equal(rawStateKey(a), rawStateKey(b), "시각만 다르면 같은 화면");
  const changed = fixture(2500);
  changed.table!.round!.dealer[1] = "Kh";
  assert.notEqual(rawStateKey(a), rawStateKey(JSON.stringify(changed)));
  assert.equal(JSON.parse(rawStateKey(a)!).me.coins, 10000, "키는 server_time만 뺀 JSON이다");
  assert.equal(rawStateKey('{"me":{},"server_time":5}'), null, "모르는 형식은 파싱해서 비교한다");
  assert.equal(readServerTime("not json"), null);
});

test("confirmed server time advances on heartbeat and never goes backward", () => {
  const confirmed = new ConfirmedTime();
  assert.equal(confirmed.confirm(1000), true, "heartbeat confirms held screen");
  assert.equal(confirmed.value, 1000);
  assert.equal(confirmed.confirm(1200), true);
  assert.equal(confirmed.value, 1200);
  assert.equal(confirmed.confirm(1100), false, "stale heartbeat ignored");
  assert.equal(confirmed.value, 1200);
  assert.equal(confirmed.confirm(1200), false);
});

test("late responses cannot roll back the selected table", () => {
  assert.equal(isStaleSnapshot(fixture(2000), fixture(1000)), true);
  assert.equal(isStaleSnapshot(fixture(2000), fixture(2000)), false);
  assert.equal(isStaleSnapshot(null, fixture()), false);
  const other = fixture(1000);
  other.table!.id = "holdem";
  assert.equal(isStaleSnapshot(fixture(2000), other), false);
});

test("the table version orders snapshots even when their clocks cross", () => {
  // 늦게 찍혔지만 먼저 읽은 v5는 v6 뒤에 와도 옛 상태다.
  assert.equal(isStaleSnapshot(fixture(2000, 6), fixture(2100, 5)), true);
  // 먼저 찍혔지만 나중에 읽은 v6은 새 상태다.
  assert.equal(isStaleSnapshot(fixture(2100, 5), fixture(2000, 6)), false);
  // 관리자가 다시 만든 테이블(버전이 처음부터)은 잠시 뒤 받아들인다.
  assert.equal(isStaleSnapshot(fixture(2000, 40), fixture(9000, 1)), false);
});

test("the clock wakes exactly at the next scheduled event or second boundary", () => {
  assert.equal(nextTickDelay(5000, [], 0), null, "대기 중에는 깨지 않는다");
  assert.equal(nextTickDelay(5000, [], 15000), 1000);
  assert.equal(nextTickDelay(5050, [], 15000), 950);
  assert.equal(nextTickDelay(5000, [5230, 5650, 6250], 0), 230, "다음 카드 출발");
  assert.equal(nextTickDelay(5300, [5230, 5650, 6250], 0), 350, "지난 사건은 건너뛴다");
  assert.equal(nextTickDelay(5000, [5600], 15000), 600, "마감 초 경계보다 카드가 먼저");
  assert.equal(nextTickDelay(5000, [7000], 5400), 400, "마감 자체");
  assert.equal(nextTickDelay(5000, [5000.4], 0), 1, "최소 1ms");
  assert.equal(nextTickDelay(6300, [5230, 5650, 6250], 0), null);
});

test("the clock offset follows the fastest message and never drops on one late message", () => {
  const clock = new OffsetEstimator();
  assert.equal(clock.add(1000), true);
  assert.equal(clock.value, 1000);
  assert.equal(clock.add(700), false, "300ms 늦게 도착한 메시지");
  assert.equal(clock.value, 1000);
  assert.equal(clock.add(1040), true, "더 빨리 도착한 메시지는 바로 반영한다");
  assert.equal(clock.value, 1040);
  assert.equal(clock.add(Number.NaN), false);
});

test("the clock offset decays slowly once the best sample leaves the window", () => {
  const clock = new OffsetEstimator();
  clock.add(1100);
  for (let i = 0; i < OFFSET_WINDOW - 1; i++) clock.add(1000);
  assert.equal(clock.value, 1100);
  clock.add(1000);
  assert.equal(clock.value, 1098, "표본당 2ms씩만 내려간다");
  for (let i = 0; i < 100; i++) clock.add(1000);
  assert.equal(clock.value, 1000, "결국 실제 값에 닿는다");
});

test("a sustained latency rise never pulls the offset back faster than the decay", () => {
  // 지연이 40ms에서 1.1초로 늘어 몇십 초 이어져도 (하트비트는 계속 온다) 추정값은 표본당 2ms씩만 내려간다.
  const clock = new OffsetEstimator();
  for (let i = 0; i < OFFSET_WINDOW; i++) clock.add(1000 - 40);
  let before = clock.value;
  for (let i = 0; i < 60; i++) {
    clock.add(1000 - 1100);
    assert.ok(before - clock.value <= OFFSET_DECAY_MS, `표본 ${i}: ${before} → ${clock.value}`);
    before = clock.value;
  }
  assert.ok(clock.value > 1000 - 40 - 60 * OFFSET_DECAY_MS - 1, "다시 크게 잡지 않는다");
  // 지연이 돌아오면 바로 제자리.
  clock.add(1000 - 30);
  assert.equal(clock.value, 970);
});

test("the device clock is monotonic and ignores wall-clock changes", () => {
  const realNow = Date.now;
  try {
    const a = deviceNow();
    Date.now = () => realNow() - 3_600_000; // 사용자가 기기 시계를 한 시간 되돌렸다.
    const b = deviceNow();
    assert.ok(b >= a, "벽시계를 되돌려도 거꾸로 가지 않는다");
    assert.ok(b - a < 1000, "벽시계를 따라 튀지 않는다");
    Date.now = () => realNow() + 3_600_000;
    assert.ok(deviceNow() - b < 1000);
  } finally {
    Date.now = realNow;
  }
});

test("unchanged chat, history, rules and tables keep their object identity", () => {
  const before = fixture(1000);
  const same = shareStable(before, fixture(2000));
  assert.equal(same.table!.messages, before.table!.messages);
  assert.equal(same.table!.history, before.table!.history);
  assert.equal(same.table!.rules, before.table!.rules);
  assert.equal(same.me, before.me);
  assert.equal(same.tables, before.tables);
  assert.equal(same.server_time, 2000);
  const chat = fixture(2000);
  chat.table!.messages.shift();
  chat.table!.messages.push({ id: "40", text: "new" } as typeof chat.table.messages[number]);
  const next = shareStable(before, chat);
  assert.notEqual(next.table!.messages, before.table!.messages);
  assert.equal(next.table!.messages[39].id, "40");
  const other = fixture(2000);
  other.table!.id = "holdem";
  assert.equal(shareStable(before, other).table!.messages, other.table!.messages, "다른 테이블은 그대로 쓴다");
  assert.equal(shareStable(null, other), other);
});

test("a short clock step candidate does not re-centre, but a sustained one does", () => {
  const clock = new OffsetEstimator();
  clock.add(1000);
  for (let i = 0; i < 14; i++) clock.add(-1000);
  assert.equal(clock.value, 1000);
  clock.add(-1000);
  assert.equal(clock.value, -1000);
});
