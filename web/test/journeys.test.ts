// @vitest-environment jsdom
/**
 * Journeys: what somebody actually does, in the order they do it.
 *
 * The panel tests next door check one control at a time. These check the paths
 * through the whole app - a sequence of real clicks with the state read off the
 * screen after every one of them, because most of what goes wrong in an
 * interface goes wrong between the steps rather than inside them.
 *
 * Every journey gets a fresh engine, so nothing leaks from the one before, and
 * everything is driven through the DOM. Where a number can be worked out on
 * paper it is asserted exactly: a range of `AA` on a king-high board is six
 * combinations, all of them an overpair, and if the panel says anything else
 * the panel is wrong.
 */

import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";

import { boot, chrome, mutate, restore, snapshot, state, statDefs } from "../src/store";
import { createBoardPanel } from "../src/ui/boardPanel";
import { dismissOne, installDismiss } from "../src/ui/dismiss";
import { createFlopsPanel } from "../src/ui/flopsPanel";
import { createHotkeySheet, installHotkeys } from "../src/ui/hotkeys";
import { createMenuBar } from "../src/ui/menubar";
import { createOutputPanel } from "../src/ui/outputPanel";
import { createRangePanel } from "../src/ui/rangePanel";
import { createStatsPanel } from "../src/ui/statsPanel";
import { createTopStrip } from "../src/ui/topStrip";
import { loadColumns } from "../src/ui/workspace";
import { wasmBytes } from "./harness";

/** Everything on screen, plus the ways a journey drives it. */
interface App {
  render: () => void;
  menubar: HTMLElement;
  strip: HTMLElement;
  range: HTMLElement;
  board: HTMLElement;
  stats: HTMLElement;
  output: HTMLElement;
  flops: HTMLElement;
  sheet: HTMLElement;
  press: (key: string, init?: KeyboardEventInit) => void;
  teardown: () => void;
}

let bytes: Buffer;
let app: App | null = null;

beforeAll(async () => {
  bytes = await wasmBytes();
});

afterEach(() => {
  app?.teardown();
  app = null;
});

/**
 * A new session with everything wired, as the page does it.
 *
 * `boot` builds a fresh engine, so each journey starts where a reader starting
 * the app starts: the button opening range, no board, nothing painted.
 */
async function open(): Promise<App> {
  // Anything a previous journey left in the chrome is not part of a fresh start.
  Object.assign(chrome, {
    hovered: null,
    editing: null,
    suitCell: null,
    suitPeek: null,
    output: "groups",
    showCombos: false,
    brush: 1,
    continueShare: 1,
    cut: null,
    flopsOpen: true,
    dealtBucket: null,
    preflop: null,
    preflopRunning: false,
    boardCards: [],
    visible: 0,
  });
  // The app writes the session into the address as you work, and `boot` reads
  // it back. A fresh visit has no address to read, so neither does a fresh
  // journey - otherwise each one starts inside the last one.
  window.location.hash = "";
  await boot(bytes);
  loadColumns();

  const menubar = createMenuBar();
  const strip = createTopStrip();
  const range = createRangePanel();
  const board = createBoardPanel();
  const stats = createStatsPanel();
  const output = createOutputPanel();
  const flops = createFlopsPanel();
  const sheet = createHotkeySheet();
  const panels = [menubar, strip, range, board, stats, output, flops];
  document.body.replaceChildren(...panels.map((panel) => panel.element), sheet.element);

  const render = () => panels.forEach((panel) => panel.render());
  const stopDismiss = installDismiss();
  const uninstall = installHotkeys({
    randomBoard: board.randomBoard,
    stepStreet: board.stepStreet,
    toggleSheet: sheet.toggle,
    // The app's own ladder, not a copy of it: a copy is a thing that drifts.
    escape: () => dismissOne(sheet.close),
    say: () => {},
    actions: menubar.actions,
  });
  render();

  app = {
    render,
    menubar: menubar.element,
    strip: strip.element,
    range: range.element,
    board: board.element,
    stats: stats.element,
    output: output.element,
    flops: flops.element,
    sheet: sheet.element,
    press: (key, init = {}) => {
      window.dispatchEvent(new window.KeyboardEvent("keydown", { key, bubbles: true, ...init }));
      render();
    },
    teardown: () => {
      uninstall();
      stopDismiss();
      document.body.replaceChildren();
    },
  };
  return app;
}

/* ----- reading the screen -------------------------------------------------- */

/** Types a range into the box, the way a reader does. */
function type(app: App, text: string): void {
  const notation = app.range.querySelector<HTMLTextAreaElement>(".notation")!;
  notation.dispatchEvent(new window.Event("focus"));
  notation.value = text;
  notation.dispatchEvent(new window.Event("blur"));
  app.render();
}

/** Clicks a card in one of the 4x13 grids. */
function card(where: HTMLElement, name: string): void {
  where.querySelector<HTMLButtonElement>(`.card-cell[data-card="${name}"]`)!.click();
  app!.render();
}

/** One statistics row, by the name printed on it. */
function row(app: App, label: string): HTMLElement {
  const found = Array.from(app.stats.querySelectorAll<HTMLElement>(".stat-row")).find(
    (candidate) => candidate.querySelector(".stat-label")?.textContent === label,
  );
  if (!found) throw new Error(`no statistic called ${label}`);
  return found;
}

/** What a row reports: its marker, its number, and whether it is greyed. */
function reading(app: App, label: string): { mark: string; value: string; empty: boolean } {
  const element = row(app, label);
  return {
    mark: element.querySelector(".filter-mark")!.className.replace("filter-mark mark-", ""),
    value: element.querySelector(".stat-value")!.textContent ?? "",
    empty: element.classList.contains("empty"),
  };
}

/** One matrix cell, by the hand written on it. */
function cell(app: App, label: string): HTMLElement {
  const found = Array.from(app.range.querySelectorAll<HTMLElement>(".cell")).find(
    (candidate) => candidate.querySelector(".cell-label")?.textContent === label,
  );
  if (!found) throw new Error(`no cell called ${label}`);
  return found;
}

/** How a cell is drawn: in the range, how much of it, and how much got through. */
function drawn(
  app: App,
  label: string,
): {
  on: boolean;
  fill: string;
  filtered: boolean;
  passing: string;
  grouped: boolean;
  count: string;
} {
  const element = cell(app, label);
  return {
    on: element.classList.contains("on"),
    fill: element.style.getPropertyValue("--fill"),
    filtered: element.classList.contains("filtered"),
    passing: element.style.getPropertyValue("--passing"),
    grouped: element.classList.contains("grouped"),
    count: element.querySelector(".cell-count")?.textContent ?? "",
  };
}

/** Hovers a cell and returns the suit breakdown it opens. */
function peek(app: App, label: string): HTMLElement {
  cell(app, label).dispatchEvent(new window.Event("pointerenter", { bubbles: false }));
  app.render();
  return app.range.querySelector<HTMLElement>(".suit-popup")!;
}

/** Pins a cell's breakdown open, the way shift-clicking it does. */
function pin(app: App, label: string): HTMLElement {
  cell(app, label).dispatchEvent(
    new window.MouseEvent("pointerdown", { bubbles: true, shiftKey: true }),
  );
  app.render();
  return app.range.querySelector<HTMLElement>(".suit-popup")!;
}

/** How the breakdown draws each combination, by name. */
function suits(popup: HTMLElement): Map<
  string,
  {
    on: boolean;
    fill: string;
    filtered: boolean;
    passing: string;
    colour: string;
    dealt: boolean;
    lit: boolean;
  }
> {
  const found = new Map<
    string,
    {
      on: boolean;
      fill: string;
      filtered: boolean;
      passing: string;
      colour: string;
      dealt: boolean;
      lit: boolean;
    }
  >();
  for (const element of popup.querySelectorAll<HTMLElement>(".suit-cell:not(.absent)")) {
    found.set(element.dataset.name!, {
      on: element.classList.contains("on"),
      fill: element.style.getPropertyValue("--fill"),
      filtered: element.classList.contains("filtered"),
      passing: element.style.getPropertyValue("--passing"),
      colour: element.dataset.colour ?? "none",
      dealt: element.classList.contains("dealt"),
      lit: element.classList.contains("lit"),
    });
  }
  return found;
}

/** Clicks one combination in the breakdown. */
function suitClick(popup: HTMLElement, name: string): void {
  const element = popup.querySelector<HTMLElement>(`.suit-cell[data-name="${name}"]`);
  if (!element) throw new Error(`no ${name} in the breakdown`);
  element.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true }));
  window.dispatchEvent(new window.Event("pointerup"));
  app!.render();
}

/** How much of a cell the hover band covers, as a fraction. */
function glow(element: HTMLElement): number {
  return Number(element.style.getPropertyValue("--glow").replace("%", "")) / 100;
}

/** The suit pips drawn on a cell, as one string. */
function pips(app: App, label: string): string {
  return Array.from(cell(app, label).querySelectorAll(".cell-suits .pip"))
    .map((pip) => pip.textContent)
    .join("");
}

/** The buttons in a panel's header. */
/** The seat tiles in the top strip. */
function seats2(app: App): HTMLButtonElement[] {
  return Array.from(app.strip.querySelectorAll<HTMLButtonElement>(".seat"));
}

/** A button in a panel's header, by the word on it. */
function headButton(panel: HTMLElement, label: string): HTMLButtonElement {
  const found = headButtons(panel).find((button) => button.textContent === label);
  if (!found) throw new Error(`no ${label} button in the header`);
  return found;
}

function headButtons(panel: HTMLElement): HTMLButtonElement[] {
  return Array.from(panel.querySelectorAll<HTMLButtonElement>(".panel-head .btn"));
}

/** The street filter buttons that are on screen. */
function streets(app: App): HTMLButtonElement[] {
  return Array.from(app.stats.querySelectorAll<HTMLButtonElement>(".street-filter")).filter(
    (button) => !button.hidden,
  );
}

/** A palette swatch. */
function swatch(app: App, colour: string): HTMLButtonElement {
  return app.stats.querySelector<HTMLButtonElement>(`.swatch.colour-${colour}`)!;
}

/** An output tab. */
function tab(app: App, key: string): HTMLButtonElement {
  return app.output.querySelector<HTMLButtonElement>(`.output-tab[data-tab="${key}"]`)!;
}

/** A library chip, by its row and label. */
function chip(app: App, game: string, row: string, label: string): HTMLButtonElement {
  const found = Array.from(
    app.range.querySelectorAll<HTMLButtonElement>(`.library-${game} .action-${row} .seat-chip`),
  ).find((candidate) => candidate.textContent === label);
  if (!found) throw new Error(`no ${label} chip in ${game}/${row}`);
  return found;
}

/** A stack chip. */
function stack(app: App, game: string, label: string): HTMLButtonElement {
  const found = Array.from(
    app.range.querySelectorAll<HTMLButtonElement>(`.library-${game} .stack-chip`),
  ).find((candidate) => candidate.textContent === label);
  if (!found) throw new Error(`no ${label} stack chip in ${game}`);
  return found;
}

/**
 * Everything on screen, read off the screen, with the invariants checked.
 *
 * The numbers in four panels are four views of one calculation, and the way
 * they go wrong is by disagreeing: a button counting its own frozen set while
 * the footer counts what survived every filter, and both looking plausible on
 * their own. So they are read together and cross-checked, rather than each
 * being compared against a constant somebody typed into a test.
 */
function audit(app: App): {
  total: number;
  pass: number | null;
  streets: number[];
  rows: Map<string, number>;
  pie: number;
  cells: number;
  filtersOn: boolean;
} {
  const view = state();
  const footer = app.stats.querySelector(".effective")!.textContent ?? "";
  const total = Number(footer.match(/Total number of combos: ([\d.]+)/)![1]);
  const passMatch = footer.match(/Combos that pass the filters: ([\d.]+)/);
  const pass = passMatch ? Number(passMatch[1]) : null;
  const onOff = footer.match(/The filters are (ON|OFF)/)![1];

  // The footer's total is the range the board and the dead cards have left.
  expect(total).toBeCloseTo(view.liveCombos, 1);
  expect(app.board.querySelector(".tally")!.textContent).toContain(formatted(view.liveCombos));
  expect(onOff === "ON").toBe(view.filtersEnabled);
  expect(pass === null).toBe(!view.filtersEnabled);

  // The street buttons read down as a chain, and the deepest one that is on is
  // the same number the footer calls the survivors.
  const buttons = streets(app);
  const counts = buttons.map((button) => Number(button.textContent!.match(/([\d.]+)$/)![1]));
  for (let index = 1; index < counts.length; index += 1) {
    expect(counts[index], `street ${index} cannot hold more than ${index - 1}`).toBeLessThanOrEqual(
      counts[index - 1] + 1e-6,
    );
  }
  const deepest = buttons.reduce(
    (found, button, index) =>
      button.querySelector(".lamp")!.classList.contains("on") ? index : found,
    -1,
  );
  if (pass !== null) {
    expect(deepest, "the filters are on, so some street must be lit").toBeGreaterThanOrEqual(0);
    expect(counts[deepest], "the button and the footer are one fact said twice").toBeCloseTo(
      pass,
      1,
    );
  }

  // Every combination has exactly one made rung, so in absolute mode that block
  // adds up to the whole of what the panel is about.
  const rows = new Map<string, number>();
  let made = 0;
  app.press("Tab"); // combinations rather than percentages
  for (const definition of statDefs) {
    const element = row(app, definition.label);
    if (element.hidden) continue;
    const value = Number(element.querySelector(".stat-value")!.textContent);
    rows.set(definition.label, value);
    if (definition.block === "made") made += value;
    expect(value, definition.label).toBeLessThanOrEqual(total + 1e-6);
  }
  app.press("Tab");
  if (view.mode === "absolute" && total > 0) {
    const panelTotal = pass ?? total;
    expect(made, "the made-hand block is a partition").toBeCloseTo(panelTotal, 1);
  }

  // The pie is of what the filters left, and its slices add up to it.
  const pie = view.groupShares.reduce((sum, share) => sum + share, 0);
  expect(pie).toBeCloseTo(pass ?? total, 1);

  // And the cells hold the range they are drawn from.
  const cells = view.classCombos.reduce((sum, count) => sum + count, 0);
  expect(cells).toBeCloseTo(view.players[view.active].combos, 1);

  return { total, pass, streets: counts, rows, pie, cells, filtersOn: view.filtersEnabled };
}

