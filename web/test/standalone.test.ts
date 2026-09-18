/**
 * The file people keep on their desktop.
 *
 * Two promises are made about it: it runs with nothing to fetch, and it does not
 * call home. Both are build-time properties, so they are checked against the
 * built artefact rather than against the source that was meant to produce it.
 */

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeAll, describe, expect, test } from "vitest";

const root = resolve(import.meta.dirname, "..");
const artefact = resolve(root, "dist-standalone/index.html");
let page = "";

beforeAll(() => {
  execFileSync("npx", ["vite", "build", "--mode", "standalone", "--logLevel", "warn"], {
    cwd: root,
    stdio: "inherit",
  });
  page = readFileSync(artefact, "utf8");
}, 180_000);

describe("the standalone build", () => {
  test("carries no trace of the beacon", () => {
    // The endpoint, the API it would use, and the names it would report. None of
    // them survive the build, because `__ANALYTICS__` folds the calls away.
    for (const trace of ["/api/event", "sendBeacon", "flop_dealt", "keepalive"]) {
      expect(page).not.toContain(trace);
    }
  });

  test("has nothing left to fetch", () => {
    const local = page.match(/(?:src|href)="(?!data:|https?:|mailto:|#)[^"]+"/g);
    expect(local).toBeNull();
    // The engine rides along rather than being fetched.
    expect(page).toContain("__KONGZILLA_WASM__");
    expect(page.length).toBeGreaterThan(1_000_000);
  });

  test("still links back to the site for the written pages", () => {
    expect(page).toContain("https://kongzilla.leonid.sh/guide/");
  });
});
