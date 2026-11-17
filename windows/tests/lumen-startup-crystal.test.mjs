// Regression: on Windows vite swaps src/mochi/greeting for src/lumen/greeting
// (vite.config.ts), so the crystal must live in the swapped-in file — the 0.7.11
// fix landed in the file that is never bundled and the old compass kept showing.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";

const read = (p) => readFileSync(new URL(`../${p}`, import.meta.url), "utf8");

test("the Windows launch greeting is the crystal, never the compass", () => {
  assert.match(read("vite.config.ts"), /src\/mochi\/greeting"\)\) return resolve\(__dirname, "src\/lumen\/greeting\.ts"\)/);
  const greeting = read("src/lumen/greeting.ts");
  assert.doesNotMatch(greeting, /compass/i);
  assert.match(greeting, /lumen-crystal\.gif/);
  assert.match(greeting, /lumen-crystal\.png/);
  assert.match(greeting, /prefers-reduced-motion: reduce/);
  for (const f of ["lumen-crystal.gif", "lumen-crystal.png"]) assert.ok(existsSync(new URL(`../public/${f}`, import.meta.url)));
});

test("the original Coucou greeting does not know about Lumen", () => {
  assert.doesNotMatch(read("src/mochi/greeting.ts"), /lumen/i);
});
