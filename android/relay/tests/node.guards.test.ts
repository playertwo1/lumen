import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";

// Read as text from the repository (the Workers runtime has no host files, so these run with the Node tests).
describe("source guards", () => {
  const dir = new URL("../src/", import.meta.url);
  const sources = readdirSync(dir).filter((f) => f.endsWith(".ts")).map((f) => [f, readFileSync(new URL(f, dir), "utf8")] as const);

  it("the relay never logs or calls out, and only Room SQLite stores opaque mailbox bytes", () => {
    for (const [file, text] of sources) {
      const code = text.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
      const commonForbidden = ["console.", "KVNamespace", "R2Bucket", "D1Database", "fetch(\"http", "fetch(`http", "Queue", "waitUntil", "setAlarm"];
      for (const forbidden of commonForbidden) {
        expect(code.includes(forbidden), `${file} uses ${forbidden}`).toBe(false);
      }
      if (file !== "room.ts") {
        expect(code.includes(".sql"), `${file} uses .sql`).toBe(false);
      }
    }
  });

  it("the config logs nothing and holds no secret", () => {
    const toml = readFileSync(new URL("../wrangler.toml", import.meta.url), "utf8");
    expect(toml).toMatch(/\[observability\]\s*\nenabled = false/);
    expect(toml).not.toMatch(/ACCESS_KEY\s*=/); // the key is a secret, set with `wrangler secret put`
    expect(toml).not.toMatch(/\[vars\]/);
    expect(toml).toMatch(/new_sqlite_classes = \["Room"\]/);
  });
});
