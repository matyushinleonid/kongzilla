/**
 * Does the app stay quick under the pointer, and keep its place?
 *
 * The regressions that reach a reader are the ones nothing here used to catch.
 * Pointing at a statistics row once cost three hundred and sixty milliseconds,
 * because a render read `scrollHeight` and made the browser lay the page out
 * again to answer; the equity table jumped back to the top on every hover,
 * because pointing at a row rebuilt the table under it. Neither is a wrong
 * answer - both are the right answer, delivered badly - so no test of what is
 * on the page was ever going to see them.
 *
 * The budgets below are generous on purpose. They are not a target to tune
 * against; they are the line between quick and visibly not, and they are there
 * to catch a change that crosses it by an order of magnitude.
 */
import { dealFlop, drive, fillOtherSeat, open, settle } from "./harness.mjs";

/** A laptop, since that is where a slow hover is felt first. */
const SCREEN = { width: 1512, height: 820 };

/**
 * Milliseconds one pointer move may cost, start to finish.
 *
 * Most of it is not JavaScript. A render dirties the page and the browser has
 * to lay it out again before it can paint, and that is what a reader waits
 * for - so it is counted here, and trying to separate it out only flatters the
 * number. Profiling a hover puts around seven parts in ten in that layout.
 *
 * Deliberately loose. This is not a target to tune against and the machine it
 * runs on is not the machine anyone reads on; it is the line between quick and
 * visibly not. What it is for is the regression that made pointing at a row
 * cost three hundred and sixty milliseconds, and it would have caught that
 * three times over.
 */
const BUDGET = 120;

/**
 * What one pointer move costs, with the layout it causes forced.
 *
 * Forced because that is the cost: a handler that dirties the page has spent
 * the time whether or not it waited for it, and reading `offsetHeight` is how
 * you make it show up here rather than on the next frame. The same two reads
 * with nothing pointed at come back alongside, so a number that has grown can
 * be read as the app's doing or the page's.
 */
const costOf = (page, selector, times) =>
  page.evaluate(
    (css, runs) => {
      const element = document.querySelector(css);
      if (!element) return null;
      const box = element.getBoundingClientRect();
      const at = { clientX: box.left + box.width / 2, clientY: box.top + box.height / 2 };
      const move = (type) =>
        element.dispatchEvent(
          new PointerEvent(type, { bubbles: true, ...at, pointerType: "mouse" }),
        );
      const time = (body) => {
        const started = performance.now();
        for (let run = 0; run < runs; run += 1) body();
        return (performance.now() - started) / runs;
      };

      // One first, so what is measured is the steady state rather than
      // whatever the first of anything costs.
      move("pointerenter");
      move("pointerleave");
      const both = time(() => {
        move("pointerenter");
        void document.body.offsetHeight;
        move("pointerleave");
        void document.body.offsetHeight;
      });
      // The same two layouts with nothing pointed at. Laying this page out is
      // not free and not the app's doing; what the app is answerable for is
      // the difference.
      const alone = time(() => {
        void document.body.offsetHeight;
        void document.body.offsetHeight;
      });
      return { cost: both, idle: alone };
    },
    selector,
    times,
  );

await drive("Staying quick", async ({ browser, report, url }) => {
  const page = await open(browser, url, SCREEN);

  // On a board, where the statistics panel is at its longest and the hover
  // has the most to light up.
  const dealt = await dealFlop(page);
  if (dealt !== 3) report(`the flop did not go down - ${dealt} cards on the board`);

  for (const [what, selector] of [
    ["a statistics row", ".stat-row"],
    ["a matrix cell", ".panel-range .cell"],
  ]) {
    const cost = await costOf(page, selector, 20);
    if (cost === null) {
      report(`${what} is not on the page to point at`);
    } else if (cost.cost > BUDGET) {
      report(`pointing at ${what} costs ${cost.cost.toFixed(1)}ms, over the ${BUDGET}ms budget`);
    } else {
      console.log(
        `  pointing at ${what}: ${cost.cost.toFixed(1)}ms` +
          ` (${cost.idle.toFixed(1)}ms with nothing pointed at)`,
      );
    }
  }

  // Something to measure against, or the equity views are an empty note and
  // there is no table to keep its place.
  await fillOtherSeat(page);

  // The equity table keeps its place. Pointing at a row is reading, not
  // navigating, and a table that jumps to the top under the pointer cannot be
  // read at all.
  await page.evaluate(() => {
    document.querySelector('.output-tab[data-tab="eq-graph"]')?.click();
  });
  await settle(page, 300);
  const kept = await page.evaluate(() => {
    const table = document.querySelector(".eq-table");
    if (!table || table.scrollHeight - table.clientHeight < 40) return null;
    table.scrollTop = 120;
    const row = table.querySelectorAll(".eq-row[data-combo]")[4];
    row?.dispatchEvent(new PointerEvent("pointerenter", { bubbles: true, pointerType: "mouse" }));
    void document.body.offsetHeight;
    return table.scrollTop;
  });
  if (kept === null) report("the equity table has nothing to scroll, so its place is not tested");
  else if (kept !== 120) report(`the equity table jumped from 120 to ${kept} under the pointer`);

  // The picture it exports, which nothing else can check: jsdom has no canvas,
  // so the only test of this today is that it fails there. Driven through the
  // button a reader presses rather than through a hook put here for the
  // purpose - which means the naming dialog is on the way, and gets tested too.
  await page.evaluate(() => {
    // The browser's own save dialog cannot be answered from here, so take the
    // path the browsers without one take.
    Object.defineProperty(window, "showSaveFilePicker", {
      configurable: true,
      value: undefined,
    });
    window.__made = null;
    const real = URL.createObjectURL.bind(URL);
    URL.createObjectURL = (blob) => {
      window.__made = blob;
      return real(blob);
    };
  });
  await page.evaluate(() => {
    const image = [...document.querySelectorAll(".menubar .btn")].find(
      (button) => button.textContent === "Image",
    );
    image?.click();
  });
  await page.waitForSelector(".save-sheet", { timeout: 5000 }).catch(() => null);
  await page.evaluate(() => document.querySelector(".save-sheet")?.requestSubmit());
  await settle(page, 300);
  const picture = await page.evaluate(async () => {
    const blob = window.__made;
    if (!blob) return "nothing was made";
    const bitmap = await createImageBitmap(blob);
    return { type: blob.type, size: blob.size, width: bitmap.width, height: bitmap.height };
  });
  if (typeof picture === "string") {
    report(`the range picture could not be made: ${picture}`);
  } else if (picture.type !== "image/png" || picture.size < 1000) {
    report(`the range picture came out as ${picture.size} bytes of ${picture.type}`);
  } else if (picture.width < 200 || picture.height < 200) {
    report(`the range picture came out ${picture.width}x${picture.height}`);
  } else {
    console.log(`  the range picture: ${picture.width}x${picture.height}, ${picture.size} bytes`);
  }

  await page.close();
});
