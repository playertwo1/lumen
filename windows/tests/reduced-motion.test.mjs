// Reduced motion (src/core/motion.ts): Mochi keeps his colour, eyes and badge but
// stops bouncing, breathing, hopping and throwing particles, and his frame loop
// goes idle instead of running for ever.

import { test } from "node:test";
import assert from "node:assert/strict";

const { BotEngine } = await import("../src/mochi/engine.ts");
const { setReducedMotion } = await import("../src/core/motion.ts");

let clock = 1_000_000;
performance.now = () => clock;
function run(engine, seconds) {
  for (let i = 0; i < seconds * 60; i++) {
    clock += 1000 / 60;
    engine.update(1 / 60);
  }
}

test("a sleeping Mochi breathes and sends z's, unless motion is reduced", () => {
  setReducedMotion(false);
  const a = new BotEngine();
  a.setState("sleeping", true);
  run(a, 4);
  assert.ok(a.busy, "the loop keeps running to breathe");
  assert.ok(a.particles.length > 0, "z's float up");

  setReducedMotion(true);
  const b = new BotEngine();
  b.setState("sleeping", true);
  run(b, 4);
  assert.equal(b.particles.length, 0);
  assert.equal(b.sy, 1, "no breathing");
  assert.equal(b.busy, false, "the loop can stop");
});

test("a body animation lands on its last pose at once", () => {
  setReducedMotion(true);
  const e = new BotEngine();
  e.anim("oy", [[-0.3, 140, (p) => p], [0, 380, (p) => p]]);
  run(e, 0.1);
  assert.equal(e.oy, 0, "no hop");
  assert.equal(e.busy, false);
});

test("colour still changes: state is told by tint, not by movement", () => {
  setReducedMotion(true);
  const e = new BotEngine();
  const before = [...e.colT];
  e.setState("finished", true);
  assert.notDeepEqual([...e.colT], before);
  setReducedMotion(false);
});
