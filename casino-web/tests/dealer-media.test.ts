import assert from "node:assert/strict";
import test from "node:test";
import { selectDealerClip, shouldFinishDealerGesture } from "../src/dealer-media.ts";

test("a short card reveal does not cut a playing hand gesture; new actions still interrupt", () => {
  assert.equal(shouldFinishDealerGesture("idle", "deal", true), true);
  assert.equal(shouldFinishDealerGesture("idle", "flip", true), true);
  assert.equal(shouldFinishDealerGesture("idle", "deal", false), false);
  assert.equal(shouldFinishDealerGesture("flip", "deal", true), false);
  assert.equal(shouldFinishDealerGesture("idle", "idle", true), false);
});

test("already preloaded deal/flip clips switch on each mood change", () => {
  const ready = { idle: true, deal: true, flip: true };
  assert.equal(selectDealerClip("deal", "idle", ready, {}), "deal");
  assert.equal(selectDealerClip("flip", "deal", ready, {}), "flip");
  assert.equal(selectDealerClip("idle", "flip", ready, {}), "idle");
  assert.equal(selectDealerClip("deal", "idle", ready, {}), "deal");
});

test("loading/missing clips keep a usable frame and never invent an unrelated move", () => {
  assert.equal(selectDealerClip("flip", "deal", { deal: true }, {}), "deal");
  assert.equal(selectDealerClip("deal", "deal", { deal: true, idle: true }, { deal: true }), "idle");
  assert.equal(selectDealerClip("idle", null, { flip: true }, {}), null);
  assert.equal(selectDealerClip("deal", null, {}, {}), null);
});
