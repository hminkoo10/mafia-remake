import assert from "node:assert/strict";
import test from "node:test";
import { isStaleSnapshot, nextClockDelay, snapshotKey } from "../src/state-sync.ts";

const fixture = (server_time = 1000) => ({
  server_time, me: { user_id: "1", coins: 10000 }, tables: [],
  table: { id: "blackjack", version: 5, round: { dealer: ["As", "??"] }, messages: Array.from({ length: 40 }, (_, i) => ({ id: String(i), text: "hello" })) },
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

test("late responses cannot roll back the selected table", () => {
  assert.equal(isStaleSnapshot(fixture(2000), fixture(1000)), true);
  assert.equal(isStaleSnapshot(fixture(2000), fixture(2000)), false);
  assert.equal(isStaleSnapshot(null, fixture()), false);
  const other = fixture(1000);
  other.table!.id = "holdem";
  assert.equal(isStaleSnapshot(fixture(2000), other), false);
});

test("clock sleeps when idle, ticks on second boundaries, and finishes scheduled reveals", () => {
  assert.equal(nextClockDelay(5000, 0, 0, 0), null);
  assert.equal(nextClockDelay(5000, 15000, 0, 0), 1000);
  assert.equal(nextClockDelay(5050, 15000, 0, 0), 950);
  assert.equal(nextClockDelay(5000, 15000, 5400, 0), 100);
  assert.equal(nextClockDelay(5375, 0, 5400, 0), 25);
  assert.equal(nextClockDelay(5400, 0, 5400, 0), null);
  assert.equal(nextClockDelay(5000, 0, 0, 7500), 100);
});
