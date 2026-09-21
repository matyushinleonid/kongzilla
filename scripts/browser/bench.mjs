/**
 * What the engine costs in the browser rather than in Rust.
 *
 * Every timing in this project has been taken natively and scaled by a guess
 * at what WebAssembly does to it. That guess is load-bearing in one place:
 * the interface decides whether to run a pass over the flops without being
 * asked by comparing an estimate of the work against a threshold, and the
 * threshold was set against native throughput. If wasm is half the speed then
 * "quick enough to run itself" is twice the wait than was intended.
 *
 * So this measures rather than prints an opinion. It reports and does not
 * fail: these are numbers to know, and they move with the machine.
 */
import { dealFlop, drive, fillOtherSeat, open, settle } from "./harness.mjs";

const SCREEN = { width: 1512, height: 900 };

/** Runs something in the page and says how long it took, warm. */
const timeIt = (page, body) => page.evaluate(body);

await drive("What the engine costs here", async ({ browser, report, url }) => {
  const page = await open(browser, url, SCREEN);
  const said = [];
  const say = (line) => {
    said.push(line);
    console.log(`  ${line}`);
  };

  // The heaviest thing there is: every hand in the range against all 22,100
  // flops, and - with two ranges - every hand's equity over the same flops.
  await fillOtherSeat(page);
  const pass = await timeIt(page, async () => {
    const button = [...document.querySelectorAll(".panel-stats .btn")].find((element) =>
      element.textContent?.startsWith("Calculate over"),
    );
    if (!button) return null;
    const started = performance.now();
    button.click();
    // The pass is put behind a timeout so the caption can paint first, so
    // waiting for the caption to go is waiting for the pass.
    await new Promise((done) => {
      const look = () => {
        const still = [...document.querySelectorAll(".panel-stats .btn")].some((element) =>
          element.textContent?.startsWith("Working through"),
        );
        if (still) setTimeout(look, 20);
        else done();
      };
      setTimeout(look, 20);
    });
    return performance.now() - started;
  });
  if (pass === null) report("there was no pass to run");
  else say(`a pass over every flop, with equity: ${(pass / 1000).toFixed(2)}s`);

  // What the interface thinks that cost, against what it did. The threshold it
  // is compared with is the one that decides whether a pass runs unasked.
  const work = await page.evaluate(() => {
    const text = document.body.textContent ?? "";
    const flops = /over all ([\d,]+) flops/.exec(text);
    return flops ? Number(flops[1].replace(/,/g, "")) : null;
  });
  if (work && pass) {
    say(`which is ${Math.round(work)} flops, so ${((pass * 1000) / work).toFixed(1)}µs a flop`);
  }

  // The exact per-hand equity, which happens on every board change and so has
  // to be quick rather than merely bearable.
  const flop = await dealFlop(page);
  if (flop !== 3) report(`the flop did not go down - ${flop} cards on the board`);
  const exact = await timeIt(page, () => {
    const tab = document.querySelector('.output-tab[data-tab="eq-matrix"]');
    const started = performance.now();
    tab?.click();
    void document.body.offsetHeight;
    return performance.now() - started;
  });
  say(`per-hand equity on a flop, drawn: ${exact.toFixed(0)}ms`);

  // And a plain redraw, for scale.
  const redraw = await timeIt(page, () => {
    const cell = document.querySelector(".panel-range .cell");
    const box = cell.getBoundingClientRect();
    const at = { clientX: box.left + 2, clientY: box.top + 2, bubbles: true, pointerType: "mouse" };
    cell.dispatchEvent(new PointerEvent("pointerenter", at));
    void document.body.offsetHeight;
    const started = performance.now();
    for (let run = 0; run < 20; run += 1) {
      cell.dispatchEvent(new PointerEvent("pointerleave", at));
      void document.body.offsetHeight;
      cell.dispatchEvent(new PointerEvent("pointerenter", at));
      void document.body.offsetHeight;
    }
    return (performance.now() - started) / 20;
  });
  say(`one redraw under the pointer: ${redraw.toFixed(1)}ms`);

  await page.close();
});
