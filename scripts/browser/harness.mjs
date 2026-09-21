/**
 * Driving the built page in a real browser.
 *
 * jsdom answers what is on the page; it cannot answer how tall it comes out,
 * what a hover costs, whether a finger scrolls, or what the engine takes in
 * WebAssembly rather than in Rust. Those need a browser, so the checks that
 * need them live here rather than in the test suite.
 *
 * They are kept out of `make check` on purpose: they want a browser image and
 * the better part of a minute, and what they guard against is slow drift
 * rather than something a wrong line of code does straight away.
 */
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

// The browser and its driver come from the image rather than from the app's
// own dependencies: this checks the built page, it is not something the page
// ships, and nobody building the app should have to download a browser.
export const puppeteer = createRequire(import.meta.url)("puppeteer");

const TYPES = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".wasm": "application/wasm",
  ".png": "image/png",
  ".webp": "image/webp",
  ".xml": "application/xml",
  ".txt": "text/plain",
};

/** Serves the built site, on whatever port is going. */
export function serve(root) {
  const base = path.resolve(root);
  return new Promise((done) => {
    const server = http.createServer((request, response) => {
      const asked = decodeURIComponent(new URL(request.url, "http://x").pathname);
      let file = path.join(base, asked);
      if (fs.existsSync(file) && fs.statSync(file).isDirectory()) {
        file = path.join(file, "index.html");
      }
      if (!file.startsWith(base) || !fs.existsSync(file)) {
        response.writeHead(404).end("no");
        return;
      }
      const type = TYPES[path.extname(file)] ?? "application/octet-stream";
      response.writeHead(200, { "content-type": type }).flushHeaders?.();
      fs.createReadStream(file).pipe(response);
    });
    server.listen(0, "127.0.0.1", () => done(server));
  });
}

/**
 * Opens the app and hands it to `body`, with a way to record what is wrong.
 *
 * Every check reports rather than throws, so one failure does not hide the
 * others: a screen that is wrong in four places should say so in one run.
 */
export async function drive(what, body, { root = "web/dist" } = {}) {
  const server = await serve(root);
  const browser = await puppeteer.launch({ args: ["--no-sandbox", "--disable-dev-shm-usage"] });
  const wrong = [];
  const report = (line) => wrong.push(line);
  try {
    await body({
      browser,
      report,
      url: `http://127.0.0.1:${server.address().port}/`,
    });
  } finally {
    await browser.close();
    server.close();
  }
  if (wrong.length > 0) {
    console.error(`${what}: ${wrong.length} thing(s) wrong\n`);
    for (const line of wrong) console.error(`  ${line}`);
    process.exit(1);
  }
  console.log(`${what}: all good`);
}

/** Opens the app on a fresh visit - nothing saved, nothing left over. */
export async function open(browser, url, viewport) {
  const page = await browser.newPage();
  await page.setViewport(viewport);
  await page.goto(url, { waitUntil: "networkidle0" });
  // A column width left over from another screen is not what is being
  // measured, and the first visit is the one that has to be right.
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "networkidle0" });
  await page.waitForSelector(".panel-range .cell");
  await settle(page);
  return page;
}

/**
 * Deals a flop, from the board panel rather than from the dead cards.
 *
 * Both have a grid of fifty-two, and the dead one comes first in the document -
 * so a selector that does not say which lands on the wrong one and quietly
 * marks three cards dead instead of dealing them.
 */
export async function dealFlop(page, cards = ["Kh", "7d", "2c"]) {
  await page.evaluate((names) => {
    for (const name of names) {
      document.querySelector(`.panel-board .card-cell[data-card="${name}"]`)?.click();
    }
  }, cards);
  await settle(page, 250);
  // Said back rather than assumed: a selector that lands on nothing clicks
  // nothing, and a check that went on to measure an empty board would pass for
  // the wrong reason.
  return page.evaluate(() => document.querySelectorAll(".board-slot[data-card]").length);
}

/** Fills the other seat, so there is something to measure against. */
export async function fillOtherSeat(page, notation = "22+, A2s+, K9s+, A8o+, KJo+") {
  await page.evaluate(() => document.querySelectorAll(".seat")[1]?.click());
  await settle(page, 150);
  await page.evaluate((text) => {
    const box = document.querySelector(".notation");
    box.dispatchEvent(new Event("focus"));
    box.value = text;
    box.dispatchEvent(new Event("blur"));
  }, notation);
  await settle(page, 150);
  await page.evaluate(() => document.querySelectorAll(".seat")[0]?.click());
  await settle(page, 200);
}

/** Lets the app finish what a press set off. */
export const settle = (page, ms = 80) =>
  page.evaluate(
    (wait) =>
      new Promise((done) => {
        requestAnimationFrame(() => setTimeout(done, wait));
      }),
    ms,
  );
