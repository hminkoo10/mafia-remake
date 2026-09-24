import assert from "node:assert/strict";
import test from "node:test";
import { MUSIC_BARS_PER_CYCLE, MUSIC_BPM, MUSIC_CYCLE_BEATS, MUSIC_LOOK_AHEAD_SECONDS, MUSIC_MASTER_GAIN, musicNotesForCycle, notesInWindow, progressionForCycle } from "../src/music.ts";

test("음악 노트는 look-ahead 창 안에서 시간순으로 나온다", () => {
  const notes = musicNotesForCycle(0);
  const beatWindow = (MUSIC_LOOK_AHEAD_SECONDS * MUSIC_BPM) / 60;
  const emitted = notesInWindow(notes, 4, 4 + beatWindow);
  assert.ok(emitted.length > 0);
  assert.ok(emitted.every((note) => note.beat >= 4 && note.beat < 4 + beatWindow));
  assert.deepEqual(emitted, [...emitted].sort((a, b) => a.beat - b.beat || a.kind.localeCompare(b.kind)));
});

test("진행은 세 사이클 뒤 같은 순서로 돌아온다", () => {
  assert.deepEqual(progressionForCycle(0), progressionForCycle(3));
  assert.notDeepEqual(progressionForCycle(0), progressionForCycle(1));
  assert.equal(musicNotesForCycle(2).at(-1)?.beat, MUSIC_CYCLE_BEATS - 0.5);
  assert.equal(MUSIC_BARS_PER_CYCLE, 8);
});

test("음악 게인과 노트 길이는 조용한 범위 안에 있다", () => {
  assert.ok(MUSIC_MASTER_GAIN > 0 && MUSIC_MASTER_GAIN <= 0.1);
  for (const note of musicNotesForCycle(1)) {
    assert.ok(note.gain > 0 && note.gain <= 0.25);
    assert.ok(note.duration > 0);
  }
});
