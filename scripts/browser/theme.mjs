/**
 * Is every word on the page readable, in both colour schemes?
 *
 * A stylesheet can go wrong here in a way no test in jsdom can see: jsdom does
 * not cascade colours, so nothing there knows what a word actually comes out
 * as. Two mistakes are easy to make and invisible until somebody switches
 * theme:
 *
 *  - a control that does not inherit its colour. A `<button>` starts at the
 *    browser's own black rather than at the page's ink, so a button that never
 *    says `color` is black in both themes - fine on a white panel, gone on a
 *    dark one. That is exactly how the equity on a range tile disappeared.
 *  - a colour mixed towards a literal rather than towards a token. It reads as
 *    chosen in the theme it was written in and as a stain in the other.
 *
 * So this reads every visible word in a real browser, in both themes, works
 * out what is actually painted behind it, and reports anything a reader would
 * have to squint at. The threshold is deliberately loose: this is a check for
 * text that has gone invisible, not an accessibility audit.
 */
import { dealFlop, drive, open, settle } from "./harness.mjs";

/** Below this, the word and its background are too close to tell apart. */
const CONTRAST = 2.2;

/**
 * Marks whose job is to be nearly invisible.
 *
 * The unlit marker beside a statistic is drawn in the faintest ink there is,
 * on purpose: seventeen of them down the panel at full strength would drown
 * the one or two that are actually lit. Being quiet is the design, so it is
 * not a finding.
 */
const QUIET = [".filter-mark.mark-none"];

/**
 * Parses a computed colour into 0-255 channels and an alpha.
 *
 * Two spellings come back from a browser, and they do not agree on scale:
 * `rgb()` and `rgba()` count channels to 255, while `color(srgb r g b / a)` -
 * which is what a `color-mix` resolves to - counts them to one. Reading the
 * second as the first turns a pale wash into near-black and invents failures.
 */
