/**
 * Does the whole app fit on a screen, in every setting, without scrolling?
 *
 * jsdom has no layout, so the rest of the tests cannot answer this: they can
 * say what is on the page but not how tall it comes out. This drives a real
 * browser at the size of a laptop and measures.
 *
 * The contract is that on a screen this big the page never scrolls. Panels may
 * scroll inside themselves - that is what they are for - but the page itself
 * scrolling means something has been pushed off the bottom, and what is pushed
 * off is whatever happens to be last rather than whatever matters least.
 */
import { dealFlop, drive, open, settle } from "./harness.mjs";

/** The screens the app promises to fit on. */
const SCREENS = [
  // A 15-inch MacBook Air at its default scaling, less the menu bar and the
  // browser's own chrome. The smallest screen this promise is made for.
  { name: "MacBook Air 15", width: 1512, height: 820 },
  { name: "1080p", width: 1920, height: 950 },
];

await drive("Fitting the screen", async ({ browser, report, url }) => {
  for (const screen of SCREENS) {
    const page = await open(browser, url, { width: screen.width, height: screen.height });

    const measure = async (what) => {
      const over = await page.evaluate(() => ({
        down: document.documentElement.scrollHeight - window.innerHeight,
        across: document.documentElement.scrollWidth - window.innerWidth,
      }));
      if (over.down > 0 || over.across > 0) {
        report(`${screen.name}: ${what} overflows by ${over.down}px down, ${over.across}px across`);
      }
    };

    await measure("a fresh visit");

    // Every depth in the library, since each brings its own rows of chips.
    const stacks = await page.$$eval(".stack-chip", (chips) => chips.map((c) => c.textContent));
    for (const stack of stacks) {
      await page.evaluate((label) => {
        const chip = [...document.querySelectorAll(".stack-chip")].find(
          (element) => element.textContent === label,
        );
        chip?.click();
      }, stack);
      await settle(page);
      await measure(`the ${stack} charts`);
    }

    // Every output view, with a flop down so none of them is an empty note.
    const dealt = await dealFlop(page);
    if (dealt !== 3) report(`${screen.name}: the flop did not go down - ${dealt} cards on the board`);
    await measure("a flop dealt");

    const tabs = await page.$$eval(".output-tab", (all) => all.map((t) => t.dataset.tab));
    for (const tab of tabs) {
      await page.evaluate((key) => {
        document.querySelector(`.output-tab[data-tab="${key}"]`)?.click();
      }, tab);
      await settle(page, 150);
      await measure(`the ${tab} view`);
    }

    // The statistics panel at its tallest: every category showing.
    await page.evaluate(() => document.querySelector(".panel-stats .panel-head .btn")?.click());
    await settle(page, 150);
    await measure("the statistics panel opened up");

    await page.close();
  }
});
