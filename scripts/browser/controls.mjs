/**
 * Can the controls actually be taken hold of?
 *
 * A slider can be wired up correctly, report the right value when it is set
 * from script, have a test that passes - and still be a dead strip of grey on
 * the screen, because the part a hand aims at is not where the browser thinks
 * the thumb is. That is what happened here: the thumb was drawn six pixels
 * below its bar and half of it hung outside the box the browser clips it to,
 * so a press on the thumb you could see landed on the track behind it and the
 * slider did nothing at all. Every test in the suite passed throughout, since
 * every one of them set `value` and fired `input` by hand.
 *
 * So this presses where the thumb is drawn and drags, which is the only way to
 * find out. It walks whatever range inputs are on screen rather than a list
 * kept here, so a slider added later is covered by having been added.
 */
import { dealFlop, drive, open, settle } from "./harness.mjs";

/** What has to be pointed at before everything is on screen. */
const REVEAL = ".share-row";

/**
 * Everything on screen that a hand is meant to be able to drag.
 *
 * Runs inside the page, so it carries its own helper: a function handed to the
 * browser arrives as its own source and knows nothing about the module it came
 * from.
 */
function sliders(only) {
  // A point along the track, never within a thumb's width of either end.
  const hold = (share, length) => Math.min(Math.max(share * length, 7), length - 7);
  const all = [...document.querySelectorAll("input[type=range]")];
  return all
    .map((input, index) => ({ input, index }))
    .filter(({ input }) => input.offsetParent !== null && !input.disabled)
    .filter(({ index }) => only === null || index === only)
    .map(({ input, index }) => {
      const box = input.getBoundingClientRect();
      const min = Number(input.min || 0);
      const max = Number(input.max || 100);
      const at = (Number(input.value) - min) / (max - min || 1);
      const vertical = box.height > box.width;
      return {
        // Sliders come in pairs and pairs share a class, so which one this is
        // is where it is in the page rather than what it is called.
        index,
        what: `${input.className.split(" ").slice(-1)[0]} #${index}`,
        value: input.value,
        vertical,
        // Where the thumb sits, kept a few pixels inside the ends so the press
        // lands on the thumb rather than beside it.
        x: box.x + (vertical ? box.width / 2 : hold(at, box.width)),
        y: box.y + (vertical ? hold(1 - at, box.height) : box.height / 2),
        length: vertical ? box.height : box.width,
        origin: vertical ? box.y : box.x,
      };
    });
}

await drive("Taking hold of the controls", async ({ browser, report, url }) => {
  const page = await open(browser, url, { width: 1512, height: 940 });
  // A board and a second range, so the sliders that need something to measure
  // against are on screen rather than hidden.
  await dealFlop(page, ["Kh", "7h", "2c"]);
  await page.evaluate(() => document.querySelectorAll(".seat")[1].click());
  await settle(page);
  await page.evaluate(() => {
    const box = document.querySelector(".notation");
    box.dispatchEvent(new Event("focus"));
    box.value = "88+, ATs+, AQo+";
    box.dispatchEvent(new Event("blur"));
  });
  await settle(page);
  await page.evaluate(() => document.querySelectorAll(".seat")[0].click());
  await settle(page, 300);
  await page.hover(REVEAL);
  await settle(page, 200);

  const found = await page.evaluate(sliders, null);
  if (found.length < 5) report(`only ${found.length} sliders on screen, expected more`);

  for (const { index } of found) {
    // Pointed at again before each one: the widest of these lives in a popup
    // that is only open while the pointer is on the row it belongs to, and by
    // now the pointer has been somewhere else. Measured again too, for the
    // same reason - a box read a moment ago may be a box that has closed.
    await page.hover(REVEAL);
    await settle(page, 120);
    const [slider] = await page.evaluate(sliders, index);
    if (!slider) {
      report(`slider #${index} left the page before it could be taken hold of`);
      continue;
    }

    const before = await page.evaluate(() =>
      [...document.querySelectorAll("input[type=range]")].map((input) => input.value),
    );
    // Pull it towards whichever end it is further from: aiming for the middle
    // moves nothing when the thumb is already sitting there, and a slider that
    // did not move is what this check is looking for.
    const at = (Number(slider.value) - 0) / 100;
    const towards = slider.origin + slider.length * (at > 0.5 ? 0.25 : 0.75);
    await page.mouse.move(slider.x, slider.y);
    await page.mouse.down();
    if (slider.vertical) await page.mouse.move(slider.x, towards, { steps: 8 });
    else await page.mouse.move(towards, slider.y, { steps: 8 });
    await page.mouse.up();
    await settle(page, 120);

    // What moved, rather than which input moved. Two handles on one track can
    // be sitting on the same value, and then a drag deliberately picks up the
    // other one - the control answered, which is what is being asked.
    const after = await page.evaluate(
      (at) => {
        const all = [...document.querySelectorAll("input[type=range]")];
        const mine = all[at];
        if (!mine) return null;
        // Kin is the track they share, not the class they wear: the two
        // handles of one slider are named apart on purpose.
        return all
          .map((input, index) => ({ index, value: input.value, kin: input.parentElement === mine.parentElement }))
          .filter((input) => input.index === at || input.kin);
      },
      index,
    );
    if (after === null) {
      report(`${slider.what} went off the page while being dragged`);
      continue;
    }
    const moved = after.filter(({ index: at, value }) => value !== before[at]);
    if (moved.length === 0) {
      report(`${slider.what} does not answer a drag on its thumb (still ${slider.value})`);
    } else {
      const said = moved.map(({ index: at }) => `#${at} ${before[at]} → ${after.find((s) => s.index === at).value}`);
      console.log(`  ${slider.what}: ${said.join(", ")}`);
    }
  }
});
