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

/**
 * Shorter windows, where the promise is weaker but still worth keeping.
 *
 * Below a certain height nothing can fit: the matrix has a smallest readable
 * size and the controls under it take what they take, so the range panel is
 * taller than the window and the page has to scroll. What must not happen is
 * everything *else* scrolling too - the statistics panel has a list it can
 * give up, and the bargain is that it does. Getting this wrong turned twenty
 * pixels of unavoidable scrolling into three hundred.
 */
const SHORT = [
  { name: "a short laptop", width: 1512, height: 760 },
  { name: "a shorter one", width: 1440, height: 700 },
];

/** Rounding, and the pixel a border costs. */
const SLACK = 2;

await drive("Fitting the screen", async ({ browser, report, url }) => {
  for (const screen of SCREENS) {
    const page = await open(browser, url, {
      width: screen.width,
      height: screen.height,
    });

    const measure = measuring(page, report, screen.name, true);

    await measure("a fresh visit");

    // Every depth in the library, since each brings its own rows of chips.
    const stacks = await page.$$eval(".stack-chip", (chips) =>
      chips.map((c) => c.textContent),
    );
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
    if (dealt !== 3)
      report(
        `${screen.name}: the flop did not go down - ${dealt} cards on the board`,
      );
    await measure("a flop dealt");

    const tabs = await page.$$eval(".output-tab", (all) =>
      all.map((t) => t.dataset.tab),
    );
    for (const tab of tabs) {
      await page.evaluate((key) => {
        document.querySelector(`.output-tab[data-tab="${key}"]`)?.click();
      }, tab);
      await settle(page, 150);
      await measure(`the ${tab} view`);
    }

    // The statistics panel at its tallest: every category showing.
    await page.evaluate(() =>
      document.querySelector(".panel-stats .panel-head .btn")?.click(),
    );
    await settle(page, 150);
    await measure("the statistics panel opened up");

    await page.close();
  }

  // The short ones, where what is checked is that nothing scrolls that did not
  // have to.
  for (const screen of SHORT) {
    const page = await open(browser, url, {
      width: screen.width,
      height: screen.height,
    });
    const measure = measuring(page, report, screen.name, false);
    await measure("a fresh visit");
    await dealFlop(page);
    await settle(page);
    await measure("a flop dealt");
    await page.evaluate(() =>
      document.querySelector(".panel-stats .panel-head .btn")?.click(),
    );
    await settle(page, 150);
    await measure("the statistics panel opened up");
    await page.close();
  }

  // And a window that changes size under a session already running, which is
  // what happens when a reader splits their screen or rotates a tablet. The
  // fit is worked out from measurements, so it has to be worked out again -
  // and the pass that does it must land in one go rather than creep into place
  // over the next few repaints.
  const page = await open(browser, url, { width: 1920, height: 950 });
  await dealFlop(page);
  await settle(page);
  for (const [width, height, promise] of [
    [1512, 820, true],
    [1512, 760, false],
    [1440, 700, false],
    [1920, 950, true],
  ]) {
    await page.setViewport({ width, height });
    await settle(page, 200);
    const measure = measuring(
      page,
      report,
      `resized to ${width}x${height}`,
      promise,
    );
    await measure("the layout");
  }
  await page.close();

  // And going full screen, which is a window changing size without the reader
  // touching anything - so nothing is being hovered when it happens, and a fit
  // that waits to be hovered is a fit that never comes. The window itself
  // cannot grow under a headless browser, so what is checked is that the pass
  // runs at all: the widths are pushed to something the window has no room for
  // first, and only a fresh measurement puts them back.
  const screen = await open(browser, url, { width: 1512, height: 820 });
  await dealFlop(screen);
  await settle(screen);
  const pressFull = async () => {
    // A real press rather than a scripted one: full screen is only given to a
    // window whose reader asked for it, and a `click()` from script is not a
    // reader asking.
    const button = await screen.evaluateHandle(() =>
      [...document.querySelectorAll(".menubar .btn")].find(
        (b) =>
          b.textContent.includes("full screen") ||
          b.textContent.includes("Full screen"),
      ),
    );
    await button.asElement()?.click();
    await settle(screen, 500);
  };
  for (const what of ["going full screen", "coming back out"]) {
    await screen.evaluate(() => {
      document
        .querySelector(".workspace")
        .style.setProperty("--w-range", "1200px");
    });
    await pressFull();
    const kept = await screen.evaluate(() =>
      document.querySelector(".workspace").style.getPropertyValue("--w-range"),
    );
    if (kept === "1200px") {
      report(
        `${what}: the panels kept the size they were before, and nothing measured again`,
      );
    }
    const measure = measuring(screen, report, what, true);
    await measure("the layout");
  }
  await screen.close();
});

/**
 * How much of the page hangs off the screen, and how much of that was forced.
 *
 * `forced` is the tallest panel that has no list to give up. Where that is
 * already past the bottom the page scrolls by the difference whatever else
 * happens, so that much is not a finding - anything beyond it is.
 */
function measuring(page, report, where, mustFit) {
  return async (what) => {
    const over = await page.evaluate(() => {
      const workspace = document.querySelector(".workspace");
      const top = workspace.getBoundingClientRect().top + window.scrollY;
      const below =
        Number.parseFloat(getComputedStyle(workspace).paddingBottom) || 0;
      const room = window.innerHeight - top - below;
      const forced = [
        ...document.querySelectorAll(".panel-range, .panel-output"),
      ].reduce(
        (tallest, panel) =>
          Math.max(tallest, panel.getBoundingClientRect().height),
        0,
      );
      return {
        down: document.documentElement.scrollHeight - window.innerHeight,
        across: document.documentElement.scrollWidth - window.innerWidth,
        forced: Math.max(0, Math.ceil(forced - room)),
      };
    });
    if (over.across > 0) {
      report(`${where}: ${what} is ${over.across}px wider than the screen`);
    }
    if (mustFit && over.down > 0) {
      report(`${where}: ${what} overflows by ${over.down}px down`);
    } else if (over.down > over.forced + SLACK) {
      report(
        `${where}: ${what} scrolls ${over.down}px, and only ${over.forced}px of that is a panel that cannot shrink`,
      );
    }
  };
}