/** The way the panels print a combination count. */
function formatted(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

/* ----- the journeys -------------------------------------------------------- */

describe("opening the app", () => {
  test("lands on a button opening range with nothing else assumed", async () => {
    const app = await open();

    // The range is loaded and the chip that loaded it is lit.
    expect(state().board).toBe("");
    expect(state().players[state().active].combos).toBeCloseTo(720, 0);
    expect(app.range.querySelector(".summary")!.textContent).toMatch(/720 combos in range/);
    expect(chip(app, "mtt", "open", "BTN").classList.contains("active")).toBe(true);

    // Preflop: the palette and the street filters have nothing to act on.
    expect(app.stats.classList.contains("preflop-mode")).toBe(true);
    expect(app.stats.querySelector<HTMLElement>(".palette")!.hidden).toBe(true);
    expect(streets(app)).toHaveLength(0);
    expect(app.board.querySelector(".tally")!.textContent).toMatch(/Pick a flop/);

    // And the deck-wide panel is there whatever else is.
    expect(app.flops.textContent).toMatch(/22,100 flops/);
    expect(app.flops.querySelectorAll(".flop-row").length).toBeGreaterThan(10);
  });
});

describe("numbers you can check on paper", () => {
  test("a single pair of aces on a king-high board", async () => {
    const app = await open();
    type(app, "AA");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");

    // Six combinations, none of them blocked by the board.
    expect(state().players[state().active].combos).toBe(6);
    expect(state().liveCombos).toBe(6);
    expect(state().players[state().active].percent).toBeCloseTo((6 / 1326) * 100, 4);
    expect(app.board.querySelector(".tally")!.textContent).toBe("6 combos, none filtered out yet.");

    // Every one of them is an overpair and nothing else.
    expect(reading(app, "overpair")).toMatchObject({ value: "100.0%", empty: false });
    for (const label of ["top pair", "set", "two pair", "flushdraw", "no made hand"]) {
      expect(reading(app, label), label).toMatchObject({ value: "0.0%", empty: true });
    }

    // In combinations rather than percentages, the same thing said differently.
    app.press("Tab");
    expect(reading(app, "overpair").value).toBe("6");
    app.press("Tab");

    // One cell, wholly in the range, and it is the pair on the diagonal.
    expect(drawn(app, "AA")).toMatchObject({ on: true, fill: "100.0%", count: "6" });
    expect(cell(app, "AA").classList.contains("pair")).toBe(true);
    expect(drawn(app, "AKs").on).toBe(false);
  });

  test("the board blocks half of a pair of kings", async () => {
    const app = await open();
    type(app, "KK");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");

    // Six in the range, three of them impossible: the king of hearts is out.
    expect(state().players[state().active].combos).toBe(6);
    expect(state().liveCombos).toBe(3);
    expect(reading(app, "set")).toMatchObject({ value: "100.0%" });
    app.press("Tab");
    expect(reading(app, "set").value).toBe("3");
    app.press("Tab");

    // The cell still says six, because the range still holds six.
    expect(drawn(app, "KK").count).toBe("6");
  });

  test("two pairs at once split the panel in a way that adds up", async () => {
    const app = await open();
    type(app, "AA,KK");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");

    // Nine live: six aces and three kings.
    expect(state().liveCombos).toBe(9);
    expect(reading(app, "overpair").value).toBe("66.7%");
    expect(reading(app, "set").value).toBe("33.3%");
    app.press("Tab");
    expect(reading(app, "overpair").value).toBe("6");
    expect(reading(app, "set").value).toBe("3");
    app.press("Tab");

    // The default grouping takes both, so everything passes the flop filter.
    expect(reading(app, "overpair").mark).toBe("blue");
    expect(reading(app, "set").mark).toBe("blue");
    streets(app)[0].click();
    app.render();
    expect(state().passFraction).toBeCloseTo(1, 9);
    expect(app.stats.querySelector(".effective")!.textContent).toMatch(
      /Combos that pass the filters: 9 \(100\.00%\)/,
    );
  });

  test("one suited hand, one heart combination, and a quarter of a cell", async () => {
    const app = await open();
    // All four AQs, on a board with two hearts: exactly one of them is a
    // flushdraw, and it is the one holding both remaining hearts.
    type(app, "AQs");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");

    expect(state().liveCombos).toBe(4);
    expect(reading(app, "flushdraw").value).toBe("25.0%");
    expect(reading(app, "ace high").value).toBe("100.0%");

    // Paint only the flushdraw, and the filter keeps exactly that one hand.
    headButton(app.stats, "Clear").click();
    app.render();
    row(app, "flushdraw").click();
    app.render();
    expect(reading(app, "flushdraw").mark).toBe("blue");

    streets(app)[0].click();
    app.render();
    expect(state().passFraction).toBeCloseTo(0.25, 9);
    expect(app.stats.querySelector(".effective")!.textContent).toMatch(
      /Combos that pass the filters: 1 \(25\.00%\)/,
    );

    // A quarter of the cell got through, drawn as a quarter of its width.
    expect(drawn(app, "AQs")).toMatchObject({
      on: true,
      fill: "100.0%",
      filtered: true,
      passing: "25.0%",
    });
    // One colour in use, so no colour mark is drawn on top of it.
    expect(state().coloursUsed).toBe(1);
    expect(drawn(app, "AQs").grouped).toBe(false);
  });

  test("a fractional weight is half a cell high", async () => {
    const app = await open();
    type(app, "AA,KK:0.5");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");

    // Six aces at full weight and six kings at half: nine combinations.
    expect(state().players[state().active].combos).toBeCloseTo(9, 6);
    expect(drawn(app, "AA").fill).toBe("100.0%");
    expect(drawn(app, "KK").fill).toBe("50.0%");
    expect(drawn(app, "KK").count).toBe("3");

    // And the weight follows into the statistics: 6 aces against 1.5 kings.
    expect(state().liveCombos).toBeCloseTo(7.5, 6);
    expect(reading(app, "overpair").value).toBe("80.0%");
    expect(reading(app, "set").value).toBe("20.0%");
  });
});

describe("drawing on the matrix", () => {
  test("painting, weighting and reading a range back as text", async () => {
    const app = await open();
    app.range.querySelector<HTMLButtonElement>(".quick:last-of-type")!.click(); // Clear
    app.render();
    expect(state().players[state().active].combos).toBe(0);
    expect(app.range.querySelectorAll(".cell.on")).toHaveLength(0);

    // One cell, at full weight: six combinations and the notation to match.
    cell(app, "AA").dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true }));
    window.dispatchEvent(new window.Event("pointerup"));
    app.render();
    expect(state().players[state().active].notation).toBe("AA");
    expect(state().players[state().active].combos).toBe(6);
    expect(drawn(app, "AA")).toMatchObject({ on: true, fill: "100.0%", count: "6" });

    // Half weight on the next one, off the brush buttons.
    Array.from(app.range.querySelectorAll<HTMLButtonElement>(".brush"))
      .find((button) => button.textContent === "50%")!
      .click();
    app.render();
    expect(chrome.brush).toBe(0.5);
    cell(app, "KK").dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true }));
    window.dispatchEvent(new window.Event("pointerup"));
    app.render();
    expect(state().players[state().active].notation).toBe("AA,KK:0.5");
    expect(state().players[state().active].combos).toBeCloseTo(9, 6);
    expect(drawn(app, "KK").fill).toBe("50.0%");

    // Up and down on the keyboard move the brush in fives.
    app.press("ArrowUp");
    expect(chrome.brush).toBeCloseTo(0.55, 6);
    app.press("ArrowDown");
    expect(chrome.brush).toBeCloseTo(0.5, 6);

    // Clicking a cell that is already painted takes it out again.
    cell(app, "AA").dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true }));
    window.dispatchEvent(new window.Event("pointerup"));
    app.render();
    expect(state().players[state().active].notation).toBe("KK:0.5");
  });

  test("hovering a hand opens its suits, and moving off closes them", async () => {
    const app = await open();
    type(app, "AKs");
    expect(state().players[state().active].combos).toBe(4);

    // Nothing is open until the pointer arrives.
    expect(app.range.querySelector<HTMLElement>(".suit-popup")!.hidden).toBe(true);

    const popup = peek(app, "AKs");
    expect(popup.hidden).toBe(false);
    expect(popup.querySelector(".suit-popup-head")!.textContent).toContain("AKs");

    // Four combinations, one per suit, all of them entirely in the range.
    const held = suits(popup);
    expect([...held.keys()].sort()).toEqual(["AcKc", "AdKd", "AhKh", "AsKs"]);
    for (const [name, drawing] of held) {
      expect(drawing.on, name).toBe(true);
      expect(drawing.fill, name).toBe("100.0%");
      expect(drawing.filtered, name).toBe(false);
      expect(drawing.dealt, name).toBe(false);
    }

    // And the cell says nothing about suits, because it holds all of them.
    expect(pips(app, "AKs")).toBe("");

    // Leaving the matrix closes it again.
    app.range
      .querySelector<HTMLElement>(".matrix")!
      .dispatchEvent(new window.Event("pointerleave"));
    app.render();
    expect(app.range.querySelector<HTMLElement>(".suit-popup")!.hidden).toBe(true);
  });

  test("the brush takes one suit out of a hand, and the cell says which are left", async () => {
    const app = await open();
    type(app, "AKs");

    // Pinned, so the pointer can leave the cell for the popup.
    const popup = pin(app, "AKs");
    expect(popup.classList.contains("pinned")).toBe(true);
    expect(popup.querySelector(".suit-popup-hint")!.textContent).toContain("close");
    expect(popup.querySelector(".suit-close")).not.toBeNull();

    // Clicking a held combination takes it out, and the count is in
    // combinations rather than cells.
    suitClick(popup, "AhKh");
    expect(state().players[state().active].combos).toBe(3);
    expect(drawn(app, "AKs").count).toBe("3");
    expect(drawn(app, "AKs").fill).toBe("75.0%");
    expect(suits(popup).get("AhKh")!.on).toBe(false);

    // Three suits out of four, so the cell names them - and not the one gone.
    expect(pips(app, "AKs")).toBe("♠♦♣");

    // Put it back and the cell goes quiet again: all four is no information.
    suitClick(popup, "AhKh");
    expect(state().players[state().active].combos).toBe(4);
    expect(pips(app, "AKs")).toBe("");

    // A lighter brush paints a fraction of one combination.
    app.range.querySelectorAll<HTMLButtonElement>(".brush")[2].click();
    app.render();
    suitClick(popup, "AsKs"); // in at 100%, so this takes it out
    suitClick(popup, "AsKs"); // and this puts it back at the brush
    expect(suits(popup).get("AsKs")!.fill).toBe("50.0%");
    expect(state().players[state().active].combos).toBeCloseTo(3.5, 6);
    // Still no pips: the pips say which suits are there, and all four are. How
    // much of one is there is what the fill in the breakdown is for.
    expect(pips(app, "AKs")).toBe("");
  });

  test("the breakdown greys the suits a filter took", async () => {
    const app = await open();
    type(app, "AKs");
    card(app.board, "Qs");
    card(app.board, "7s");
    card(app.board, "2d");

    // One of the four makes a flushdraw: the two spades.
    expect(reading(app, "flushdraw").value).toBe("25.0%");

    row(app, "flushdraw").click();
    app.render();
    streets(app)[0].click();
    app.render();

    const popup = peek(app, "AKs");
    const held = suits(popup);
    // AsKs is the hand that continues; the other three are in the range and
    // greyed, which is the same story the cell tells at a coarser grain.
    expect(held.get("AsKs")!.filtered).toBe(false);
    expect(held.get("AsKs")!.passing).toBe("100.0%");
    for (const name of ["AhKh", "AdKd", "AcKc"]) {
      expect(held.get(name)!.on, name).toBe(true);
      expect(held.get(name)!.filtered, name).toBe(true);
      expect(held.get(name)!.passing, name).toBe("0.0%");
    }

    // One suit of the four left standing, and the cell names it.
    expect(pips(app, "AKs")).toBe("♠");
    expect(drawn(app, "AKs").filtered).toBe(true);
    expect(audit(app).pass).toBeCloseTo(1, 6);
  });

  test("a card on the board takes a suit out of the breakdown", async () => {
    const app = await open();
    type(app, "AKs");
    card(app.board, "Kh");
    card(app.board, "8d");
    card(app.board, "3c");

    // The king of hearts takes one of the four off the table: the range still
    // says four, and three of them can actually be held.
    expect(state().players[state().active].combos).toBe(4);
    expect(state().liveCombos).toBe(3);
    const popup = peek(app, "AKs");
    expect(suits(popup).get("AhKh")!.dealt).toBe(true);
    expect(suits(popup).get("AsKs")!.dealt).toBe(false);

    // Holding all three that remain is holding all of them, so no pips: the
    // cell must not read as narrowed by a card removal it did not choose.
    expect(pips(app, "AKs")).toBe("");

    // Dropping one of the three does read as a choice.
    pin(app, "AKs");
    suitClick(popup, "AsKs");
    expect(pips(app, "AKs")).toBe("♦♣");
    expect(state().liveCombos).toBe(2);
    audit(app);
  });

  test("a hovered statistic lights the share of the cell it is about", async () => {
    const app = await open();
    type(app, "AKs,QJo");
    card(app.board, "Ts");
    card(app.board, "4s");
    card(app.board, "2h");

    // AKs: one of its four is a flushdraw. QJo: none of its twelve are.
    row(app, "flushdraw").dispatchEvent(new window.Event("pointerenter"));
    app.render();
    const suited = cell(app, "AKs");
    expect(suited.classList.contains("lit")).toBe(true);
    // A quarter of the cell, at full strength, rather than the whole of it at a
    // quarter strength: the band is the number.
    expect(glow(suited)).toBeCloseTo(0.25, 6);
    expect(cell(app, "QJo").classList.contains("lit")).toBe(false);

    // The breakdown marks the same hand, at the finer grain.
    const popup = peek(app, "AKs");
    expect(suits(popup).get("AsKs")!.lit).toBe(true);
    expect(suits(popup).get("AhKh")!.lit).toBe(false);

    // A row every combination matches lights the whole cell.
    row(app, "flushdraw").dispatchEvent(new window.Event("pointerleave"));
    row(app, "no made hand").dispatchEvent(new window.Event("pointerenter"));
    app.render();
    expect(glow(cell(app, "QJo"))).toBeCloseTo(1, 6);
  });

  test("the shift offer is made in words, not in a glyph", async () => {
    const app = await open();
    type(app, "AKs");

    // A lone arrow says a key is involved and not which one or what it does.
    const hint = row(app, "flushdraw").querySelector<HTMLElement>(".shift-hint")!;
    expect(hint.textContent).toBe("⇧-click for combos");
    // And it is not a control: it must never eat the click it advertises.
    expect(hint.getAttribute("aria-hidden")).toBe("true");

    // The matrix cell makes no separate offer, because hovering it opens the
    // breakdown and the breakdown says the same thing properly.
    expect(cell(app, "AKs").querySelector(".shift-hint")).toBeNull();
    expect(peek(app, "AKs").querySelector(".suit-popup-hint")!.textContent).toContain("⇧-click");
  });

  test("the breakdown shows the colours a hand is painted", async () => {
    const app = await open();
    type(app, "AKs");
    card(app.board, "Qs");
    card(app.board, "7s");
    card(app.board, "2d");

    swatch(app, "green").click();
    app.render();
    row(app, "flushdraw").click();
    app.render();

    const popup = peek(app, "AKs");
    // The flushdraw is the spade hand, so that is the one wearing the colour.
    expect(suits(popup).get("AsKs")!.colour).toBe("green");
    expect(suits(popup).get("AhKh")!.colour).toBe("none");
  });

  test("the breakdown answers about the range, not about the deck", async () => {
    const app = await open();
    type(app, "AsKs");
    card(app.board, "Qs");
    card(app.board, "7s");
    card(app.board, "2d");

    swatch(app, "green").click();
    app.render();
    row(app, "flushdraw").click();
    app.render();
    row(app, "flushdraw").dispatchEvent(new window.Event("pointerenter"));
    app.render();

    const popup = peek(app, "AKs");
    const held = suits(popup);
    // The one hand in the range is lit and coloured.
    expect(held.get("AsKs")).toMatchObject({ on: true, lit: true, colour: "green" });
    // The other three would be flushdraws too, and are painted green by the
    // same category - but the range does not hold them, so the breakdown says
    // nothing about them. Lighting them would claim hands that are not there.
    for (const name of ["AhKh", "AdKd", "AcKc"]) {
      expect(held.get(name), name).toMatchObject({ on: false, lit: false, colour: "none" });
    }
  });

  test("a pair opens into a triangle and a suited hand into a row", async () => {
    const app = await open();
    type(app, "QQ,AKs");

    // Six combinations for a pair, laid out on two suit axes: there is no such
    // hand as two queens of spades, so the diagonal is empty.
    const pair = peek(app, "QQ");
    expect(pair.querySelectorAll(".suit-cell:not(.absent)")).toHaveLength(6);
    expect(pair.querySelector(".suit-grid")!.classList.contains("one-row")).toBe(false);
    const names = [...suits(pair).keys()];
    expect(names.every((name) => name[1] !== name[3])).toBe(true);

    // A suited hand has one suit per combination, so it gets one row rather
    // than a grid with twelve empty slots in it.
    const suited = peek(app, "AKs");
    expect(suited.querySelector(".suit-grid")!.classList.contains("one-row")).toBe(true);
    expect(suited.querySelectorAll(".suit-cell:not(.absent)")).toHaveLength(4);
    expect(suited.querySelectorAll(".suit-cell.absent")).toHaveLength(0);
    expect([...suits(suited).keys()].every((name) => name[1] === name[3])).toBe(true);
  });

  test("escape closes a pinned breakdown before anything else", async () => {
    const app = await open();
    type(app, "AKs");
    const popup = pin(app, "AKs");
    expect(popup.hidden).toBe(false);

    app.press("Escape");
    app.render();
    expect(app.range.querySelector<HTMLElement>(".suit-popup")!.hidden).toBe(true);
  });

  test("the percentage slider takes a band off the top of the ranking", async () => {
    const app = await open();
    const percent = app.range.querySelector<HTMLInputElement>(".percent")!;

    percent.value = "10";
    percent.dispatchEvent(new window.Event("change", { bubbles: true }));
    app.render();
    const ten = state().players[state().active].percent;
    expect(ten).toBeGreaterThan(8);
    expect(ten).toBeLessThan(12);
    // The strongest hands are the ones in it.
    expect(drawn(app, "AA").on).toBe(true);
    expect(drawn(app, "72o").on).toBe(false);

    percent.value = "40";
    percent.dispatchEvent(new window.Event("change", { bubbles: true }));
    app.render();
    expect(state().players[state().active].percent).toBeGreaterThan(ten * 3);
    expect(drawn(app, "AA").on).toBe(true);
  });

  test("dragging left off a pair of stacked handles pulls the red one out", async () => {
    const app = await open();
    const track = app.range.querySelector<HTMLElement>(".slider-track")!;
    const cut = app.range.querySelector<HTMLInputElement>(".slider-cut")!;
    const top = app.range.querySelector<HTMLInputElement>(".slider-top")!;

    // jsdom has no layout, so the track is measured for the test.
    track.getBoundingClientRect = () => ({ left: 0, width: 211, top: 0, height: 18 }) as DOMRect;

    // Park both handles on the same value: an empty band, nothing selected.
    const percent = app.range.querySelector<HTMLInputElement>(".percent")!;
    percent.value = "0";
    percent.dispatchEvent(new window.Event("change", { bubbles: true }));
    top.value = "30";
    top.dispatchEvent(new window.Event("input", { bubbles: true }));
    cut.value = "30";
    cut.dispatchEvent(new window.Event("input", { bubbles: true }));
    app.render();
    expect([cut.value, top.value]).toEqual(["30", "30"]);
    expect(state().players[state().active].combos).toBe(0);

    // Press on the stack and pull left. The blue handle cannot go left - the
    // end of the band may not pass its start - so the red one has to come out.
    const at = (value: number) => 5.5 + (value / 100) * 200;
    track.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, clientX: at(30) }));
    track.dispatchEvent(new window.MouseEvent("pointermove", { bubbles: true, clientX: at(12) }));
    app.render();
    expect(cut.value).toBe("12");
    expect(top.value).toBe("30");

    // Which is a real band now, off the top of the ranking.
    const band = state().players[state().active].percent;
    expect(band).toBeGreaterThan(15);
    expect(band).toBeLessThan(20);
    expect(drawn(app, "AA").on).toBe(false);

    // The gesture stays with the handle it picked: dragging back past the other
    // handle must not hand the drag over mid-pull.
    track.dispatchEvent(new window.MouseEvent("pointermove", { bubbles: true, clientX: at(44) }));
    app.render();
    expect(top.value).toBe("30");
    expect(cut.value).toBe("30");
    track.dispatchEvent(new window.MouseEvent("pointerup", { bubbles: true, clientX: at(44) }));

    // And pulling right off a stack takes the blue one instead.
    track.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, clientX: at(30) }));
    track.dispatchEvent(new window.MouseEvent("pointermove", { bubbles: true, clientX: at(65) }));
    app.render();
    expect(top.value).toBe("65");
    expect(cut.value).toBe("30");
    track.dispatchEvent(new window.MouseEvent("pointerup", { bubbles: true, clientX: at(65) }));
  });

  test("the value buttons add the ranges written on them", async () => {
    const app = await open();
    const value = (label: string) => {
      const found = Array.from(
        app.range.querySelectorAll<HTMLButtonElement>(".value-row .quick"),
      ).find((button) => button.textContent === label);
      if (!found) throw new Error(`no ${label} button`);
      return found;
    };

    // Three of them, and they sit on their own line rather than among the
    // buttons that select a shape of the matrix.
    expect(app.range.querySelectorAll(".value-row .quick")).toHaveLength(3);

    type(app, "");
    value("QQ+/AK").click();
    app.render();
    // Three pairs at six, plus both halves of ace-king: 18 + 16.
    expect(state().players[state().active].combos).toBe(34);
    expect(state().players[state().active].notation).toBe("QQ+,AKs,AKo");
    expect(drawn(app, "QQ").count).toBe("6");
    expect(drawn(app, "AKo").count).toBe("12");
    expect(drawn(app, "JJ").on).toBe(false);

    // The buttons add, the way the shape buttons do, so two of them together
    // are the wider of the two rather than the second one alone.
    value("TT+/AQ+").click();
    app.render();
    expect(state().players[state().active].combos).toBe(62);
    expect(state().players[state().active].notation).toBe("TT+,AQs+,AQo+");

    type(app, "");
    value("99+/AJ+/KQ").click();
    app.render();
    // Six pairs, three ace-highs and king-queen: 36 + 48 + 16.
    expect(state().players[state().active].combos).toBe(100);
    expect(drawn(app, "99").on).toBe(true);
    expect(drawn(app, "88").on).toBe(false);
    expect(drawn(app, "KQs").on).toBe(true);
    expect(drawn(app, "KJs").on).toBe(false);
  });
});

