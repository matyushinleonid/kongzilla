/**
 * Does the file on somebody's desktop actually run?
 *
 * The build makes two promises about it, and the test suite checks the first
 * one by reading the file: nothing left to fetch, and no trace of the beacon.
 * Reading proves what is not in there. It cannot prove the thing that matters -
 * that double-clicking it opens a working tool - because that needs a browser,
 * a `file://` address with no server behind it, and a megabyte of WebAssembly
 * decoded out of the page itself.
 *
 * So this opens it the way its reader does, waits for the engine, and does a
 * little work in it. What it is really guarding against is the class of change
 * that is invisible in the source and fatal here: an import that resolves to a
 * URL, a fetch that only works over http, a build flag that stops inlining.
 */
import path from "node:path";
import fs from "node:fs";

import { drive, settle } from "./harness.mjs";

const PAGE = path.resolve("web/dist-standalone/index.html");

if (!fs.existsSync(PAGE)) {
  console.error(`No standalone build at ${PAGE}. Run \`make standalone\` first.`);
  process.exit(1);
}

await drive("Running from a file", async ({ browser, report }) => {
  const page = await browser.newPage();
  await page.setViewport({ width: 1512, height: 820 });

  // Anything the page asks the network for is a promise broken: the file is
  // meant to work on a laptop in a hotel with the wifi off.
  const fetched = [];
  page.on("request", (request) => {
    if (!request.url().startsWith("file://") && !request.url().startsWith("data:")) {
      fetched.push(request.url());
    }
  });
  const broke = [];
  page.on("pageerror", (error) => broke.push(String(error)));
  page.on("console", (message) => {
    if (message.type() === "error") broke.push(message.text());
  });

  await page.goto(`file://${PAGE}`, { waitUntil: "load" });

  // The engine is decoded from the page rather than fetched, so this is the
  // one wait that matters: no cells means no engine.
  try {
    await page.waitForSelector(".panel-range .cell", { timeout: 30000 });
  } catch {
    const said = await page.evaluate(() => document.getElementById("app")?.textContent ?? "");
    report(`the app never came up from a file: "${said.slice(0, 120)}"`);
    await page.close();
    return;
  }
  await settle(page);

  // And it works: a flop goes down and the statistics answer for it.
  await page.evaluate(() => {
    for (const card of ["Kh", "7h", "2c"]) {
      document.querySelector(`.panel-board .card-cell[data-card="${card}"]`)?.click();
    }
  });
  await settle(page);
  const worked = await page.evaluate(() => {
    const rows = [...document.querySelectorAll(".stat-row")].filter((row) => !row.hidden);
    const named = rows.map((row) => row.querySelector(".stat-label")?.textContent);
    return {
      cards: document.querySelectorAll(".panel-board .card-slot.dealt, .board-slots .dealt").length,
      rows: rows.length,
      pair: named.includes("top pair"),
      combos: document.querySelector(".summary")?.textContent ?? "",
    };
  });
  if (worked.rows < 5 || !worked.pair) {
    report(`the statistics did not fill in: ${worked.rows} rows, top pair ${worked.pair}`);
  }
  if (!/\d/.test(worked.combos)) report(`the range summary says nothing: "${worked.combos}"`);

  for (const url of new Set(fetched)) report(`it went to the network for ${url}`);
  for (const line of new Set(broke)) report(`it reported an error: ${line.slice(0, 160)}`);

  await page.close();
});