function parse(css) {
  const parts = (css.match(/[\d.]+/g) ?? []).map(Number);
  if (/^color\(/.test(css)) {
    // The leading token of `color(srgb ...)` is the space name, not a number,
    // so the channels are the first three numbers and the alpha the fourth.
    const [r = 0, g = 0, b = 0, a = 1] = parts;
    return { r: r * 255, g: g * 255, b: b * 255, a };
  }
  return { r: parts[0] ?? 0, g: parts[1] ?? 0, b: parts[2] ?? 0, a: parts[3] ?? 1 };
}

/** WCAG relative luminance, which is what "how light is this" has to mean. */
function luminance({ r, g, b }) {
  const channel = (value) => {
    const unit = value / 255;
    return unit <= 0.03928 ? unit / 12.92 : ((unit + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrast(ink, ground) {
  const a = luminance(ink);
  const b = luminance(ground);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

/** Every visible word on the page, with the colour it is written in. */
function readPage(quiet) {
  const out = [];
  for (const element of document.querySelectorAll("body *")) {
    // Only leaves: a container's text is its children's, and measuring it
    // twice reports the same word against the wrong background.
    if (element.children.length > 0) continue;
    const text = (element.textContent ?? "").trim();
    if (text === "" || element.offsetParent === null) continue;
    const box = element.getBoundingClientRect();
    if (box.width === 0 || box.height === 0) continue;
    const style = getComputedStyle(element);
    if (style.visibility === "hidden" || Number(style.opacity) === 0) continue;
    if (quiet.some((selector) => element.matches(selector))) continue;

    // What is painted behind it: the backgrounds stack up until one of them is
    // opaque, and a translucent one lets the next through.
    const layers = [];
    for (let at = element; at; at = at.parentElement) {
      const paint = getComputedStyle(at).backgroundColor;
      if (!paint || paint === "rgba(0, 0, 0, 0)" || paint === "transparent") continue;
      layers.push(paint);
      // Keep going while what was found lets light through. Both spellings a
      // browser uses put the alpha last, so the last number is the test.
      const numbers = (paint.match(/[\d.]+/g) ?? []).map(Number);
      const alpha = /^color\(/.test(paint) ? (numbers[3] ?? 1) : (numbers[3] ?? 1);
      if (alpha >= 1) break;
    }
    out.push({
      what: `${element.tagName.toLowerCase()}.${element.className}`.trim().slice(0, 44),
      text: text.slice(0, 16),
      ink: style.color,
      layers,
    });
  }
  return out;
}

/** Lays the background stack over the page's own ground, nearest last. */
function ground(layers) {
  let back = { r: 255, g: 255, b: 255, a: 1 };
  for (const layer of [...layers].reverse()) {
    const over = parse(layer);
    back = {
      r: over.r * over.a + back.r * (1 - over.a),
      g: over.g * over.a + back.g * (1 - over.a),
      b: over.b * over.a + back.b * (1 - over.a),
      a: 1,
    };
  }
  return back;
}

await drive("Reading in either theme", async ({ browser, report, url }) => {
  // A pinned scheme has to be on before anything is drawn. The app's own
  // module cannot do it: it runs after the page has been painted once, and the
  // engine it waits for is the better part of a megabyte - so a reader who
  // pinned dark on a light machine would watch a white loading screen until
  // the wasm arrived. The page stamps it from storage instead, and this is
  // what says so.
  const first = await browser.newPage();
  await first.setViewport({ width: 1200, height: 800 });
  await first.emulateMediaFeatures([{ name: "prefers-color-scheme", value: "light" }]);
  await first.goto(url, { waitUntil: "domcontentloaded" });
  await first.evaluate(() => localStorage.setItem("kongzilla.theme", "dark"));
  await first.goto(url, { waitUntil: "domcontentloaded" });
  const early = await first.evaluate(() => ({
    stamp: document.documentElement.dataset.theme ?? "none",
    waiting: Boolean(document.querySelector("#app.loading")),
    ground: getComputedStyle(document.body).backgroundColor,
  }));
  if (early.stamp !== "dark") {
    report(`the loading screen is still ${early.stamp}: a pinned scheme flashes the other one`);
  }
  await first.evaluate(() => localStorage.clear());
  await first.close();

  const page = await open(browser, url, { width: 1512, height: 820 });
  // A flop and a second range, so the numbers that only appear once there is
  // something to say - equity on the tiles, most of all - are on screen.
  await dealFlop(page);
  await page.evaluate(() => document.querySelectorAll(".seat")[1].click());
  await settle(page);
  await page.evaluate(() => {
    const box = document.querySelector(".notation");
    box.dispatchEvent(new Event("focus"));
    box.value = "22+, A2s+, KTo+";
    box.dispatchEvent(new Event("blur"));
  });
  await settle(page);
  await page.evaluate(() => document.querySelectorAll(".seat")[0].click());
  await settle(page);

  const pick = async (theme) => {
    await page.evaluate((want) => {
      const button = Array.from(document.querySelectorAll("button")).find((b) =>
        /Light|Dark|Auto/.test(b.textContent ?? ""),
      );
      // The toggle rounds: system, light, dark. Press until it lands.
      for (let press = 0; press < 3; press += 1) {
        if (document.documentElement.dataset.theme === want) return;
        button?.click();
      }
    }, theme);
    await settle(page);
  };

  // Every view in the output panel, because each brings colours of its own and
  // only the one on screen can be measured.
  const tabs = await page.$$eval(".output-tab", (buttons) =>
    buttons.map((button) => button.dataset.tab),
  );

  // Both ways round, because the two halves of a theme are chosen in different
  // places: the system says one thing and the reader can pin the other. A
  // stylesheet that gets this wrong looks right until somebody disagrees with
  // their own machine - which is exactly when the colours were reported wrong.
  for (const [system, theme] of [
    ["light", "light"],
    ["light", "dark"],
    ["dark", "dark"],
    ["dark", "light"],
  ]) {
    await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: system }]);
    await pick(theme);
    const landed = await page.evaluate(() => document.documentElement.dataset.theme);
    if (landed !== theme) {
      report(`could not switch to the ${theme} theme, stuck on ${landed}`);
      continue;
    }
    // The widgets the browser draws itself follow this and nothing else.
    const scheme = await page.evaluate(() => getComputedStyle(document.documentElement).colorScheme);
    if (scheme !== theme) {
      report(
        `on a ${system} system, pinning ${theme} leaves color-scheme at "${scheme}" — scrollbars and dropdowns stay ${system}`,
      );
    }
    const said = new Set();
    for (const tab of tabs) {
      await page.evaluate((key) => {
        document.querySelector(`.output-tab[data-tab="${key}"]`)?.click();
      }, tab);
      await settle(page);
      const words = await page.evaluate(readPage, QUIET);
      if (words.length < 50) {
        report(`${system}+${theme}/${tab}: only ${words.length} words on screen`);
      }
      for (const word of words) {
        const ratio = contrast(parse(word.ink), ground(word.layers));
        if (ratio >= CONTRAST || said.has(word.what)) continue;
        said.add(word.what);
        report(
          `${system}+${theme}/${tab}: "${word.text}" on ${word.what} is ${ratio.toFixed(2)}:1 — ${word.ink} on ${word.layers[0] ?? "the page"}`,
        );
      }
    }
  }
});