describe("hovering the statistics", () => {
  test("lights the matrix and re-reads every other row", async () => {
    const app = await open();
    type(app, "AA,KK,AQs");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");

    // Nothing hovered: the panel is about the whole range.
    expect(app.range.querySelectorAll(".cell.lit")).toHaveLength(0);
    expect(reading(app, "overpair").value).toBe("46.2%");

    // Hover the flushdraw: only AhQh has one, and the panel says so.
    row(app, "flushdraw").dispatchEvent(new window.MouseEvent("pointerenter", { bubbles: true }));
    app.render();
    expect(chrome.hovered).not.toBeNull();
    expect(reading(app, "flushdraw").value).toBe("100.0%");
    expect(reading(app, "overpair").value).toBe("0.0%");
    const lit = Array.from(app.range.querySelectorAll<HTMLElement>(".cell.lit"));
    expect(lit.map((element) => element.querySelector(".cell-label")!.textContent)).toEqual([
      "AQs",
    ]);
    // A quarter of that cell is the flushdraw, and the band says so: the glow
    // covers the matching share of the cell rather than tinting the whole of it.
    expect(glow(lit[0])).toBeCloseTo(0.25, 3);

    row(app, "flushdraw").dispatchEvent(new window.MouseEvent("pointerleave", { bubbles: true }));
    app.render();
    expect(chrome.hovered).toBeNull();
    expect(app.range.querySelectorAll(".cell.lit")).toHaveLength(0);
    expect(reading(app, "overpair").value).toBe("46.2%");
  });

  test("absolute against cumulative changes what a rung means", async () => {
    const app = await open();
    type(app, "AA,KK,QQ");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");

    // Exactly this: six aces over fifteen live.
    expect(reading(app, "overpair").value).toBe("40.0%");
    expect(reading(app, "set").value).toBe("20.0%");

    // This or better: an overpair is everything above it too, which here is
    // the three sets plus the six aces.
    const mode = headButtons(app.stats)[0];
    expect(mode.textContent).toBe("absolute");
    mode.click();
    app.render();
    expect(mode.textContent).toBe("cumulative");
    expect(reading(app, "overpair").value).toBe("60.0%");
    expect(reading(app, "set").value).toBe("20.0%");
  });
});

