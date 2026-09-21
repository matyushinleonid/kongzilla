/**
 * Does the app leave a finger free to scroll?
 *
 * What a phone gets wrong is not what is on the screen but what happens when
 * you touch it. The app wants most of the screen, so anything that swallows a
 * gesture takes the one the whole page depends on, and the failure reads as
 * "this page will not scroll" - which is as bad as a page can be.
 *
 * Testing it by scrolling would be better, and is not on offer: scrolling is
 * the compositor's work, and headless Chrome will not drive it from a
 * synthesized touch - the same gesture as a wheel moves the page three hundred
 * pixels and as a finger moves it none. So this tests the three things that
 * actually stop a finger, each of which is exact and none of which needs a
 * compositor:
 *
 *   - `touch-action`, which says which gestures the browser may keep;
 *   - a press handler calling `preventDefault`, which takes the gesture from
 *     the browser before it has decided what it was;
 *   - a scroll container with nothing to scroll that refuses to pass the
 *     gesture on, which is `overflow` and `overscroll-behavior` together.
 *
 * The last one is the one that had the statistics panel stuck.
 */
import { drive, open, settle } from "./harness.mjs";

/** A phone, and a tablet held upright - both narrow enough to stack. */
const DEVICES = [
  { name: "phone", width: 390, height: 844, deviceScaleFactor: 3, isMobile: true, hasTouch: true },
  { name: "tablet", width: 820, height: 1180, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
];

/** Everywhere a reader's finger lands, which is very nearly everywhere. */
const SURFACES = [
  ["the matrix", ".panel-range .cell"],
  ["the range panel", ".panel-range"],
  ["a statistics row", ".stat-row"],
  ["the statistics list", ".stats-body"],
  ["the board", ".panel-board"],
  ["the flops panel", ".panel-flops"],
  ["the output panel", ".panel-output"],
  ["the seat strip", ".seat"],
];

/** Which `touch-action` values still let the browser pan up and down. */
const PANS_DOWN = new Set(["auto", "manipulation", "pan-y", "pan-y pinch-zoom", "pan-up", "pan-down"]);

await drive("Leaving a finger free", async ({ browser, report, url }) => {
  for (const device of DEVICES) {
    const page = await open(browser, url, device);

    // Stacked and taller than the screen, or there is nothing to scroll and
    // nothing below says anything.
    const over = await page.evaluate(() => ({
      down: document.documentElement.scrollHeight - window.innerHeight,
      across: document.documentElement.scrollWidth - window.innerWidth,
    }));
    if (over.down <= 40) {
      report(`${device.name}: the page is not taller than the screen, so nothing here is tested`);
      await page.close();
      continue;
    }
    // Sideways is how a column ends up half off the edge with no way to tell.
    if (over.across > 0) {
      report(`${device.name}: the page is ${over.across}px wider than the screen`);
    }

    const stuck = await page.evaluate(
      (surfaces, allowed) =>
        surfaces.flatMap(([what, css]) => {
          const element = document.querySelector(css);
          if (!element) return [`${what} is not on the page to touch`];
          const wrong = [];

          // A press must leave the gesture with the browser until it is clear
          // the touch was not a scroll.
          const box = element.getBoundingClientRect();
          const press = new PointerEvent("pointerdown", {
            bubbles: true,
            cancelable: true,
            pointerType: "touch",
            pointerId: 7,
            clientX: box.left + box.width / 2,
            clientY: box.top + box.height / 2,
          });
          element.dispatchEvent(press);
          if (press.defaultPrevented) wrong.push(`${what} takes the touch on the way down`);

          for (let node = element; node; node = node.parentElement) {
            const style = getComputedStyle(node);
            const name = node.className?.split?.(" ")[0] || node.tagName.toLowerCase();
            if (!allowed.includes(style.touchAction)) {
              wrong.push(`${what} sits under ${name}, whose touch-action is ${style.touchAction}`);
            }
            // A scroll container with nothing to scroll and no chaining is a
            // box that scrolls nothing and lets nothing else scroll either.
            const scrolls = ["auto", "scroll", "overlay"].includes(style.overflowY);
            const room = node.scrollHeight - node.clientHeight;
            const chains = ["auto"].includes(style.overscrollBehaviorY);
            if (scrolls && room <= 1 && !chains && node !== document.body) {
              wrong.push(
                `${what} sits inside ${name}, which scrolls nothing and passes nothing on`,
              );
            }
            if (node === document.body) break;
          }
          return wrong;
        }),
      SURFACES,
      [...PANS_DOWN],
    );
    for (const line of new Set(stuck)) report(`${device.name}: ${line}`);

    // A wheel is not a finger, but it is the one gesture headless will drive -
    // so it at least proves the page is scrollable at all and that nothing
    // above is a false alarm about a page that was never going to move.
    const client = await page.createCDPSession();
    await page.evaluate(() => window.scrollTo(0, 0));
    await client.send("Input.synthesizeScrollGesture", {
      x: Math.round(device.width / 2),
      y: Math.round(device.height / 2),
      yDistance: -300,
      gestureSourceType: "mouse",
      speed: 2000,
    });
    await settle(page, 200);
    const moved = await page.evaluate(() => window.scrollY);
    if (moved < 40) report(`${device.name}: the page did not scroll at all, even by wheel`);
    await client.detach();

    await page.close();
  }
});