describe("the equity matrix", () => {
  test("names every hand and puts its equity in the corner", async () => {
    const app = await open();
    type(app, "AA,KK,AQs");
    card(app.board, "Qs");
    card(app.board, "2h");
    card(app.board, "7s");
    // Equity is equity against somebody, so the other seat gets a range.
    seats2(app)[1].click();
    app.render();
    type(app, "JJ,TT,AKo");
    seats2(app)[0].click();
    app.render();
    tab(app, "eq-matrix").click();
    app.render();

    const cells = Array.from(app.output.querySelectorAll<HTMLElement>(".eq-cell"));
    expect(cells).toHaveLength(169);
    // Laid out and named like the range matrix, so a hand is in the same place
    // under the same name.
    expect(cells[0].querySelector(".eq-name")!.textContent).toBe("AA");
    expect(cells[1].querySelector(".eq-name")!.textContent).toBe("AKs");
    expect(cells[14].querySelector(".eq-name")!.textContent).toBe("KK");

    // Only the hands in the range carry a number, and it is in the corner.
    const filled = cells.filter((element) => element.classList.contains("on"));
    expect(filled.length).toBe(3);
    for (const element of filled) {
      const value = element.querySelector(".eq-value")!;
      expect(value.textContent).toMatch(/^\d+$/);
      expect(Number(value.textContent)).toBeGreaterThanOrEqual(0);
      expect(Number(value.textContent)).toBeLessThanOrEqual(100);
      expect(element.title).toMatch(/: \d+\.\d%$/);
    }
    // A hand not in the range is still named, and says nothing else.
    const bare = cells.find((element) => !element.classList.contains("on"))!;
    expect(bare.querySelector(".eq-name")!.textContent).not.toBe("");
    expect(bare.querySelector(".eq-value")).toBeNull();

    // Aces are ahead of the ace-king in the dead slots; a pair of queens on the
    // board would not be, so the numbers have to differ between cells.
    const values = filled.map((element) => Number(element.querySelector(".eq-value")!.textContent));
    expect(new Set(values).size).toBeGreaterThan(1);
  });

  test("the dice are drawn, not set from a font", async () => {
    const app = await open();
    // A dice character lives in a Unicode block plenty of systems have no font
    // for, and a missing glyph renders as an empty box - so it is an SVG.
    const random = app.board.querySelector<HTMLElement>(".board-actions .btn:last-of-type")!;
    expect(random.textContent).toContain("Random");
    expect(random.querySelector("svg.die")).not.toBeNull();
    expect(random.querySelectorAll(".die-pip")).toHaveLength(5);

    const deal = app.flops.querySelector<HTMLButtonElement>(".deal-flop")!;
    expect(deal.querySelector("svg.die")).not.toBeNull();
    expect(deal.textContent).toBe("");

    deal.click();
    app.render();
    expect(state().boardCards).toHaveLength(3);
  });

  test("suits are pips wherever a hand is printed", async () => {
    const app = await open();
    type(app, "AA,KK");
    card(app.board, "Qs");
    card(app.board, "2h");
    card(app.board, "7s");
    const seats = Array.from(app.strip.querySelectorAll<HTMLButtonElement>(".seat"));
    seats[1].click();
    app.render();
    type(app, "AKo");
    seats[0].click();
    app.render();
    tab(app, "eq-graph").click();
    app.render();

    // The table names hands the way the matrix does: rank, then a coloured pip.
    const first = app.output.querySelector<HTMLElement>(".eq-row:not(.eq-head) span:nth-child(2)")!;
    expect(first.textContent).toMatch(
      /^[2-9TJQKA][\u2660\u2665\u2666\u2663][2-9TJQKA][\u2660\u2665\u2666\u2663]$/,
    );
    expect(first.querySelectorAll("i.pip")).toHaveLength(2);
    expect(
      Array.from(first.querySelectorAll("i.pip")).every((pip) => /suit-[cdhs]/.test(pip.className)),
    ).toBe(true);

    // No letter-suit notation anywhere the reader is looking.
    expect(app.output.textContent).not.toMatch(/\b[2-9TJQKA][cdhs][2-9TJQKA][cdhs]\b/);

    // The text box keeps its letters, because that is what gets pasted.
    expect(app.range.querySelector<HTMLTextAreaElement>(".notation")!.value).toMatch(/[A-Z]/);
  });
});

describe("the output views", () => {
  test("each one says what it needs before it will say anything else", async () => {
    const app = await open();
    type(app, "22+");

    // Preflop, with nothing to measure against: every view says so plainly
    // rather than drawing an empty chart.
    for (const [key, wanted] of [
      ["overlap", /Pick a flop/],
      ["eq-matrix", /Needs a board/],
      ["eq-graph", /Needs a board/],
      ["hotness", /Needs a dealt hand/],
    ] as const) {
      tab(app, key).click();
      app.render();
      expect(app.output.textContent, key).toMatch(wanted);
    }

    // A flop is enough for the overlap.
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    // The made-against-draws cut needs a hand that is both, so give it one:
    // AhQh is ace high and the nut flushdraw at the same time.
    type(app, "22+,AQs");
    tab(app, "overlap").click();
    app.render();
    expect(app.output.querySelector(".overlap-table")).not.toBeNull();
    // Made hands down the side, draws across the top, and no diagonal to read.
    const columns = Array.from(app.output.querySelectorAll(".overlap-col")).map(
      (element) => element.textContent,
    );
    expect(columns).toContain("flushdraw");
    expect(columns).not.toContain("top pair");

    // A cell of it paints the hands that are both.
    const actionable = app.output.querySelectorAll<HTMLElement>(".overlap-cell.actionable");
    expect(actionable.length).toBeGreaterThan(0);
    headButton(app.stats, "Clear").click();
    app.render();
    app.output.querySelector<HTMLElement>(".overlap-cell.actionable")!.click();
    app.render();
    expect(
      state()
        .groupShares.slice(1)
        .some((share) => share > 0),
    ).toBe(true);
  });
});

describe("two seats and their filters", () => {
  test("a filter belongs to the seat that set it, whichever one is selected", async () => {
    const app = await open();
    const seats = Array.from(app.strip.querySelectorAll<HTMLButtonElement>(".seat"));

    // Seat A: four pairs, narrowed to the sets by a flop filter.
    type(app, "AA,KK,77,22");
    card(app.board, "Qs");
    card(app.board, "2h");
    card(app.board, "7s");
    headButton(app.stats, "Clear").click();
    app.render();
    row(app, "set").click();
    app.render();
    streets(app)[0].click();
    app.render();
    // Sevens and deuces make a set; the board took one of each.
    expect(state().liveCombos * state().passFraction).toBeCloseTo(6, 6);
    const narrowedA = state().effectiveNotation;

    // Seat B: something to measure against.
    console.log(
      "DBG1 A eff",
      state().effectiveNotation.slice(0, 40),
      "eq",
      JSON.stringify(state().equity),
    );
    seats[1].click();
    app.render();
    console.log(
      "DBG2 active",
      state().active,
      "B notation",
      JSON.stringify(state().players[1].notation),
    );
    type(app, "AKo");
    console.log(
      "DBG3 B notation",
      state().players[1].notation,
      "eq",
      JSON.stringify(state().equity),
      "board",
      state().board,
    );

    // Looking from B, A has to be the six sets and not the twenty-two pairs -
    // the filter is A's, and A still has it.
    tab(app, "eq-graph").click();
    app.render();
    console.log(
      "DBG equity",
      JSON.stringify(state().equity),
      "A",
      JSON.stringify(state().players[0].notation.slice(0, 40)),
      "B",
      state().players[1].notation,
      "eff",
      state().effectiveNotation.slice(0, 40),
    );
    const theirs = app.output.querySelectorAll<HTMLElement>(".eq-graph .curve").length;
    expect(theirs).toBe(2);
    const rows = Array.from(app.output.querySelectorAll<HTMLElement>(".eq-row:not(.eq-head)"));
    // B's own hands are on the table: twelve of AKo, less the ones the board
    // blocks, which is none of them here.
    expect(rows).toHaveLength(12);

    // And the equity is of A's continuing range, not of A as written. Sets
    // against ace-king is a rout; the whole range would not be.
    const againstSets = state().equity!.players[1].equity;
    seats[0].click();
    app.render();
    expect(state().effectiveNotation).toBe(narrowedA);
    streets(app)[0].click();
    app.render();
    seats[1].click();
    app.render();
    const againstAll = state().equity!.players[1].equity;
    expect(againstAll).toBeGreaterThan(againstSets);
  });

  test("each seat keeps its colour wherever it is drawn", async () => {
    const app = await open();
    const seats = Array.from(app.strip.querySelectorAll<HTMLButtonElement>(".seat"));
    type(app, "AA");
    seats[1].click();
    app.render();
    type(app, "KK");
    seats[0].click();
    app.render();
    card(app.board, "Qs");
    card(app.board, "2h");
    card(app.board, "7s");
    tab(app, "eq-graph").click();
    app.render();

    // The strip says A is the accent and B is the warning colour.
    expect(seats[0].querySelector(".seat-bar")!.classList.contains("seat-1")).toBe(false);
    expect(seats[1].querySelector(".seat-bar")!.classList.contains("seat-1")).toBe(true);

    // The graph has to agree, and go on agreeing when the selection changes.
    const curves = () =>
      Array.from(app.output.querySelectorAll<SVGElement>(".eq-graph .curve")).map((element) =>
        element.getAttribute("class")!,
      );
    expect(curves().sort()).toEqual(["curve seat-curve-0", "curve seat-curve-1"]);
    const keys = () =>
      Array.from(app.output.querySelectorAll(".eq-key")).map((element) => element.className);
    expect(keys()).toContain("eq-key key-seat-0");
    expect(keys()).toContain("eq-key key-seat-1");

    seats[1].click();
    app.render();
    expect(curves().sort()).toEqual(["curve seat-curve-0", "curve seat-curve-1"]);
    expect(keys()).toContain("eq-key key-seat-0");
    expect(keys()).toContain("eq-key key-seat-1");
  });
});

describe("more than two ranges in the pot", () => {
  const seats = (app: App) => Array.from(app.strip.querySelectorAll<HTMLButtonElement>(".seat"));

  test("a third range takes equity from the first two", async () => {
    const app = await open();
    card(app.board, "Kh");
    card(app.board, "7d");
    card(app.board, "2c");

    type(app, "AhAs");
    seats(app)[1].click();
    app.render();
    type(app, "QhQs");
    app.render();

    // Two seats: the readout is one pot between them, exactly enumerated.
    const heads = state().equity!;
    expect(heads.exact).toBe(true);
    expect(state().equitySeats).toEqual([0, 1]);
    expect(heads.players[0].equity).toBeGreaterThan(0.85);

    // A third seat arrives empty, so it is not in the pot yet.
    app.strip.querySelector<HTMLButtonElement>(".seat-add")!.click();
    app.render();
    expect(seats(app)).toHaveLength(3);
    expect(state().players[2].name).toBe("Range C");
    expect(state().equitySeats).toEqual([0, 1]);

    // Fill it, and the pot is three-way: sampled, and still one whole pot.
    seats(app)[2].click();
    app.render();
    type(app, "7h6h");
    app.render();
    const three = state().equity!;
    expect(state().equitySeats).toEqual([0, 1, 2]);
    expect(three.exact).toBe(false);
    expect(three.players).toHaveLength(3);
    const total = three.players.reduce((sum, player) => sum + player.equity, 0);
    expect(total, "one pot, however many are in it").toBeCloseTo(1, 6);
    // The favourite gives up share to the newcomer.
    expect(three.players[0].equity).toBeLessThan(heads.players[0].equity);
    // The other two do not both have to: a third player takes two cards out of
    // the deck, which thins the run-outs and can make somebody's outs *more*
    // likely. Here the queens gain a little, because the sevens are not among
    // the cards they were waiting for.

    // Every seat shows its own share on its badge.
    const badges = Array.from(app.strip.querySelectorAll(".seat-badge")).map(
      (badge) => badge.textContent,
    );
    expect(badges.every((text) => text!.endsWith("%"))).toBe(true);

    // The cross on a seat drops it, and the letters close the gap.
    seats(app)[1].querySelector<HTMLButtonElement>(".seat-drop")!.click();
    app.render();
    expect(seats(app)).toHaveLength(2);
    expect(state().players.map((player) => player.name)).toEqual(["Range A", "Range B"]);
    // What was C is now B, and it kept its hand.
    expect(state().players[1].notation).toBe("7h6h");
  });

  test("two seats is the floor, six the ceiling", async () => {
    const app = await open();
    const add = () => app.strip.querySelector<HTMLButtonElement>(".seat-add");

    for (let at = 2; at < state().maxSeats; at += 1) {
      add()!.click();
      app.render();
    }
    expect(seats(app)).toHaveLength(state().maxSeats);
    // Full: the button goes rather than sitting there doing nothing.
    expect(add()).toBeNull();

    // And the last two cannot be dropped: at two seats the cross goes away
    // rather than sitting there refusing to work.
    const drop = (at: number) => seats(app)[at].querySelector<HTMLButtonElement>(".seat-drop")!;
    while (state().players.length > 2) {
      drop(state().players.length - 1).click();
      app.render();
    }
    expect(seats(app)).toHaveLength(2);
    expect(drop(0).hidden).toBe(true);
    expect(drop(1).hidden).toBe(true);
  });
});

describe("a table of three, used", () => {
  const drop = (app: App, at: number) =>
    seats2(app)[at].querySelector<HTMLButtonElement>(".seat-drop")!;

  /** Three seats on a flop, filled and selected back to A. */
  async function table(): Promise<App> {
    const app = await open();
    card(app.board, "Kh");
    card(app.board, "7d");
    card(app.board, "2c");
    type(app, "AA,KK");
    seats2(app)[1].click();
    app.render();
    type(app, "QQ,JJ");
    app.strip.querySelector<HTMLButtonElement>(".seat-add")!.click();
    app.render();
    seats2(app)[2].click();
    app.render();
    type(app, "76s,65s");
    seats2(app)[0].click();
    app.render();
    return app;
  }

  test("the readout names every seat in the pot and says how it knows", async () => {
    const app = await table();

    // Three seats, three lines, each in its seat's colour, and the pot whole.
    expect(app.strip.querySelector(".equity-who")!.textContent).toBe("3-way");
    const lines = Array.from(app.strip.querySelectorAll<HTMLElement>(".equity-line"));
    expect(lines).toHaveLength(3);
    expect(lines.map((line) => line.textContent!.split(":")[0])).toEqual([
      "Range A",
      "Range B",
      "Range C",
    ]);
    expect(lines.map((line) => line.style.getPropertyValue("--seat"))).toEqual([
      "var(--seat-0)",
      "var(--seat-1)",
      "var(--seat-2)",
    ]);
    const shares = state().equity!.players.map((player) => player.equity);
    expect(shares.reduce((sum, share) => sum + share, 0)).toBeCloseTo(1, 6);

    // Three players cannot be enumerated, and the panel says so rather than
    // letting a sampled number pass for an exact one.
    expect(state().equity!.exact).toBe(false);
    expect(app.strip.querySelector(".equity-panel .hint")!.textContent).toMatch(/^sampled · /);

    // Two players can be, and it changes back when one leaves.
    drop(app, 2).click();
    app.render();
    expect(state().equity!.exact).toBe(true);
    expect(app.strip.querySelector(".equity-panel .hint")!.textContent).toMatch(/^exact · /);
  });

  test("a seat its own filter has emptied is not in the pot", async () => {
    const app = await table();

    // Seat C keeps only its flushdraws, and it has none on this board.
    seats2(app)[2].click();
    app.render();
    headButton(app.stats, "Clear").click();
    app.render();
    row(app, "flushdraw").click();
    app.render();
    expect(streets(app)[0].textContent).toMatch(/0$/);
    streets(app)[0].click();
    app.render();

    // With nothing left to be dealt, that seat has no hands in the pot - so the
    // pot is between the two that do, and exactly countable again.
    expect(state().equitySeats).toEqual([0, 1]);
    expect(state().equity!.exact).toBe(true);
    expect(app.strip.querySelector(".equity-who")!.textContent).toBe("Range A vs Range B");
    // And the emptied seat's badge says its name rather than a share of nothing.
    const badges = Array.from(app.strip.querySelectorAll(".seat-badge")).map(
      (badge) => badge.textContent,
    );
    expect(badges[2]).toBe("Range C");
  });

  test("dropping the seat you are looking at lands somewhere sensible", async () => {
    const app = await table();
    seats2(app)[2].click();
    app.render();
    expect(state().active).toBe(2);

    // Drop the one you are on: the selection has to go somewhere, and the
    // ranges left have to be the ones that were not dropped.
    drop(app, 2).click();
    app.render();
    expect(state().players).toHaveLength(2);
    expect(state().active).toBeLessThan(2);
    expect(state().players.map((player) => player.notation)).toEqual(["KK+", "JJ-QQ"]);
    expect(state().equitySeats).toEqual([0, 1]);
  });

  test("dropping the seat a setting points at leaves the setting somewhere real", async () => {
    const app = await table();

    // Point the equity views at C and the statistics column at B.
    tab(app, "eq-graph").click();
    app.render();
    const versus = app.output.querySelector<HTMLButtonElement>(".versus-output")!;
    versus.click();
    app.render();
    versus.click();
    app.render();
    expect(state().versusSeat).toBe(2);
    app.stats.querySelector<HTMLButtonElement>(".versus-button:not(.versus-output)")!.click();
    app.render();
    expect(state().compareSeat).toBe(1);

    // Now take C away. The setting cannot keep pointing at a seat that is not
    // there, and it must not point at a seat that is somebody else now either.
    drop(app, 2).click();
    app.render();
    expect(state().players).toHaveLength(2);
    expect(state().versusSeat).toBe(1);
    expect(app.output.querySelector<HTMLButtonElement>(".versus-output")!.textContent).toBe("vs B");
    // The statistics column still has a real seat to be about.
    expect(state().compareSeat).toBe(1);
    expect(row(app, "overpair").querySelector<HTMLElement>(".stat-versus")!.hidden).toBe(false);
  });

  test("each of three seats keeps its own filter", async () => {
    const app = await table();

    // A keeps only its sets; C keeps everything.
    headButton(app.stats, "Clear").click();
    app.render();
    row(app, "set").click();
    app.render();
    streets(app)[0].click();
    app.render();
    const aPass = state().passFraction;
    expect(aPass).toBeLessThan(1);

    // Switching seats shows that seat's filters, not the one set on A.
    seats2(app)[2].click();
    app.render();
    expect(state().filtersEnabled).toBe(false);
    expect(state().passFraction).toBe(1);

    // And back to A, its filter is where it was left.
    seats2(app)[0].click();
    app.render();
    expect(state().filtersEnabled).toBe(true);
    expect(state().passFraction).toBeCloseTo(aPass, 6);
  });

  test("a link carries the whole table, not just the first two seats", async () => {
    const app = await table();
    tab(app, "eq-graph").click();
    app.render();
    app.output.querySelector<HTMLButtonElement>(".versus-output")!.click();
    app.render();

    const before = {
      names: state().players.map((player) => player.name),
      notations: state().players.map((player) => player.notation),
      active: state().active,
      versus: state().versusSeat,
      seats: [...state().equitySeats],
    };
    expect(before.notations).toHaveLength(3);
    expect(before.versus).toBe(1);

    const packed = snapshot();
    restore(packed);
    app.render();

    expect(state().players.map((player) => player.name)).toEqual(before.names);
    expect(state().players.map((player) => player.notation)).toEqual(before.notations);
    expect(state().active).toBe(before.active);
    expect(state().versusSeat).toBe(before.versus);
    expect(state().equitySeats).toEqual(before.seats);
    expect(seats2(app)).toHaveLength(3);
  });

  test("dead cards take cards out of the deck and leave the pot alone", async () => {
    const app = await table();
    const before = state().liveCombos;
    const seatsBefore = [...state().equitySeats];

    // Two of them used to be read as "my hand" and collapse a three-way pot to
    // a hand against one range. They do not: the seats say what is measured,
    // and these cards say only that nobody at the table can hold them.
    card(app.strip, "Ah");
    card(app.strip, "Ad");
    app.render();
    expect(state().hand).toBeNull();
    expect(state().liveCombos).toBeLessThan(before);
    expect(state().equitySeats).toEqual(seatsBefore);
    expect(app.strip.querySelector(".equity-who")!.textContent).toBe("3-way");
  });

  test("a dealt hand is its own kind of seat", async () => {
    const app = await table();

    // Dealt from the dead-card panel, never by typing: the button is the mode,
    // and a click on the grid without it still only thins the deck.
    const deal = app.strip.querySelector<HTMLButtonElement>(".deal-hand")!;
    expect(deal.textContent).toBe("Deal a hand");
    deal.click();
    app.render();
    expect(deal.textContent).toBe("Pick two cards…");
    card(app.strip, "As");
    card(app.strip, "Ks");
    app.render();

    // A fourth seat, named after its cards - and the reader is left where they
    // were. There is nothing to do to a hand, so being moved onto one would
    // mean arriving at a dead panel and clicking back out of it.
    expect(state().players).toHaveLength(4);
    expect(state().players[3].hand).toBe("AsKs");
    expect(state().active).toBe(0);
    expect(state().editable).toBe(true);
    expect(app.range.classList.contains("dealt-hand")).toBe(false);
    expect(deal.textContent).toBe("Deal a hand");

    // It is marked as a hand rather than left to look like a small range.
    expect(seats2(app)[3].classList.contains("is-hand")).toBe(true);
    // And going to it, the panel has nothing to offer.
    seats2(app)[3].click();
    app.render();
    expect(state().editable).toBe(false);
    expect(app.range.classList.contains("dealt-hand")).toBe(true);

    // Its cards are out of the deck for everyone else, which is the whole
    // reason it is not a range.
    expect(state().dealt.sort()).toEqual(["As", "Ks"]);
    seats2(app)[0].click();
    app.render();
    expect(state().effectiveNotation).not.toMatch(/AsKs/);

    // Hotness is about one hand, and now there is one.
    seats2(app)[3].click();
    app.render();
    tab(app, "hotness").click();
    app.render();
    expect(app.output.querySelectorAll(".hot-row").length).toBeGreaterThan(40);

    // And it comes apart in one action, whatever the floor on ranges is.
    seats2(app)[3].querySelector<HTMLButtonElement>(".seat-drop")!.click();
    app.render();
    expect(state().players).toHaveLength(3);
    expect(state().dealt).toEqual([]);
  });

  test("typing one combination is still a range", async () => {
    const app = await table();
    type(app, "AhQh");
    app.render();

    // Nobody has seen those cards, so nothing is out of the deck and the seat
    // is still a range: editable, lettered, and counted as one of the two.
    expect(state().players[0].hand).toBeNull();
    expect(state().editable).toBe(true);
    expect(state().dealt).toEqual([]);
    expect(seats2(app)[0].classList.contains("is-hand")).toBe(false);
    expect(app.range.classList.contains("dealt-hand")).toBe(false);
  });

  test("the equity matrix is measured against the seat that was named", async () => {
    const app = await table();
    tab(app, "eq-matrix").click();
    app.render();

    const readFirst = () => {
      const cell = app.output.querySelector<HTMLElement>(".eq-cell .eq-value");
      return Number(cell!.textContent!.replace("%", ""));
    };
    const againstField = readFirst();

    const versus = app.output.querySelector<HTMLButtonElement>(".versus-output")!;
    versus.click();
    app.render();
    expect(state().versusSeat).toBe(1);
    const againstQueens = readFirst();

    versus.click();
    app.render();
    expect(state().versusSeat).toBe(2);
    const againstConnectors = readFirst();

    // Two overpairs below the aces are drawing close to dead. Suited connectors
    // on this board hold a pair of sevens and a straight draw, so they are the
    // harder of the two - and the field, being both, sits between them.
    expect(againstQueens).toBeGreaterThan(againstConnectors);
    expect(againstField).toBeLessThan(againstQueens);
    expect(againstField).toBeGreaterThan(againstConnectors);
  });
});

describe("choosing who to measure against", () => {
  const versus = (app: App) => app.output.querySelector<HTMLButtonElement>(".versus-output")!;

  test("any seat can be named, which is what makes B against C possible", async () => {
    const app = await open();
    card(app.board, "Kh");
    card(app.board, "7d");
    card(app.board, "2c");
    type(app, "AhAs");
    seats2(app)[1].click();
    app.render();
    type(app, "QhQs");
    seats2(app)[0].click();
    app.render();

    // With one other range there is nothing to choose: the field is that range,
    // and it says so by name rather than as an anonymous crowd of one.
    expect(state().versusSeat).toBe(1);
    tab(app, "eq-graph").click();
    app.render();
    expect(versus(app).textContent).toBe("vs B");

    // A third range gives the question somewhere to go.
    app.strip.querySelector<HTMLButtonElement>(".seat-add")!.click();
    app.render();
    seats2(app)[2].click();
    app.render();
    type(app, "7h6h");
    seats2(app)[1].click();
    app.render();

    // Seat B, against the aces and against the sevens, are different questions
    // and both can now be asked without moving any seat anywhere.
    const queens = () => {
      const view = state();
      return view.players[view.active].notation;
    };
    expect(queens()).toBe("QhQs");

    const equityOfQueens = () => {
      tab(app, "eq-graph").click();
      app.render();
      const mine = app.output.querySelector(".eq-key")!.textContent ?? "";
      expect(mine).toMatch(/^Range B · /);
      return Number(mine.match(/([\d.]+)%/)![1]);
    };

    // Against the field to begin with, which is both of them laid over one
    // another - not the three-way pot, which is the readout's question.
    expect(state().versusSeat).toBeNull();
    tab(app, "eq-graph").click();
    app.render();
    expect(versus(app).textContent).toBe("vs all");
    const againstField = equityOfQueens();

    versus(app).click();
    app.render();
    expect(state().versusSeat).toBe(0);
    expect(versus(app).textContent).toBe("vs A");
    const againstAces = equityOfQueens();

    versus(app).click();
    app.render();
    expect(state().versusSeat).toBe(2);
    expect(versus(app).textContent).toBe("vs C");
    const againstSevens = equityOfQueens();

    expect(againstAces).toBeLessThan(20);
    expect(againstSevens).toBeGreaterThan(50);
    expect(againstField).toBeGreaterThan(againstAces);
    expect(againstField).toBeLessThan(againstSevens);

    // And round again to the field.
    versus(app).click();
    app.render();
    expect(state().versusSeat).toBeNull();
  });

  test("the control appears only where it changes what is drawn", async () => {
    const app = await open();
    card(app.board, "Kh");
    card(app.board, "7d");
    card(app.board, "2c");
    type(app, "AA");
    seats2(app)[1].click();
    app.render();
    type(app, "KK");
    seats2(app)[0].click();
    app.render();

    // Three views are measured against another range - the two equity ones and
    // the hotness, which asks how the cards to come treat one hand against it.
    // The pie is about colours and the overlap about statistics, and a control
    // that changed nothing would teach the reader that it does nothing.
    const showing = (key: string) => {
      tab(app, key).click();
      app.render();
      return !versus(app).hidden;
    };
    expect(showing("eq-matrix")).toBe(true);
    expect(showing("eq-graph")).toBe(true);
    expect(showing("hotness")).toBe(true);
    expect(showing("groups")).toBe(false);
    expect(showing("overlap")).toBe(false);

    // Preflop there is no board to be measured on at all.
    mutate((engine) => engine.setBoard(""));
    app.render();
    expect(state().board).toBe("");
    expect(showing("eq-graph")).toBe(false);
  });

  test("the graph draws the opponent in that seat's colour", async () => {
    const app = await open();
    card(app.board, "Kh");
    card(app.board, "7d");
    card(app.board, "2c");
    type(app, "AhAs");
    seats2(app)[1].click();
    app.render();
    type(app, "QhQs");
    app.strip.querySelector<HTMLButtonElement>(".seat-add")!.click();
    app.render();
    seats2(app)[2].click();
    app.render();
    type(app, "7h6h");
    seats2(app)[0].click();
    app.render();
    tab(app, "eq-graph").click();
    app.render();

    // The field is nobody's range, so it is nobody's colour either.
    const curves = () =>
      Array.from(app.output.querySelectorAll<HTMLElement>(".eq-graph .curve")).map((curve) => ({
        cls: curve.getAttribute("class"),
        seat: curve.style.getPropertyValue("--seat"),
      }));
    expect(curves().some((curve) => curve.cls?.includes("curve-field"))).toBe(true);

    versus(app).click();
    app.render();
    // Named, and now wearing that seat's colour - the same one its tile wears.
    expect(state().versusSeat).toBe(1);
    const named = curves().find((curve) => curve.cls?.includes("seat-curve-1"));
    expect(named?.seat).toBe("var(--seat-1)");
  });
});

describe("a colour that continues part of the time", () => {
  const passSlider = (app: App) => app.stats.querySelector<HTMLInputElement>(".pass-slider")!;
  const passRow = (app: App) => app.stats.querySelector<HTMLElement>(".pass-row")!;

  test("half a colour is half its combinations through the filter", async () => {
    const app = await open();
    type(app, "KK");
    card(app.board, "Qh");
    card(app.board, "7d");
    card(app.board, "2c");

    // Six kings, all of them an overpair, all painted blue by default.
    expect(reading(app, "overpair").mark).toBe("blue");
    expect(streets(app)[0].textContent).toMatch(/6$/);
    expect(passRow(app).hidden).toBe(false);
    expect(passSlider(app).value).toBe("100");
    expect(app.stats.querySelector(".pass-value")!.textContent).toBe("100%");

    // Continue with half of them, and the filter's number halves with it.
    passSlider(app).value = "50";
    passSlider(app).dispatchEvent(new window.Event("input", { bubbles: true }));
    app.render();
    expect(streets(app)[0].textContent).toMatch(/3$/);
    expect(app.stats.querySelector(".pass-value")!.textContent).toBe("50%");
    // The swatch says so too, so the palette carries the whole strategy.
    expect(swatch(app, "blue").dataset.share).toBe("50");
    expect(swatch(app, "blue").classList.contains("partial")).toBe(true);

    // Pressing the filter keeps three of the six.
    streets(app)[0].click();
    app.render();
    const seen = audit(app);
    expect(seen.pass).toBeCloseTo(3, 6);
    // Half of each cell rather than half the cells: the matrix shows it as a
    // cell that is partly filtered rather than as a hand that vanished.
    expect(drawn(app, "KK").on).toBe(true);
    expect(drawn(app, "KK").filtered).toBe(true);
    expect(drawn(app, "KK").passing).toBe("50.0%");

    // All the way down is the same as not painting them at all.
    passSlider(app).value = "0";
    passSlider(app).dispatchEvent(new window.Event("input", { bubbles: true }));
    app.render();
    expect(audit(app).pass).toBeCloseTo(0, 6);
  });

  test("each colour continues at its own rate", async () => {
    const app = await open();
    type(app, "KK,JJ");
    card(app.board, "Qh");
    card(app.board, "7d");
    card(app.board, "2c");

    // Kings are an overpair, jacks sit under the queen. Paint them apart.
    headButton(app.stats, "Clear").click();
    app.render();
    swatch(app, "blue").click();
    app.render();
    row(app, "overpair").click();
    app.render();
    swatch(app, "green").click();
    app.render();
    row(app, "pp < top card").click();
    app.render();
    expect(streets(app)[0].textContent).toMatch(/12$/);

    // A quarter of the green ones: six blue plus a quarter of six green.
    passSlider(app).value = "25";
    passSlider(app).dispatchEvent(new window.Event("input", { bubbles: true }));
    app.render();
    expect(streets(app)[0].textContent).toMatch(/7\.5$/);
    expect(swatch(app, "green").dataset.share).toBe("25");
    expect(swatch(app, "blue").dataset.share).toBe("");

    // Switching the held colour switches which one the slider is about.
    swatch(app, "blue").click();
    app.render();
    expect(passSlider(app).value).toBe("100");
    expect(passRow(app).textContent).toMatch(/blue continues/);

    streets(app)[0].click();
    app.render();
    expect(audit(app).pass).toBeCloseTo(7.5, 6);
  });

  test("the eraser has no share to set", async () => {
    const app = await open();
    type(app, "KK");
    card(app.board, "Qh");
    card(app.board, "7d");
    card(app.board, "2c");
    app.stats.querySelector<HTMLButtonElement>(".swatch.colour-none")!.click();
    app.render();
    // Unpainted hands never continue, so there is nothing to ask about.
    expect(passRow(app).hidden).toBe(true);
  });
});

describe("comparing two ranges", () => {
  const versus = (app: App) => app.stats.querySelector<HTMLButtonElement>(".versus-button")!;
  const column = (app: App, label: string) =>
    row(app, label).querySelector<HTMLElement>(".stat-versus")!;

  test("a second column reads the other range on the same board", async () => {
    const app = await open();
    card(app.board, "Kh");
    card(app.board, "7d");
    card(app.board, "2c");

    // Seat A: two aces. Seat B: two kings, which is a set on this board.
    type(app, "AhAs,AhAd");
    seats2(app)[1].click();
    app.render();
    type(app, "KsKd");
    seats2(app)[0].click();
    app.render();

    // No comparison to begin with, and the column is not taking up room.
    expect(state().compareSeat).toBeNull();
    expect(versus(app).textContent).toBe("vs —");
    expect(column(app, "overpair").hidden).toBe(true);

    versus(app).click();
    app.render();
    expect(state().compareSeat).toBe(1);
    expect(versus(app).textContent).toBe("vs B");

    // A holds two overpair combinations and no set; B holds one set and no
    // overpair. Read in combinations, both columns are countable by hand.
    app.press("Tab");
    expect(reading(app, "overpair").value).toBe("2");
    expect(column(app, "overpair").textContent).toBe("0");
    expect(reading(app, "set").value).toBe("0");
    expect(column(app, "set").textContent).toBe("1");
    // The other range being ahead on a row is said in weight, not left to be
    // worked out by subtracting.
    expect(column(app, "set").classList.contains("ahead")).toBe(true);
    expect(column(app, "overpair").classList.contains("ahead")).toBe(false);
    app.press("Tab");

    // Pressing again comes back round to no comparison.
    versus(app).click();
    app.render();
    expect(state().compareSeat).toBeNull();
    expect(column(app, "set").hidden).toBe(true);
  });

  test("the second column follows the first into a hovered statistic", async () => {
    const app = await open();
    card(app.board, "Kh");
    card(app.board, "7d");
    card(app.board, "2c");
    type(app, "AhAs,KsKd");
    seats2(app)[1].click();
    app.render();
    type(app, "AhAs,KsKd");
    seats2(app)[0].click();
    app.render();
    versus(app).click();
    app.render();

    // Identical ranges, so the two columns say the same thing.
    app.press("Tab");
    expect(column(app, "overpair").textContent).toBe(reading(app, "overpair").value);

    // Hovering restricts the panel to one statistic; the second column has to
    // be restricted the same way or the two stop being comparable.
    row(app, "set").dispatchEvent(new window.Event("pointerenter"));
    app.render();
    expect(reading(app, "overpair").value).toBe("0");
    expect(column(app, "overpair").textContent).toBe("0");
    expect(column(app, "set").textContent).toBe(reading(app, "set").value);
    row(app, "set").dispatchEvent(new window.Event("pointerleave"));
    app.render();
    app.press("Tab");
  });

  test("comparing a seat with itself shows nothing", async () => {
    const app = await open();
    card(app.board, "Kh");
    card(app.board, "7d");
    card(app.board, "2c");
    type(app, "AA");
    versus(app).click();
    app.render();
    expect(state().compareSeat).toBe(1);

    // Switching to the seat being compared against leaves nothing to compare:
    // a range against itself is not a comparison, so the column goes.
    seats2(app)[1].click();
    app.render();
    expect(state().compareSeat).toBeNull();
    expect(column(app, "overpair").hidden).toBe(true);
  });
});

describe("two seats side by side", () => {
  test("each keeps its own range, board reading and painting", async () => {
    const app = await open();
    const seats = Array.from(app.strip.querySelectorAll<HTMLButtonElement>(".seat"));
    expect(seats).toHaveLength(2);

    type(app, "AA");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    expect(reading(app, "overpair").value).toBe("100.0%");

    seats[1].click();
    app.render();
    expect(state().active).toBe(1);
    // A seat nobody has filled in is empty, and the board is shared.
    expect(state().players[1].combos).toBe(0);
    expect(state().board).toBe("Kh 7h 2c");
    type(app, "KK");
    expect(reading(app, "set").value).toBe("100.0%");
    expect(reading(app, "overpair").value).toBe("0.0%");

    // Painting one seat does not paint the other.
    headButton(app.stats, "Clear").click();
    app.render();
    swatch(app, "green").click();
    row(app, "set").click();
    app.render();
    expect(reading(app, "set").mark).toBe("green");

    seats[0].click();
    app.render();
    expect(state().players[state().active].notation).toBe("AA");
    expect(reading(app, "overpair").value).toBe("100.0%");
    expect(reading(app, "set").mark).not.toBe("green");

    // And each thumbnail draws its own range.
    expect(seats[0].querySelectorAll(".thumb-matrix i.on").length).toBeGreaterThan(0);
    expect(seats[1].querySelectorAll(".thumb-matrix i.on").length).toBeGreaterThan(0);
  });
});

describe("the numbers on screen agree with each other", () => {
  test("a small range walked from flop to river, audited at every step", async () => {
    const app = await open();
    // Four pairs and two suited hands: small enough to check by hand, varied
    // enough that the board changes what they are.
    type(app, "AA,KK,77,AQs,54s");
    expect(state().players[state().active].combos).toBe(26);

    card(app.board, "Qs");
    card(app.board, "2h");
    card(app.board, "7s");
    // The board takes three sevens and one AQs: 26 - 3 - 1 = 22.
    expect(state().liveCombos).toBe(22);
    let seen = audit(app);
    expect(seen.total).toBe(22);
    expect(seen.pass).toBeNull();
    // Aces and kings are both over the queen, so both are overpairs; the
    // sevens are a set, AQs is top pair, and 54s has missed entirely.
    expect(seen.rows.get("overpair")).toBe(12);
    expect(seen.rows.get("set")).toBe(3);
    expect(seen.rows.get("top pair")).toBe(3);
    expect(seen.rows.get("no made hand")).toBe(4);

    // The default grouping is top pair or better plus the flushdraws, and the
    // board is two-tone in spades - so 5s4s comes along and its three offsuit
    // cousins do not. Eighteen made hands and one draw.
    expect(seen.rows.get("flushdraw")).toBe(1);
    streets(app)[0].click();
    app.render();
    seen = audit(app);
    expect(seen.pass).toBe(19);
    expect(seen.streets).toEqual([19]);
    expect(seen.pie).toBeCloseTo(19, 6);

    // The king of hearts lands. It blocks half the kings, so the flop's own
    // number falls with it - and the footer follows. Nineteen less the three
    // pairs of kings that used it.
    card(app.board, "Kh");
    expect(state().street).toBe("turn");
    seen = audit(app);
    expect(seen.streets).toHaveLength(2);
    expect(seen.streets[0]).toBe(16);
    expect(seen.pass).toBe(16);

    // Repaint for the turn and press its filter: the chain reads down.
    headButton(app.stats, "Clear").click();
    app.render();
    row(app, "set").click();
    app.render();
    streets(app)[1].click();
    app.render();
    seen = audit(app);
    expect(seen.streets[0]).toBeGreaterThanOrEqual(seen.streets[1]);
    expect(seen.pass).toBe(seen.streets[1]);
    // Only the sets are left, and on a king-high board that is the kings and
    // the sevens: three of each, and the kings lost half to the turn card.
    expect(seen.pass).toBe(6);

    // The river, and a third link in the chain.
    card(app.board, "4d");
    expect(state().street).toBe("river");
    seen = audit(app);
    expect(seen.streets).toHaveLength(3);
    expect(seen.streets[0]).toBeGreaterThanOrEqual(seen.streets[1]);
    expect(seen.streets[1]).toBeGreaterThanOrEqual(seen.streets[2]);
    expect(seen.pass).toBe(seen.streets[1]);

    // Lifting the turn's filter leaves whatever else is on - and Clear took the
    // flop's away earlier, so nothing is, and every panel has to say so.
    streets(app)[1].click();
    app.render();
    seen = audit(app);
    expect(seen.filtersOn).toBe(false);
    expect(seen.pass).toBeNull();
    expect(seen.total).toBe(state().liveCombos);

    // Pressing the flop's again gives the chain one link, and the deepest link
    // is the number the footer reports.
    streets(app)[0].click();
    app.render();
    seen = audit(app);
    expect(seen.filtersOn).toBe(true);
    expect(seen.pass).toBe(seen.streets[0]);
    // Only the sets are painted, and the river took nothing from them.
    expect(seen.pass).toBe(6);
  });

  test("the audit holds through painting, inverting and the equity slider", async () => {
    const app = await open();
    type(app, "22+,AQs+,AJo+");
    card(app.board, "Qs");
    card(app.board, "2h");
    card(app.board, "7s");
    audit(app);

    // Painting by category.
    headButton(app.stats, "Clear").click();
    app.render();
    audit(app);
    row(app, "top pair").click();
    app.render();
    swatch(app, "green").click();
    row(app, "set").click();
    app.render();
    const painted = audit(app);
    expect(painted.pass).toBeNull();
    expect(painted.pie).toBeCloseTo(painted.total, 1);

    // Inverting.
    app.stats.querySelector<HTMLButtonElement>(".palette-action")!.click();
    app.render();
    audit(app);

    // The slider, which cuts across the categories.
    const slider = app.stats.querySelector<HTMLInputElement>(".share-slider")!;
    slider.value = "35";
    slider.dispatchEvent(new window.Event("input", { bubbles: true }));
    app.render();
    audit(app);

    // And with a filter over the top of all of it.
    streets(app)[0].click();
    app.render();
    const filtered = audit(app);
    expect(filtered.pass).toBeGreaterThan(0);
    expect(filtered.pass).toBeLessThan(filtered.total);

    // Undo the slider and audit again.
    app.stats.querySelector<HTMLButtonElement>(".cut-clear")!.click();
    app.render();
    audit(app);
  });

  test("a fractional range keeps every panel honest about halves", async () => {
    const app = await open();
    // Six aces at full weight, six kings at a quarter: 7.5 combinations.
    type(app, "AA,KK:0.25");
    card(app.board, "Qs");
    card(app.board, "2h");
    card(app.board, "7s");
    expect(state().players[state().active].combos).toBeCloseTo(7.5, 6);
    expect(state().liveCombos).toBeCloseTo(7.5, 6);

    const seen = audit(app);
    expect(seen.total).toBeCloseTo(7.5, 1);
    expect(seen.rows.get("overpair")).toBeCloseTo(7.5, 1);
    expect(seen.cells).toBeCloseTo(7.5, 1);
    // Both cells are overpairs here, so the filter keeps all of it.
    streets(app)[0].click();
    app.render();
    const after = audit(app);
    expect(after.pass).toBeCloseTo(7.5, 1);
    expect(app.board.querySelector(".tally")!.textContent).toMatch(/7\.5 combos/);
  });

  test("an empty range says nothing rather than something wrong", async () => {
    const app = await open();
    app.range.querySelector<HTMLButtonElement>(".quick:last-of-type")!.click();
    app.render();
    card(app.board, "Qs");
    card(app.board, "2h");
    card(app.board, "7s");

    expect(state().liveCombos).toBe(0);
    expect(state().passFraction).toBe(0);
    expect(app.stats.querySelector(".effective")!.textContent).toMatch(/Total number of combos: 0/);
    expect(app.board.querySelector(".tally")!.textContent).toBe("0 combos, none filtered out yet.");
    // No slice of nothing, and no stray colour in the matrix.
    tab(app, "groups").click();
    app.render();
    expect(app.output.textContent).toMatch(/Nothing in the range to group/);
    expect(app.range.querySelectorAll(".cell.on")).toHaveLength(0);
    // And pressing a filter on an empty range is not an error.
    streets(app)[0].click();
    app.render();
    expect(state().passFraction).toBe(0);
    expect(app.stats.querySelector(".effective")!.textContent).toMatch(/pass the filters: 0/);
  });
});

describe("the core loop", () => {
  test("range, flop, read, filter, and back again", async () => {
    const app = await open();
    chip(app, "mtt", "open", "UTG").click();
    app.render();
    const opened = state().players[state().active].percent;
    expect(opened).toBeGreaterThan(16);
    expect(opened).toBeLessThan(18);

    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    expect(state().street).toBe("flop");

    // The default grouping is on, and the filters are not.
    expect(reading(app, "top pair").mark).toBe("blue");
    expect(reading(app, "middle pair").mark).toBe("none");
    expect(state().filtersEnabled).toBe(false);
    expect(app.stats.querySelector(".effective")!.textContent).toMatch(/The filters are OFF/);
    expect(app.range.querySelectorAll(".cell.filtered")).toHaveLength(0);

    // The lamp says off, and the count says what pressing would leave.
    const flop = streets(app)[0];
    expect(flop.querySelector(".lamp")!.classList.contains("on")).toBe(false);
    const wouldPass = Number(flop.textContent!.match(/[\d.]+$/)![0]);
    expect(wouldPass).toBeGreaterThan(0);
    expect(wouldPass).toBeLessThan(state().liveCombos);

    flop.click();
    app.render();
    expect(state().filtersEnabled).toBe(true);
    expect(flop.querySelector(".lamp")!.classList.contains("on")).toBe(true);
    // The button rounds to a tenth, so compare at the precision it prints.
    expect(state().liveCombos * state().passFraction).toBeCloseTo(wouldPass, 1);
    expect(app.range.querySelectorAll(".cell.filtered").length).toBeGreaterThan(0);

    flop.click();
    app.render();
    expect(state().filtersEnabled).toBe(false);
    expect(state().passFraction).toBeCloseTo(1, 9);
    expect(app.range.querySelectorAll(".cell.filtered")).toHaveLength(0);
  });

  test("a live filter follows the marks, and stops when a card lands", async () => {
    const app = await open();
    type(app, "AA,KK,QQ,JJ");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    headButton(app.stats, "Clear").click();
    app.render();

    // Overpairs only: aces, queens and jacks are over the king... queens and
    // jacks are not, so this is the six aces.
    row(app, "overpair").click();
    app.render();
    streets(app)[0].click();
    app.render();
    expect(state().liveCombos * state().passFraction).toBeCloseTo(6, 6);

    // Still on the flop, so adding the sets to the marks adds them here too.
    row(app, "set").click();
    app.render();
    expect(state().liveCombos * state().passFraction).toBeCloseTo(9, 6);
    expect(streets(app)[0].textContent).toMatch(/Flop · 9$/);

    // A card lands. What continued on the flop is now a fact about the flop.
    card(app.board, "3s");
    expect(state().street).toBe("turn");
    expect(streets(app)).toHaveLength(2);
    expect(streets(app)[0].textContent).toMatch(/Flop · 9$/);
    headButton(app.stats, "Clear").click();
    app.render();
    // Clearing the marks cannot reach back into the flop's decision.
    expect(streets(app)[0].textContent).toMatch(/Flop · 0$/);
  });

  test("a card landing takes the combinations it blocks out of the count", async () => {
    const app = await open();
    type(app, "AA");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    streets(app)[0].click();
    app.render();
    expect(streets(app)[0].textContent).toMatch(/Flop · 6$/);

    // The ace of spades lands: three of the six pairs of aces used it.
    card(app.board, "As");
    expect(state().street).toBe("turn");
    expect(streets(app)[0].textContent).toMatch(/Flop · 3$/);
    expect(state().liveCombos).toBe(3);
  });
});

describe("painting by hand", () => {
  test("two colours, a pie that adds up, and a mark worth drawing", async () => {
    const app = await open();
    type(app, "AA,KK,QQ");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    headButton(app.stats, "Clear").click();
    app.render();

    // Nothing painted: one slice, all of it grey.
    tab(app, "groups").click();
    app.render();
    expect(app.output.querySelectorAll(".pie-key")).toHaveLength(1);
    expect(app.output.querySelector(".pie-key")!.textContent).toMatch(/unpainted/);

    // Blue on the sets, green on the overpairs.
    row(app, "set").click();
    app.render();
    swatch(app, "green").click();
    row(app, "overpair").click();
    app.render();
    expect(reading(app, "set").mark).toBe("blue");
    expect(reading(app, "overpair").mark).toBe("green");
    expect(state().coloursUsed).toBe(2);

    // Three slices now - grey, blue, green - and they add up.
    const keys = Array.from(app.output.querySelectorAll(".pie-key"));
    expect(keys).toHaveLength(3);
    const shares = keys.map((key) => Number(key.textContent!.match(/([\d.]+)%$/)![1]));
    expect(shares.reduce((sum, share) => sum + share, 0)).toBeCloseTo(100, 1);
    // Fifteen live: six aces, three kings, six queens. Blue is the three
    // kings, green the six aces, and the rest is the queens.
    expect(state().liveCombos).toBe(15);
    expect(shares[0]).toBeCloseTo(40, 1);
    expect(shares[1]).toBeCloseTo(20, 1);
    expect(shares[2]).toBeCloseTo(40, 1);

    // Two colours make the mark worth drawing, and it lands on both cells.
    expect(drawn(app, "KK").grouped).toBe(true);
    expect(drawn(app, "AA").grouped).toBe(true);
    expect(drawn(app, "QQ").grouped).toBe(false);

    // Pressing the filter keeps both colours and drops the rest.
    streets(app)[0].click();
    app.render();
    expect(state().liveCombos * state().passFraction).toBeCloseTo(9, 6);
    expect(drawn(app, "QQ").filtered).toBe(true);
    expect(drawn(app, "QQ").passing).toBe("0.0%");
    expect(drawn(app, "AA").filtered).toBe(false);
  });

  test("editing one hand puts a gear up, and putting it back takes it down", async () => {
    const app = await open();
    type(app, "AQs");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    headButton(app.stats, "Clear").click();
    app.render();

    row(app, "ace high").click();
    app.render();
    expect(reading(app, "ace high").mark).toBe("blue");

    // Shift-click opens the category's four hands over the matrix.
    row(app, "ace high").dispatchEvent(
      new window.MouseEvent("pointerdown", { bubbles: true, shiftKey: true }),
    );
    app.render();
    const chips = () =>
      Array.from(app.range.querySelectorAll<HTMLButtonElement>(".edit-strip .combo-chip"));
    expect(chips()).toHaveLength(4);
    expect(chips().every((element) => element.classList.contains("paint-blue"))).toBe(true);

    // Out: the category no longer speaks for all of its hands.
    chips()[0].click();
    app.render();
    expect(reading(app, "ace high").mark).toBe("mixed");
    expect(row(app, "ace high").querySelector(".filter-mark")!.textContent).toBe("⚙");
    expect(state().liveCombos).toBe(4);

    // Straight back in: nothing changed, so nothing to warn about.
    chips()[0].click();
    app.render();
    expect(reading(app, "ace high").mark).toBe("blue");

    // Out again, and this time clear the gear by saying what the whole
    // category is - which is what clicking a gear row does.
    chips()[1].click();
    app.render();
    expect(reading(app, "ace high").mark).toBe("mixed");
    row(app, "ace high").click();
    app.render();
    expect(reading(app, "ace high").mark).toBe("blue");
    expect(chips().every((element) => element.classList.contains("paint-blue"))).toBe(true);

    // And clicking it again, now that it is wholly blue, unpaints it.
    row(app, "ace high").click();
    app.render();
    expect(reading(app, "ace high").mark).toBe("none");
  });

  test("Inv gives back exactly what was not painted", async () => {
    const app = await open();
    type(app, "AA,KK,QQ");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    expect(reading(app, "overpair").mark).toBe("blue");
    expect(reading(app, "pp < top card").mark).toBe("none");

    const shares = () => [...state().groupShares];
    const before = shares();
    const total = before.reduce((sum, share) => sum + share, 0);
    expect(before[0]).toBeGreaterThan(0);
    expect(before[1]).toBeGreaterThan(0);

    app.stats.querySelector<HTMLButtonElement>(".palette-action")!.click();
    app.render();

    // The two readings are one range said twice: painted now is unpainted
    // before, to the combination. Adding past the range would mean some hand
    // was on both sides of a division of it, which cannot be.
    const after = shares();
    expect(after[1]).toBeCloseTo(before[0], 6);
    expect(after[0]).toBeCloseTo(before[1], 6);
    expect(after.reduce((sum, share) => sum + share, 0)).toBeCloseTo(total, 6);
    // The pie says the same thing, because it is the same numbers drawn.
    const pie = Array.from(app.output.querySelectorAll(".pie-key")).map((key) =>
      Number(key.textContent!.match(/([\d.]+)%/)![1]),
    );
    expect(pie.reduce((sum, share) => sum + share, 0)).toBeCloseTo(100, 1);

    // A category painted whole comes out bare, and one that was bare comes out
    // painted where nothing else claims its hands.
    expect(reading(app, "overpair").mark).toBe("none");
    expect(reading(app, "pp < top card").mark).toBe("blue");

    // And twice over is where it started.
    app.stats.querySelector<HTMLButtonElement>(".palette-action")!.click();
    app.render();
    expect(shares()[1]).toBeCloseTo(before[1], 6);
    expect(reading(app, "overpair").mark).toBe("blue");
  });

  test("a colour picked over whole is a colour, not a gear", async () => {
    const app = await open();
    type(app, "KK");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    headButton(app.stats, "Clear").click();
    app.render();
    row(app, "set").click();
    app.render();
    expect(reading(app, "set").mark).toBe("blue");

    // Take every one of the three live kings out by hand. The category has been
    // picked over, but picked over whole - so it is still of one mind, and a
    // gear would be claiming a disagreement that is not there.
    row(app, "set").dispatchEvent(
      new window.MouseEvent("pointerdown", { bubbles: true, shiftKey: true }),
    );
    app.render();
    // The strip is rebuilt on every render, so it is read again each time.
    const chips = () =>
      Array.from(app.range.querySelectorAll<HTMLButtonElement>(".edit-strip .combo-chip"));
    expect(chips()).toHaveLength(3);

    for (let at = 0; at < 3; at += 1) {
      chips()[at].click();
      app.render();
    }
    expect(reading(app, "set").mark).not.toBe("mixed");

    // Put one back, and now they genuinely disagree.
    chips()[0].click();
    app.render();
    expect(reading(app, "set").mark).toBe("mixed");
  });
});

describe("the equity slider", () => {
  test("paints the top of the range, and the cross puts back what was there", async () => {
    const app = await open();
    type(app, "AA,KK,QQ,JJ,TT");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    headButton(app.stats, "Clear").click();
    app.render();
    row(app, "overpair").click();
    app.render();
    expect(reading(app, "overpair").mark).toBe("blue");

    const slider = app.stats.querySelector<HTMLInputElement>(".share-slider")!;
    slider.value = "40";
    slider.dispatchEvent(new window.Event("input", { bubbles: true }));
    app.render();

    // It paints; it does not narrow.
    expect(chrome.cut).not.toBeNull();
    expect(state().filtersEnabled).toBe(false);
    expect(state().passFraction).toBeCloseTo(1, 9);
    expect(app.stats.querySelector(".share-value")!.textContent).toMatch(/%\s·\s\d+%\+ eq/);
    // Cutting across the categories is what a gear is for.
    expect(statDefs.some((definition) => reading(app, definition.label).mark === "mixed")).toBe(
      true,
    );

    // And the cross restores exactly what was painted before it ran.
    app.stats.querySelector<HTMLButtonElement>(".cut-clear")!.click();
    app.render();
    expect(chrome.cut).toBeNull();
    expect(reading(app, "overpair").mark).toBe("blue");
    expect(reading(app, "set").mark).toBe("none");
    for (const definition of statDefs) {
      expect(reading(app, definition.label).mark, definition.label).not.toBe("mixed");
    }
  });
});

describe("equity between two seats", () => {
  test("fills both ranges, reads the graph, and the columns agree", async () => {
    const app = await open();
    type(app, "AA");
    const seats = Array.from(app.strip.querySelectorAll<HTMLButtonElement>(".seat"));
    seats[1].click();
    app.render();
    type(app, "KK");
    seats[0].click();
    app.render();
    card(app.board, "7h");
    card(app.board, "5d");
    card(app.board, "2c");

    // Aces against kings on a blank board: a long way ahead, and not certain.
    const equity = state().equity!.players[0].equity;
    // Kings need a king, and there are two of them left.
    expect(equity).toBeGreaterThan(0.9);
    expect(equity).toBeLessThan(0.99);
    expect(state().equity!.exact).toBe(true);

    tab(app, "eq-graph").click();
    app.render();
    // Both distributions, drawn.
    expect(app.output.querySelectorAll(".eq-graph .curve")).toHaveLength(2);
    expect(app.output.querySelector(".eq-graph .curve.seat-curve-1")).not.toBeNull();

    // And the table under it: six pairs of aces, each with win and tie.
    const rows = Array.from(app.output.querySelectorAll<HTMLElement>(".eq-row:not(.eq-head)"));
    expect(rows).toHaveLength(6);
    for (const line of rows) {
      const [got, win, tie] = [2, 3, 4].map((index) => Number(line.children[index].textContent));
      expect(got).toBeCloseTo(win + tie / 2, 2);
      expect(win).toBeGreaterThan(70);
    }
  });

  test("one hand against a range, and hotness ranks every card to come", async () => {
    const app = await open();
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    // A range in the seats and a hand dealt against it.
    type(app, "22+");
    app.strip.querySelector<HTMLButtonElement>(".deal-hand")!.click();
    app.render();
    card(app.strip, "As");
    card(app.strip, "Ks");
    app.render();
    // Dealing leaves the reader on their range; the hand is read by going to it.
    expect(state().players[state().players.length - 1].hand).toBe("AsKs");
    Array.from(app.strip.querySelectorAll<HTMLButtonElement>(".seat")).at(-1)!.click();
    app.render();
    expect(state().hand).toBe("AsKs");

    // Top pair, top kicker against every pocket pair: comfortably ahead. The
    // report lists the seats in seat order, so the hand is read by its seat
    // rather than by assuming it comes first.
    const mine = state().equitySeats.indexOf(state().active);
    expect(state().equity!.players[mine].equity).toBeGreaterThan(0.5);

    tab(app, "hotness").click();
    app.render();
    // The whole deck laid out, with the five gone cards left blank.
    const cards = Array.from(app.output.querySelectorAll(".hot-card"));
    expect(cards).toHaveLength(52);
    expect(cards.filter((element) => !element.classList.contains("gone"))).toHaveLength(47);

    // The list under it, best first, and every card accounted for.
    const listed = Array.from(app.output.querySelectorAll<HTMLElement>(".hot-row"));
    expect(listed).toHaveLength(47);
    const values = listed.map((line) =>
      Number(line.lastElementChild!.textContent!.replace("%", "")),
    );
    expect(values).toEqual([...values].sort((a, b) => b - a));
    expect(app.output.textContent).toMatch(/\d+ cards increase equity, \d+ decrease it/);
  });
});

describe("the library", () => {
  test("walks the depths and the games, and the light follows", async () => {
    const app = await open();

    // A hundred blinds on the button: wide.
    chip(app, "mtt", "open", "BTN").click();
    app.render();
    const deep = state().players[state().active].percent;
    expect(deep).toBeGreaterThan(50);

    // Twenty blinds, big blind against an under-the-gun raise: wider still,
    // because it is cheap to call.
    stack(app, "mtt", "20bb").click();
    app.render();
    chip(app, "mtt", "defend", "UTG").click();
    app.render();
    expect(state().players[state().active].percent).toBeGreaterThan(deep);

    // And the raked cash game, where the same seat opens tighter.
    stack(app, "cash", "NL25").click();
    app.render();
    chip(app, "cash", "open", "BTN").click();
    app.render();
    const raked = state().players[state().active].percent;
    expect(raked).toBeLessThan(deep);
    expect(chip(app, "cash", "open", "BTN").classList.contains("active")).toBe(true);

    // The same game without the rake, which is the one thing that separates the
    // two chips: paying rake on every pot is a reason to play fewer of them, so
    // the rakeless button opens wider than the raked one.
    stack(app, "cash", "cEV").click();
    app.render();
    chip(app, "cash", "open", "BTN").click();
    app.render();
    expect(state().players[state().active].percent).toBeGreaterThan(raked);

    // Rakeless six-max is also shallow enough for the small blind to limp, so
    // the big blind has an isolate the raked set has no chart for.
    expect(chip(app, "cash", "defend", "SB limp")).toBeDefined();

    // Editing the range by hand puts the light out: it is no longer that chart.
    type(app, "22+");
    expect(
      app.range.querySelectorAll(".seat-chip.active"),
      "no chip should claim a hand-typed range",
    ).toHaveLength(0);
  });

  test("clearing the marks does not leave the next range with no opinion", async () => {
    const app = await open();
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    expect(reading(app, "top pair").mark).toBe("blue");

    headButton(app.stats, "Clear").click();
    app.render();
    expect(reading(app, "top pair").mark).toBe("none");

    // A different range is a different question, so the default comes back
    // rather than leaving a range the panel says nothing about.
    stack(app, "cash", "NL25").click();
    app.render();
    chip(app, "cash", "open", "BTN").click();
    app.render();
    expect(reading(app, "top pair").mark).toBe("blue");
    expect(app.output.querySelectorAll(".pie-key").length).toBeGreaterThan(1);
  });
});

describe("the flops panel", () => {
  test("deals a flop of the kind asked for, and says which kind it was", async () => {
    const app = await open();
    const flopRow = (label: string) =>
      Array.from(app.flops.querySelectorAll<HTMLElement>(".flop-row")).find(
        (candidate) => candidate.querySelector(".flop-label")?.textContent === label,
      )!;

    flopRow("Monotone").querySelector<HTMLButtonElement>(".deal-flop")!.click();
    app.render();
    let cards = state().boardCards;
    expect(cards).toHaveLength(3);
    expect(new Set(cards.map((name) => name[1])).size).toBe(1);
    expect(flopRow("Monotone").classList.contains("current")).toBe(true);

    flopRow("Trips").querySelector<HTMLButtonElement>(".deal-flop")!.click();
    app.render();
    cards = state().boardCards;
    expect(new Set(cards.map((name) => name[0])).size).toBe(1);
    expect(flopRow("Trips").classList.contains("current")).toBe(true);
    expect(flopRow("Monotone").classList.contains("current")).toBe(false);

    // Folded away it is a strip, and the summary is still there. It counts all
    // 22,100 whatever is on the table: what is being asked is how often a kind
    // of flop comes, and the one in front of you is one of them. Only your own
    // cards take flops out of the reckoning, because only they are unavailable.
    app.flops.querySelector<HTMLButtonElement>(".fold")!.click();
    app.render();
    expect(app.flops.querySelectorAll(".flop-row")).toHaveLength(0);
    expect(app.flops.textContent).toMatch(/22,100 flops/);
  });
});

describe("the keyboard", () => {
  test("carries a whole session without touching the mouse", async () => {
    const app = await open();
    type(app, "AA,KK");

    app.press("r");
    expect(state().boardCards).toHaveLength(3);
    expect(state().street).toBe("flop");

    app.press("Tab");
    expect(chrome.showCombos).toBe(true);
    app.press("Tab");

    app.press("1");
    expect(state().filtersEnabled).toBe(true);
    app.press("1");
    expect(state().filtersEnabled).toBe(false);

    app.press("s");
    expect(state().active).toBe(1);
    app.press("s");
    expect(state().active).toBe(0);

    app.press("]");
    expect(chrome.output).toBe("overlap");
    app.press("[");
    expect(chrome.output).toBe("groups");

    app.press("s", { altKey: true });
    expect(
      state()
        .groupShares.slice(1)
        .every((share) => share === 0),
    ).toBe(true);

    app.press("Backspace");
    expect(state().board).toBe("");

    app.press("?");
    expect(app.sheet.hidden).toBe(false);
    app.press("Escape");
    expect(app.sheet.hidden).toBe(true);
  });
});

describe("sharing a session", () => {
  test("a link carries the board, the painting and the filters", async () => {
    const app = await open();
    type(app, "AA,KK,QQ");
    card(app.board, "Kh");
    card(app.board, "7h");
    card(app.board, "2c");
    headButton(app.stats, "Clear").click();
    app.render();
    swatch(app, "red").click();
    row(app, "set").click();
    app.render();
    // And one hand out of a category by hand, so a gear has to survive too.
    row(app, "set").dispatchEvent(
      new window.MouseEvent("pointerdown", { bubbles: true, shiftKey: true }),
    );
    app.render();
    app.range.querySelector<HTMLButtonElement>(".edit-strip .combo-chip")!.click();
    app.render();
    streets(app)[0].click();
    app.render();

    const before = {
      board: state().board,
      notation: state().players[state().active].notation,
      pass: state().passFraction,
      mark: reading(app, "set").mark,
      street: streets(app)[0].textContent,
      shares: [...state().groupShares],
    };
    expect(before.mark).toBe("mixed");
    expect(before.pass).toBeLessThan(1);

    // The link is the session, so reopening from it has to land on the same
    // screen - including the parts nothing else could re-derive.
    const packed = snapshot();
    const reopened = await open();
    restore(packed);
    reopened.render();

    expect(state().board).toBe(before.board);
    expect(state().players[state().active].notation).toBe(before.notation);
    expect(state().passFraction).toBeCloseTo(before.pass, 9);
    expect(reading(reopened, "set").mark).toBe("mixed");
    expect(streets(reopened)[0].textContent).toBe(before.street);
    expect([...state().groupShares]).toEqual(before.shares);
  });

  test("a malformed link is refused without taking the app down", async () => {
    const app = await open();
    const was = state().players[state().active].notation;

    // A link somebody has edited by hand is reported, not thrown: losing the
    // session you are working on because a link was wrong is the worst of both.
    restore("{ not json");
    app.render();
    expect(state().players[state().active].notation).toBe(was);
    expect(app.range.querySelectorAll(".cell")).toHaveLength(169);

    // And one that parses but describes nonsense is refused the same way.
    restore('{"version":1,"board":"Zz 7h 2c","dead":"","active":0,"players":[]}');
    app.render();
    expect(state().players[state().active].notation).toBe(was);
  });
});

describe("over every flop at once", () => {
  test("reports how often one hand hits, with nothing on the board", async () => {
    const app = await open();
    // One combination keeps the pass over 22,100 flops quick.
    type(app, "AhKh");
    expect(state().board).toBe("");
    expect(app.stats.classList.contains("preflop-mode")).toBe(true);

    const button = app.stats.querySelector<HTMLButtonElement>(".filter-toggle")!;
    expect(button.textContent).toMatch(/Calculate over all 22,100 flops/);

    // The ladder is there to be marked before the pass has run.
    row(app, "top pair").click();
    app.render();
    expect(state().checkmarks[statDefs.find((d) => d.key === "top-pair")!.index]).toBe(true);
    expect(row(app, "top pair").querySelector(".filter-mark")!.textContent).toBe("✓");

    button.click();
    await vi.waitFor(() => expect(chrome.preflopRunning).toBe(false), { timeout: 30000 });
    app.render();

    // Ace-king suited makes top pair or better on rather a lot of flops.
    expect(chrome.preflop!.hit).toBeGreaterThan(0.2);
    expect(chrome.preflop!.hit).toBeLessThan(0.6);
    expect(app.stats.querySelector(".effective")!.textContent).toMatch(
      /Hits \d+\.\d\d% of the time/,
    );
    expect(reading(app, "flushdraw").value).toMatch(/^\d+\.\d%$/);
  }, 60000);

  test("a tick asks the question, and asking it twice does not re-run the pass", async () => {
    const app = await open();
    type(app, "AhKh");

    // Nothing ticked: the panel says what the number would be about rather
    // than reporting a hit rate of nought.
    const foot = () => app.stats.querySelector(".effective")!.textContent ?? "";
    expect(foot()).toMatch(/averaged over every one of them/);

    // A tick before the pass has run is a question about a pass, so it asks
    // for one. Before this, the mark went on and not one number moved.
    row(app, "top pair").click();
    app.render();
    await vi.waitFor(() => expect(chrome.preflopRunning).toBe(false), { timeout: 30000 });
    app.render();
    const withTopPair = chrome.preflop!.hit;
    expect(withTopPair).toBeGreaterThan(0.2);
    expect(foot()).toMatch(/Hits \d+\.\d\d% of the time/);

    // A second tick re-reads the pass instead of walking 22,100 flops again:
    // which statistics count as a hit does not change what the range does.
    const rows = statDefs
      .filter((definition) => !row(app, definition.label).hidden)
      .map((definition) => reading(app, definition.label).value);
    row(app, "two pair").click();
    app.render();
    expect(chrome.preflopRunning, "no second pass").toBe(false);
    expect(chrome.preflop, "the pass is still on screen").not.toBeNull();
    const both = chrome.preflop!.hit;
    expect(both).toBeGreaterThan(withTopPair);
    // The per-row numbers are about the range, not about the ticks, so they
    // do not move at all.
    const after = statDefs
      .filter((definition) => !row(app, definition.label).hidden)
      .map((definition) => reading(app, definition.label).value);
    expect(after).toEqual(rows);

    // Two rungs of the made-hand ladder cannot both be true of one hand, so
    // ticking both adds up exactly.
    row(app, "top pair").click();
    app.render();
    const twoPairAlone = chrome.preflop!.hit;
    expect(twoPairAlone + withTopPair).toBeCloseTo(both, 6);

    // Unticking everything leaves the question rather than a hit rate of nought.
    row(app, "two pair").click();
    app.render();
    expect(foot()).toMatch(/Tick the statistics that count as hitting/);

    // And changing the range does retire the pass, because it was a pass over
    // that range.
    row(app, "top pair").click();
    app.render();
    await vi.waitFor(() => expect(chrome.preflopRunning).toBe(false), { timeout: 30000 });
    type(app, "AhQh");
    app.render();
    expect(chrome.preflop).toBeNull();
    expect(foot()).toMatch(/averaged over every one of them/);
  }, 60000);
});
