// @vitest-environment jsdom
/**
 * The interface itself.
 *
 * These build the real panels against the real engine and drive them the way a
 * pointer would, so a change that breaks rendering or wiring fails here rather
 * than in someone's browser.
 */

import { afterAll, beforeAll, describe, expect, test, vi } from "vitest";

import { boot, chrome, mutate, state, statDefs } from "../src/store";
import { createMenuBar } from "../src/ui/menubar";
import { imageName, rangeImage } from "../src/ui/rangeImage";
import { createTopStrip } from "../src/ui/topStrip";
import { createRangePanel } from "../src/ui/rangePanel";
import { createBoardPanel } from "../src/ui/boardPanel";
import { createStatsPanel } from "../src/ui/statsPanel";
import { createWorkspace, fitColumns, loadColumns } from "../src/ui/workspace";
import { createOutputPanel } from "../src/ui/outputPanel";
import { createFlopsPanel } from "../src/ui/flopsPanel";
import { BINDINGS, createHotkeySheet, installHotkeys } from "../src/ui/hotkeys";
import { cycleTheme, themeLabel } from "../src/ui/theme";
import { wasmBytes } from "./harness";

type Panel = { element: HTMLElement; render: () => void };

let panels: Panel[];
let strip: Panel;
let output: Panel;
let flops: Panel;
let range: Panel;
let board: Panel;
let stats: Panel;

function renderAll(): void {
  panels.forEach((panel) => panel.render());
}

function point(element: Element, type: string, init: MouseEventInit = {}): void {
  element.dispatchEvent(new window.MouseEvent(type, { bubbles: true, ...init }));
}

/** Clicks a card in one of the 4x13 grids. */
/**
 * Deals exactly this board with nothing dead, and hands back a restore.
 *
 * The panels share one session, so a test that needs a particular board has to
 * put back what it found or the next one reads someone else's cards.
 */
function onBoard(text: string): () => void {
  // The board panel deals from chrome.boardCards, not from the engine, so both
  // have to move together or the next click replays the old cards.
  function deal(cards: string[], dead: string, visible = cards.length): void {
    chrome.boardCards = [...cards];
    chrome.visible = visible;
    mutate((engine) => {
      engine.setDead("");
      engine.setBoard(cards.slice(0, visible).join(" "));
      engine.setDead(dead);
    });
    renderAll();
  }

  const dead = state().dead.join(" ");
  const dealt = [...chrome.boardCards];
  const visible = chrome.visible;
  deal(text.split(" "), "");
  return () => deal(dealt, dead, visible);
}

/** The thumbnail that selects one seat. */
function seatTab(index: number): HTMLButtonElement {
  return strip.element.querySelectorAll<HTMLButtonElement>(".seat")[index];
}

function pickCard(panel: Panel, card: string): void {
  const cell = panel.element.querySelector<HTMLButtonElement>(`.card-cell[data-card="${card}"]`);
  if (!cell) throw new Error(`no ${card} in this grid`);
  cell.click();
  renderAll();
}

/** Types a range into the notation field the way a person would. */
function setRange(text: string): void {
  const notation = range.element.querySelector<HTMLTextAreaElement>(".notation")!;
  notation.dispatchEvent(new window.Event("focus"));
  notation.value = text;
  notation.dispatchEvent(new window.Event("blur"));
  renderAll();
}

function statRow(label: string): HTMLElement {
  const row = Array.from(stats.element.querySelectorAll<HTMLElement>(".stat-row")).find(
    (node) => node.querySelector(".stat-label")?.textContent === label,
  );
  if (!row) throw new Error(`no statistic row called ${label}`);
  return row;
}

/** A statistic's registry index, by key. */
function statDefIndex(key: string): number {
  const found = statDefs.find((def) => def.key === key);
  if (!found) throw new Error(`no statistic called ${key}`);
  return found.index;
}

/** A preflop row's fraction, by statistic label. */
function topPairRowValue(key: string): number {
  const row = chrome.preflop?.rows.find((candidate) => candidate.key === key);
  return row?.fraction ?? 0;
}

/** A button in a panel's header, by the word on it. */
function headButton(panel: Panel, label: string): HTMLButtonElement {
  const found = headButtons(panel).find((button) => button.textContent === label);
  if (!found) throw new Error(`no ${label} button in the header`);
  return found;
}

function headButtons(panel: Panel): HTMLButtonElement[] {
  return Array.from(panel.element.querySelectorAll<HTMLButtonElement>(".panel-head .btn"));
}

beforeAll(async () => {
  await boot(await wasmBytes());

  const menubar = createMenuBar();
  strip = createTopStrip();
  range = createRangePanel();
  board = createBoardPanel();
  stats = createStatsPanel();
  output = createOutputPanel();
  flops = createFlopsPanel();
  panels = [menubar, strip, range, board, stats, output, flops];
  document.body.replaceChildren(...panels.map((panel) => panel.element));
  renderAll();
});

describe("the range panel", () => {
  test("draws a full matrix with the usual layout", () => {
    const cells = range.element.querySelectorAll(".cell");
    expect(cells).toHaveLength(169);
    const label = (index: number) => cells[index].querySelector(".cell-label")?.textContent;
    expect(label(0)).toBe("AA");
    expect(label(1)).toBe("AKs");
    expect(label(13)).toBe("AKo");
    expect(label(168)).toBe("22");
    expect(cells[0].classList.contains("pair")).toBe(true);
    expect(cells[1].classList.contains("suited")).toBe(true);
    expect(cells[13].classList.contains("offsuit")).toBe(true);
  });

  test("opens on a button chart rather than an empty matrix", () => {
    // The most-looked-at spot there is, so there are numbers on the first frame.
    const percent = state().players[0].percent;
    expect(percent).toBeGreaterThan(50);
    expect(percent).toBeLessThan(58);
    expect(chrome.libraryStack).toBe("100bb");
    // The solver's mix survives: some cells are part weight.
    expect(state().classWeights.some((w) => w > 0 && w < 1)).toBe(true);
  });

  test("the two handles cannot overtake each other", () => {
    const cut = range.element.querySelector<HTMLInputElement>(".slider-cut")!;
    const top = range.element.querySelector<HTMLInputElement>(".slider-top")!;

    top.value = "40";
    top.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();
    expect(chrome.window.high).toBe(40);

    // Dragging the lower handle past the upper one stops it at the upper one.
    cut.value = "70";
    cut.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();
    expect(chrome.window.low).toBe(40);
    expect(Number(cut.value)).toBe(40);
    expect(state().players[state().active].combos).toBe(0);

    cut.value = "0";
    cut.dispatchEvent(new window.Event("input", { bubbles: true }));
    top.value = "100";
    top.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();
  });

  test("the weight slider sets what a drag paints", () => {
    const weight = range.element.querySelector<HTMLInputElement>(".weight-slider")!;
    expect(weight.value).toBe("100");
    weight.value = "50";
    weight.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();
    expect(chrome.brush).toBe(0.5);
    expect(range.element.querySelector(".weight-value")!.textContent).toBe("50%");

    weight.value = "100";
    weight.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();
  });

  test("dragging across cells paints them and shows their combo counts", () => {
    const cells = range.element.querySelectorAll<HTMLElement>(".cell");
    // Start from empty so a press paints rather than erases.
    Array.from(range.element.querySelectorAll<HTMLButtonElement>(".quick"))[4].click();
    renderAll();
    expect(state().classWeights[0]).toBe(0);

    point(cells[0], "pointerdown");
    point(cells[1], "pointerenter");
    window.dispatchEvent(new window.Event("pointerup"));
    renderAll();

    expect(state().classWeights[0]).toBe(1);
    expect(state().classWeights[1]).toBe(1);
    expect(cells[0].classList.contains("on")).toBe(true);
    expect(cells[0].querySelector(".cell-count")?.textContent).toBe("6");
    expect(cells[1].querySelector(".cell-count")?.textContent).toBe("4");

    // Pressing a painted cell erases instead.
    point(cells[0], "pointerdown");
    window.dispatchEvent(new window.Event("pointerup"));
    renderAll();
    expect(state().classWeights[0]).toBe(0);
    expect(cells[0].querySelector(".cell-count")?.textContent).toBe("");
  });

  test("a partial weight fills part of the cell rather than fading it", () => {
    const cells = range.element.querySelectorAll<HTMLElement>(".cell");
    const weight = range.element.querySelector<HTMLInputElement>(".weight-slider")!;
    weight.value = "25";
    weight.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();

    point(cells[0], "pointerdown");
    window.dispatchEvent(new window.Event("pointerup"));
    renderAll();
    expect(state().classWeights[0]).toBeCloseTo(0.25, 6);
    expect(cells[0].style.getPropertyValue("--fill")).toBe("25.0%");
    expect(cells[0].classList.contains("partial")).toBe(true);
    expect(cells[0].querySelector(".cell-count")?.textContent).toBe("1.5");

    weight.value = "100";
    weight.dispatchEvent(new window.Event("input", { bubbles: true }));
    Array.from(range.element.querySelectorAll<HTMLButtonElement>(".quick"))[4].click();
    renderAll();
  });

  test("the slider fills the range and the summary follows", () => {
    const slider = range.element.querySelector<HTMLInputElement>(".slider-top")!;
    slider.value = "15";
    slider.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();

    expect(state().players[state().active].combos).toBeGreaterThan(0);
    expect(range.element.querySelector(".summary")!.textContent).toMatch(/combos in range/);
    expect(
      range.element.querySelector<HTMLTextAreaElement>(".notation")!.value.length,
    ).toBeGreaterThan(0);
  });

  test("the lower handle carves the strongest hands back out", () => {
    const top = range.element.querySelector<HTMLInputElement>(".slider-top")!;
    const cut = range.element.querySelector<HTMLInputElement>(".slider-cut")!;
    top.value = "20";
    top.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();
    const wide = state().players[state().active].combos;

    cut.value = "5";
    cut.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();
    expect(state().players[state().active].combos).toBeLessThan(wide);

    cut.value = "0";
    cut.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();
  });

  test("the library loads a solver chart in two clicks", () => {
    const stacks = Array.from(
      range.element.querySelectorAll<HTMLButtonElement>(".library-mtt .stack-chip"),
    );
    expect(stacks.map((chip) => chip.textContent)).toEqual([
      "100bb",
      "80bb",
      "60bb",
      "40bb",
      "20bb",
    ]);
    expect(stacks[0].classList.contains("active")).toBe(true);

    const opens = () =>
      Array.from(range.element.querySelectorAll<HTMLButtonElement>(".action-open .seat-chip"));
    const defends = () =>
      Array.from(range.element.querySelectorAll<HTMLButtonElement>(".action-defend .seat-chip"));
    expect(opens().map((chip) => chip.textContent)).toEqual([
      "UTG",
      "UTG1",
      "LJ",
      "HJ",
      "CO",
      "BTN",
      "SB",
    ]);
    // The big blind also faces a limp, which is its own chart.
    expect(defends().map((chip) => chip.textContent)).toEqual([
      "UTG",
      "UTG1",
      "LJ",
      "HJ",
      "CO",
      "BTN",
      "SB",
      "SB limp",
    ]);

    // The chip says which seat; the tooltip says the size and what the chart is.
    expect(opens()[0].title).toMatch(/UTG opens to 2.1 bb/);
    expect(defends()[5].title).toMatch(/BTN raise to 2.5 bb/);
    expect(defends()[7].title).toMatch(/over a small-blind limp/);

    opens()[0].click();
    renderAll();
    const utgOpen = state().players[state().active].percent;
    expect(utgOpen).toBeGreaterThan(16);
    expect(utgOpen).toBeLessThan(18);

    opens()[5].click();
    renderAll();
    expect(state().players[state().active].percent).toBeGreaterThan(50);

    // Defending is far wider than opening from the same seat.
    defends()[0].click();
    renderAll();
    expect(state().players[state().active].percent).toBeGreaterThan(utgOpen * 3);

    // The solver's mix survives rather than being rounded into whole combos.
    expect(state().players[state().active].notation).toMatch(/:/);

    // Switching depth reloads the same chip with the other solution.
    stacks[1].click();
    renderAll();
    expect(chrome.libraryStack).toBe("80bb");
    expect(opens()[0].title).toMatch(/UTG opens to 2 bb/);
    opens()[0].click();
    renderAll();
    expect(state().players[state().active].percent).toBeGreaterThan(16);

    // Twenty blinds has the defences too now, but no limp to isolate: that
    // shallow the small blind raises or folds.
    stacks[4].click();
    renderAll();
    expect(chrome.libraryStack).toBe("20bb");
    expect(opens()).toHaveLength(7);
    expect(defends().map((chip) => chip.textContent)).toEqual([
      "UTG",
      "UTG1",
      "LJ",
      "HJ",
      "CO",
      "BTN",
      "SB",
    ]);

    stacks[0].click();
    renderAll();
  });

  test("shift-clicking a cell pins its suit breakdown", () => {
    const cells = range.element.querySelectorAll<HTMLElement>(".cell");
    Array.from(range.element.querySelectorAll<HTMLButtonElement>(".quick"))[4].click();
    renderAll();

    // AKs: four combos, none of them selected yet.
    point(cells[1], "pointerdown", { shiftKey: true });
    renderAll();
    expect(chrome.suitCell).toBe(1);
    const popup = range.element.querySelector<HTMLElement>(".suit-popup")!;
    expect(popup.hidden).toBe(false);
    const held = Array.from(popup.querySelectorAll<HTMLButtonElement>(".suit-cell:not(.absent)"));
    expect(held).toHaveLength(4);
    expect(held.every((element) => !element.classList.contains("on"))).toBe(true);

    // Turning one on puts a single combo in the range.
    point(held[0], "pointerdown");
    window.dispatchEvent(new window.Event("pointerup"));
    renderAll();
    expect(state().players[state().active].combos).toBe(1);
    expect(state().classWeights[1]).toBeCloseTo(0.25, 6);
    expect(popup.querySelectorAll(".suit-cell.on")).toHaveLength(1);

    // And off again.
    point(popup.querySelectorAll<HTMLButtonElement>(".suit-cell.on")[0], "pointerdown");
    window.dispatchEvent(new window.Event("pointerup"));
    renderAll();
    expect(state().players[state().active].combos).toBe(0);

    // Shift-clicking the same cell again unpins it.
    point(cells[1], "pointerdown", { shiftKey: true });
    renderAll();
    expect(chrome.suitCell).toBeNull();
    expect(range.element.querySelector<HTMLElement>(".suit-popup")!.hidden).toBe(true);
  });

  test("the ranking dropdown offers more than one ordering", () => {
    const select = range.element.querySelector<HTMLSelectElement>(".select")!;
    expect(select.hidden).toBe(false);
    expect(select.options.length).toBeGreaterThan(1);
    expect(Array.from(select.options).map((option) => option.value)).toContain("chen");
  });

  test("the quick buttons add whole groups of hands", () => {
    // Two rows: the shapes of the matrix with Clear, then the value ranges.
    const quick = Array.from(
      range.element.querySelectorAll<HTMLButtonElement>(".quick-row:not(.value-row) .quick"),
    );
    expect(quick.map((button) => button.textContent)).toEqual([
      "All",
      "Pocket",
      "Broadway",
      "Suited",
      "Clear",
    ]);
    const values = Array.from(
      range.element.querySelectorAll<HTMLButtonElement>(".value-row .quick"),
    );
    expect(values.map((button) => button.textContent)).toEqual(["QQ+/AK", "TT+/AQ+", "99+/AJ+/KQ"]);

    quick[4].click(); // Clear
    renderAll();
    expect(state().players[state().active].combos).toBe(0);

    quick[1].click(); // Pocket
    renderAll();
    expect(state().players[state().active].combos).toBe(78);

    quick[0].click(); // All
    renderAll();
    expect(state().players[state().active].combos).toBe(1326);

    quick[4].click();
    renderAll();
    values[1].click(); // TT+/AQ+
    renderAll();
    expect(state().players[state().active].combos).toBe(62);

    quick[4].click();
    renderAll();
  });
});

describe("the range as a picture", () => {
  test("the file is named after the board it is of", () => {
    setRange("22+");
    const preflop = imageName();
    expect(preflop).toBe("kongzilla-preflop.png");

    const restore = onBoard("Kh 7h 2c");
    renderAll();
    expect(imageName()).toBe("kongzilla-kh7h2c.png");
    restore();
    renderAll();
  });

  test("the title bar offers it, with a key that the sheet lists", () => {
    const button = Array.from(document.querySelectorAll<HTMLButtonElement>(".menubar .btn")).find(
      (element) => element.textContent === "Image",
    );
    expect(button).toBeDefined();
    expect(button!.title).toBe("Image (Ctrl+P)");
  });

  test("a browser with no canvas says so rather than saving nothing", async () => {
    // jsdom draws nothing, which is the same position an old browser is in: the
    // point is that it fails loudly instead of writing an empty file.
    await expect(rangeImage()).rejects.toThrow(/canvas/);
  });
});

describe("the top strip", () => {
  test("shows one thumbnail per seat and switches between them", () => {
    const seats = strip.element.querySelectorAll<HTMLButtonElement>(".seat");
    expect(seats).toHaveLength(2);
    expect(seats[0].querySelectorAll(".thumb-matrix i")).toHaveLength(169);

    seats[1].click();
    renderAll();
    expect(state().active).toBe(1);
    expect(seats[1].classList.contains("active")).toBe(true);

    seats[0].click();
    renderAll();
    expect(state().active).toBe(0);
  });

  test("the dead-card grid holds all 52 cards", () => {
    expect(strip.element.querySelectorAll(".dead-grid .card-cell")).toHaveLength(52);
  });
});

describe("the board panel", () => {
  test("picking cards builds a flop", () => {
    pickCard(board, "Kc");
    pickCard(board, "Qh");
    pickCard(board, "Jh");

    expect(state().board).toBe("Kc Qh Jh");
    expect(state().street).toBe("flop");
    expect(board.element.querySelector(".street-name")!.textContent).toBe("flop");
    expect(board.element.querySelectorAll(".board-slot:not(.empty)")).toHaveLength(3);
    expect(
      board.element
        .querySelector<HTMLElement>('.card-cell[data-card="Kc"]')!
        .classList.contains("picked"),
    ).toBe(true);
    expect(board.element.querySelector(".tally")!.textContent).toMatch(/combos/);
  });

  test("the tally counts combos the board has not already taken", () => {
    setRange("KK");
    // The king of clubs is on the board, so three of the six combos are gone.
    expect(state().players[state().active].combos).toBe(6);
    expect(state().liveCombos).toBe(3);
    expect(board.element.querySelector(".tally")!.textContent).toBe(
      "3 combos, none filtered out yet.",
    );
  });

  test("the street arrows hide and restore cards", () => {
    pickCard(board, "Ts");
    expect(state().street).toBe("turn");

    const [back, forward] = board.element.querySelectorAll<HTMLButtonElement>(".street-arrow");
    back.click();
    renderAll();
    expect(state().street).toBe("flop");
    expect(chrome.boardCards).toHaveLength(4);

    forward.click();
    renderAll();
    expect(state().street).toBe("turn");

    back.click();
    renderAll();
  });

  test("dead cards only take cards out of the deck", () => {
    setRange("QQ+, AKs");
    const before = state().liveCombos;
    pickCard(strip, "Ah");
    pickCard(strip, "Kh");

    // They used to be read as a hand as well. Now they say the one thing they
    // are for: those cards are not in the deck.
    expect(state().hand).toBeNull();
    expect(state().liveCombos).toBeLessThan(before);
    expect(state().dead).toEqual(["Kh", "Ah"]);
  });
});

describe("the statistics panel", () => {
  test("renders Flopzilla's ladder under three headings", () => {
    const headings = Array.from(stats.element.querySelectorAll(".stat-block .sub-title")).map(
      (node) => node.textContent,
    );
    expect(headings).toEqual(["Made hands", "Draws", "Combinations"]);

    const labels = Array.from(
      stats.element.querySelectorAll<HTMLElement>(".stat-row:not([hidden]) .stat-label"),
    ).map((node) => node.textContent);
    // The pair ladder interleaves board pairs and pocket pairs, as Flopzilla does.
    const ladder = labels.slice(labels.indexOf("overpair"), labels.indexOf("ace high"));
    expect(ladder).toEqual([
      "overpair",
      "top pair",
      "pp < top card",
      "middle pair",
      "pp < 2nd card",
      "bottom pair",
      "pp < board",
    ]);
    expect(labels).toContain("oesd (2 card)");
    expect(labels).toContain("gutshot (1 crd)");
  });

  test("painting a row marks it; a street filter is what narrows the range", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.clearFilters());
    renderAll();
    const row = statRow("top pair");
    const mark = () => row.querySelector<HTMLElement>(".filter-mark")!;

    expect(mark().className).toContain("mark-none");
    row.click();
    renderAll();
    expect(mark().className).toContain("mark-blue");
    // Painting says which hands are which. It does not remove any of them.
    expect(state().filtersEnabled).toBe(false);
    expect(state().passFraction).toBeCloseTo(1, 9);

    // Clicking a row already in the held colour unpaints it.
    row.click();
    renderAll();
    expect(mark().className).toContain("mark-none");

    // Another colour off the palette.
    const swatch = (colour: string) =>
      stats.element.querySelector<HTMLButtonElement>(`.swatch.colour-${colour}`)!;
    swatch("green").click();
    renderAll();
    expect(swatch("green").classList.contains("active")).toBe(true);
    row.click();
    renderAll();
    expect(mark().className).toContain("mark-green");

    // The street button is what narrows it, and it is a toggle.
    const street = stats.element.querySelectorAll<HTMLButtonElement>(".street-filter")[0];
    expect(street.hidden).toBe(false);
    // The count is there whether the filter is on or off, and a red lamp says
    // which - Flopzilla shows both, and "how much would this leave" is the
    // question you ask before pressing, not after.
    expect(street.textContent).toMatch(/^Flop · \d/);
    expect(street.querySelector(".lamp")!.classList.contains("on")).toBe(false);

    street.click();
    renderAll();
    expect(state().filtersEnabled).toBe(true);
    expect(state().passFraction).toBeLessThan(1);
    expect(street.textContent).toMatch(/^Flop · \d/);
    expect(street.querySelector(".lamp")!.classList.contains("on")).toBe(true);
    expect(street.classList.contains("active")).toBe(true);

    street.click();
    renderAll();
    expect(state().filtersEnabled).toBe(false);
    expect(street.querySelector(".lamp")!.classList.contains("on")).toBe(false);
    expect(state().passFraction).toBeCloseTo(1, 9);

    swatch("blue").click();
    headButton(stats, "Clear").click();
    renderAll();
    restore();
  });

  test("the footer says what Flopzilla says, and not the range in notation", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.clearFilters());
    statRow("top pair").click();
    renderAll();
    const footer = () => stats.element.querySelector(".effective")!;

    expect(footer().textContent).toMatch(/Total number of combos: \d/);
    expect(footer().textContent).toMatch(/The filters are OFF/);
    // Not a wall of notation in the one corner nobody reads a range from.
    expect(footer().textContent).not.toMatch(/A2s\+/);
    expect(footer().textContent).not.toContain(":0.");

    stats.element.querySelectorAll<HTMLButtonElement>(".street-filter")[0].click();
    renderAll();
    expect(footer().textContent).toMatch(/Combos that pass the filters: \d+.*\(\d+\.\d\d%\)/);
    expect(footer().textContent).toMatch(/The filters are ON/);

    headButton(stats, "Clear").click();
    renderAll();
    restore();
  });

  test("a gear goes when a hand is put back the way it was", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.clearFilters());
    renderAll();
    const row = statRow("flushdraw");
    const mark = () => row.querySelector<HTMLElement>(".filter-mark")!;

    row.click();
    renderAll();
    expect(mark().className).toContain("mark-blue");

    point(row, "pointerdown", { shiftKey: true });
    renderAll();
    const chips = () =>
      Array.from(range.element.querySelectorAll<HTMLButtonElement>(".edit-strip .combo-chip"));
    expect(chips().length).toBeGreaterThan(1);

    // Out: the category has been picked over, so it is a gear.
    chips()[0].click();
    renderAll();
    expect(mark().className).toContain("mark-mixed");

    // And straight back in: nothing has changed, so there is nothing to warn
    // about. Doing and undoing is not the same as doing.
    chips()[0].click();
    renderAll();
    expect(mark().className).toContain("mark-blue");

    point(row, "pointerdown", { shiftKey: true });
    headButton(stats, "Clear").click();
    renderAll();
    restore();
  });

  test("a category whose hands disagree shows a gear and opens for editing", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.clearFilters());
    renderAll();
    const row = statRow("flushdraw");
    const mark = () => row.querySelector<HTMLElement>(".filter-mark")!;

    row.click();
    renderAll();
    expect(mark().className).toContain("mark-blue");

    // Shift-click opens the category's hands over the matrix.
    point(row, "pointerdown", { shiftKey: true });
    renderAll();
    const strip = range.element.querySelector<HTMLElement>(".edit-strip")!;
    expect(strip.hidden).toBe(false);
    const chips = Array.from(strip.querySelectorAll<HTMLButtonElement>(".combo-chip"));
    expect(chips.length).toBeGreaterThan(1);
    expect(chips.every((chip) => chip.classList.contains("paint-blue"))).toBe(true);

    // Unpaint one of them and the category stops speaking for all of them.
    chips[0].click();
    renderAll();
    expect(mark().className).toContain("mark-mixed");
    expect(mark().textContent).toBe("⚙");

    point(row, "pointerdown", { shiftKey: true });
    renderAll();
    expect(range.element.querySelector<HTMLElement>(".edit-strip")!.hidden).toBe(true);

    headButton(stats, "Clear").click();
    renderAll();
    restore();
  });

  test("the share slider paints the top of the range by equity", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.clearFilters());
    renderAll();
    const slider = stats.element.querySelector<HTMLInputElement>(".share-slider")!;
    const readout = stats.element.querySelector(".share-value")!;
    const clear = stats.element.querySelector<HTMLButtonElement>(".cut-clear")!;

    // Set something up by hand first, so the undo has something to put back.
    statRow("top pair").click();
    renderAll();
    expect(statRow("top pair").querySelector(".filter-mark")!.className).toContain("mark-blue");

    slider.value = "50";
    slider.dispatchEvent(new window.Event("input", { bubbles: true }));
    renderAll();

    // The readout names the equity the cut landed on, which is the number that
    // actually decides whether continuing is right.
    expect(readout.textContent).toMatch(/^\d+% · \d+%\+ eq$/);
    expect(chrome.cut).not.toBeNull();
    expect(chrome.cut!.covered).toBeGreaterThanOrEqual(0.5);

    // It paints. It does not narrow: the matrix only moves on a street filter.
    expect(state().filtersEnabled).toBe(false);
    expect(state().passFraction).toBeCloseTo(1, 9);
    const painted = state()
      .groupShares.slice(1)
      .reduce((sum, share) => sum + share, 0);
    const total = state().groupShares.reduce((sum, share) => sum + share, 0);
    expect(painted / total).toBeCloseTo(chrome.cut!.covered, 6);

    // It keeps hands the statistics ladder would have folded: on this board the
    // nut flushdraw has no made hand and is painted anyway.
    const street = stats.element.querySelectorAll<HTMLButtonElement>(".street-filter")[0];
    street.click();
    renderAll();
    expect(state().effectiveNotation).toMatch(/\bAhJh\b/);
    expect(state().effectiveNotation).not.toMatch(/\b44\b/);
    street.click();
    renderAll();

    // Undo puts back exactly what was there before the slider ran.
    clear.click();
    renderAll();
    expect(chrome.cut).toBeNull();
    expect(readout.textContent).toBe("all");
    expect(statRow("top pair").querySelector(".filter-mark")!.className).toContain("mark-blue");
    expect(statRow("no made hand").querySelector(".filter-mark")!.className).not.toContain(
      "mark-blue",
    );

    headButton(stats, "Clear").click();
    renderAll();
    restore();
  });

  test("the matrix says weight one way and filtering the other", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.clearFilters());
    renderAll();
    const cells = () => Array.from(range.element.querySelectorAll<HTMLElement>(".cell"));

    // One colour in use says nothing a cell does not already show, so the
    // colour mark is not drawn at all.
    statRow("flushdraw").click();
    renderAll();
    expect(state().coloursUsed).toBe(1);
    expect(range.element.querySelectorAll(".cell.grouped")).toHaveLength(0);

    // A second colour gives the mark something to tell apart.
    stats.element.querySelector<HTMLButtonElement>(".swatch.colour-green")!.click();
    statRow("top pair").click();
    renderAll();
    expect(state().coloursUsed).toBe(2);
    const marked = cells().filter((cell) => cell.classList.contains("grouped"));
    expect(marked.length).toBeGreaterThan(0);
    expect(
      marked.some((cell) => !cell.style.getPropertyValue("--groups").includes("100.00%")),
    ).toBe(true);

    // Painting filters nothing out by itself.
    expect(range.element.querySelectorAll(".cell.filtered")).toHaveLength(0);

    stats.element.querySelectorAll<HTMLButtonElement>(".street-filter")[0].click();
    renderAll();
    const filtered = cells().filter((cell) => cell.classList.contains("filtered"));
    expect(filtered.length).toBeGreaterThan(0);
    // The vertical split is how much of the cell got through, and it is a
    // different axis from the horizontal one that carries the weight.
    const shares = filtered.map((cell) => cell.style.getPropertyValue("--passing"));
    expect(shares).toContain("0.0%");
    expect(shares.some((share) => share !== "0.0%" && share !== "100.0%")).toBe(true);
    // A cell wholly through the filter is not marked as filtered at all.
    expect(
      cells().every(
        (cell) =>
          !cell.classList.contains("filtered") ||
          cell.style.getPropertyValue("--passing") !== "100.0%",
      ),
    ).toBe(true);

    stats.element.querySelector<HTMLButtonElement>(".swatch.colour-blue")!.click();
    headButton(stats, "Clear").click();
    renderAll();
    expect(range.element.querySelectorAll(".cell.filtered")).toHaveLength(0);
    restore();
  });

  test("the cash charts are a block of their own, not a chip in the MTT row", () => {
    const mtt = () => range.element.querySelector<HTMLElement>(".library-mtt")!;
    const cash = () => range.element.querySelector<HTMLElement>(".library-cash")!;
    const stack = (where: HTMLElement, label: string) =>
      Array.from(where.querySelectorAll<HTMLButtonElement>(".stack-chip")).find(
        (chip) => chip.textContent === label,
      );

    // Two headings, each naming its own game - a raked cash range and a chip-EV
    // range at the same depth are different answers, not two depths of one.
    expect(mtt().querySelector(".library-label")!.textContent).toBe("MTT chip-EV");
    expect(cash().querySelector(".library-label")!.textContent).toMatch(/cash/);
    expect(stack(mtt(), "NL25")).toBeUndefined();
    expect(
      Array.from(mtt().querySelectorAll<HTMLButtonElement>(".stack-chip")).map(
        (chip) => chip.textContent,
      ),
    ).toEqual(["100bb", "80bb", "60bb", "40bb", "20bb"]);

    // Its one depth is still a chip, because a chip is a button and reads as
    // one - it just does not stretch across the row.
    stack(cash(), "NL25")!.click();
    renderAll();
    expect(cash().classList.contains("open")).toBe(true);
    expect(mtt().classList.contains("open")).toBe(false);

    // Six-handed: no UTG1, no LJ, and no SB limp for the BB to isolate.
    const seats = (row: string) =>
      Array.from(cash().querySelectorAll<HTMLButtonElement>(`.action-${row} .seat-chip`)).map(
        (chip) => chip.textContent,
      );
    expect(seats("open")).toEqual(["UTG", "HJ", "CO", "BTN", "SB"]);
    expect(seats("defend")).toEqual(["UTG", "HJ", "CO", "BTN", "SB"]);
    // And the MTT block keeps its seats to itself while the cash one is open.
    expect(mtt().querySelectorAll(".action-open .seat-chip")).toHaveLength(0);

    // Rake makes the same seat open tighter than it would in a tournament.
    const btn = (where: HTMLElement) =>
      Array.from(where.querySelectorAll<HTMLButtonElement>(".action-open .seat-chip")).find(
        (chip) => chip.textContent === "BTN",
      )!;
    btn(cash()).click();
    renderAll();
    const raked = state().players[state().active].percent;
    stack(mtt(), "100bb")!.click();
    renderAll();
    btn(mtt()).click();
    renderAll();
    expect(raked).toBeLessThan(state().players[state().active].percent);
  });

  test("the chart chips follow the stack depth they are showing", () => {
    const chips = () =>
      Array.from(range.element.querySelectorAll<HTMLButtonElement>(".action-open .seat-chip"));
    const stack = (label: string) =>
      Array.from(range.element.querySelectorAll<HTMLButtonElement>(".stack-chip")).find(
        (chip) => chip.textContent === label,
      )!;

    stack("100bb").click();
    renderAll();
    const sb = chips().find((chip) => chip.textContent === "SB")!;
    sb.click();
    renderAll();
    const at100 = state().players[state().active].percent;
    expect(sb.classList.contains("active")).toBe(true);

    // The chips are reused across stacks, so the one that is clicked has to be
    // the one whose label is showing - not the one it was built with.
    stack("40bb").click();
    renderAll();
    const sb40 = chips().find((chip) => chip.textContent === "SB")!;
    expect(sb40.classList.contains("active")).toBe(false);
    sb40.click();
    renderAll();
    const at40 = state().players[state().active].percent;
    expect(at40).not.toBeCloseTo(at100, 1);
    // The tooltip and the range agree about what was loaded.
    expect(sb40.title).toContain(`${at40.toFixed(1)}% of hands`);

    // Editing the range by hand puts the light out.
    setRange("22+");
    renderAll();
    expect(sb40.classList.contains("active")).toBe(false);

    stack("100bb").click();
    renderAll();
  });

  test("a street filter outlives the board it was pressed on", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.clearFilters());
    renderAll();
    statRow("flushdraw").click();
    stats.element.querySelectorAll<HTMLButtonElement>(".street-filter")[0].click();
    renderAll();
    const chosen = state().effectiveNotation;
    expect(chosen).toMatch(/\bAhJh\b/);

    pickCard(board, "2s");
    pickCard(board, "3c");
    renderAll();

    // The flushdraw is not a flushdraw any more, but it is still what this range
    // continued with, so it is still here - and the turn's own button is now
    // offered alongside the flop's.
    expect(state().effectiveNotation).toMatch(/\bAhJh\b/);
    expect(statRow("flushdraw").classList.contains("empty")).toBe(true);
    const streets = stats.element.querySelectorAll<HTMLButtonElement>(".street-filter");
    expect(streets[0].classList.contains("active")).toBe(true);
    expect(streets[1].hidden).toBe(false);
    expect(streets[2].hidden).toBe(false);

    headButton(stats, "Clear").click();
    renderAll();
    restore();
  });

  test("a marker can be dragged across rows like a brush", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.clearFilters());
    renderAll();
    const first = statRow("overpair");
    const second = statRow("top pair");
    const third = statRow("middle pair");
    const mark = (row: HTMLElement) => row.querySelector<HTMLElement>(".filter-mark")!;

    point(mark(first), "pointerdown");
    point(mark(second), "pointerenter");
    point(mark(third), "pointerenter");
    window.dispatchEvent(new window.Event("pointerup"));
    renderAll();

    for (const row of [first, second, third]) {
      expect(mark(row).className).toContain("mark-blue");
    }
    // Releasing ends the sweep: a later hover changes nothing.
    point(mark(statRow("bottom pair")), "pointerenter");
    renderAll();
    expect(mark(statRow("bottom pair")).className).not.toContain("mark-blue");

    headButton(stats, "Clear").click();
    renderAll();
    restore();
  });

  test("hovering a row lights the matrix and restricts the numbers", () => {
    setRange("22+, A2s+, KJs+, AJo+");
    const row = statRow("top pair");

    point(row, "pointerenter");
    renderAll();
    expect(chrome.hovered).not.toBeNull();
    const lit = Array.from(range.element.querySelectorAll<HTMLElement>(".cell.lit"));
    expect(lit.length).toBeGreaterThan(0);
    // The glow covers the share of the cell that matches, so a cell that is
    // entirely top pair lights all the way down.
    const glows = lit.map(
      (cell) => Number(cell.style.getPropertyValue("--glow").replace("%", "")) / 100,
    );
    expect(Math.max(...glows)).toBeCloseTo(1, 2);
    expect(Math.min(...glows)).toBeGreaterThan(0);
    expect(row.querySelector(".stat-value")!.textContent).toBe("100.0%");

    point(row, "pointerleave");
    renderAll();
    expect(chrome.hovered).toBeNull();
    expect(range.element.querySelectorAll(".cell.lit")).toHaveLength(0);
  });

  test("the toggles switch units and reporting mode", () => {
    const [mode, unit] = headButtons(stats);

    expect(unit.textContent).toBe("%");
    unit.click();
    renderAll();
    expect(unit.textContent).toBe("combos");
    expect(
      stats.element.querySelector(".stat-row:not([hidden]) .stat-value")!.textContent,
    ).not.toMatch(/%/);
    unit.click();
    renderAll();

    expect(mode.textContent).toBe("absolute");
    mode.click();
    renderAll();
    expect(mode.textContent).toBe("cumulative");
    expect(state().mode).toBe("cumulative");
    mode.click();
    renderAll();
  });
});

describe("the workspace", () => {
  test("a gutter resizes the panel to its left and remembers the width", () => {
    const columns = createWorkspace([
      { key: "range", element: document.createElement("div"), label: "range", min: 340 },
      { key: "board", element: document.createElement("div"), label: "board", min: 120 },
      { key: "stats", element: document.createElement("div"), label: "stats", min: 260 },
    ]);
    document.body.append(columns.element);
    columns.render();

    const gutters = columns.element.querySelectorAll<HTMLElement>(".gutter");
    expect(gutters).toHaveLength(3);
    expect(gutters[0].dataset.column).toBe("range");

    // The matrix gets the most room; the bar chart needs the least.
    expect(chrome.columns.stats).toBeLessThan(chrome.columns.range);
    expect(chrome.columns.board).toBeLessThan(chrome.columns.stats);

    const before = chrome.columns.range;
    gutters[0].dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, clientX: 200 }));
    window.dispatchEvent(new window.MouseEvent("pointermove", { clientX: 260 }));
    window.dispatchEvent(new window.MouseEvent("pointerup", {}));
    columns.render();

    expect(chrome.columns.range).toBe(before + 60);
    expect(columns.element.style.getPropertyValue("--w-range")).toBe(`${before + 60}px`);

    // The saved width survives a reload.
    chrome.columns.range = 0;
    loadColumns();
    expect(chrome.columns.range).toBe(before + 60);
  });

  test("a panel cannot be dragged below its minimum", () => {
    const columns = createWorkspace([
      { key: "stats", element: document.createElement("div"), label: "stats", min: 260 },
    ]);
    document.body.append(columns.element);

    const gutter = columns.element.querySelector<HTMLElement>(".gutter")!;
    gutter.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, clientX: 500 }));
    window.dispatchEvent(new window.MouseEvent("pointermove", { clientX: 0 }));
    window.dispatchEvent(new window.MouseEvent("pointerup", {}));
    expect(chrome.columns.stats).toBe(260);
  });
});

describe("the output panel", () => {
  const tab = (key: string) =>
    output.element.querySelector<HTMLButtonElement>(`.output-tab[data-tab="${key}"]`)!;

  test("offers one tab per view and remembers the choice", () => {
    const tabs = Array.from(output.element.querySelectorAll<HTMLButtonElement>(".output-tab"));
    expect(tabs.map((button) => button.dataset.tab)).toEqual([
      "groups",
      "overlap",
      "eq-matrix",
      "eq-graph",
      "hotness",
    ]);
    // The pie needs nothing set up at all, so it is the one that opens.
    expect(tab("groups").classList.contains("active")).toBe(true);

    tab("eq-graph").click();
    renderAll();
    expect(chrome.output).toBe("eq-graph");
    expect(tab("eq-graph").classList.contains("active")).toBe(true);
    expect(tab("overlap").classList.contains("active")).toBe(false);
  });

  test("a cell of the overlap paints the hands that are both", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.clearFilters());
    tab("overlap").click();
    renderAll();

    const cells = Array.from(
      output.element.querySelectorAll<HTMLElement>(".overlap-cell.actionable"),
    );
    expect(cells.length).toBeGreaterThan(0);
    // A cell with nothing in it is not a button, because there is nothing to do.
    const empty = Array.from(output.element.querySelectorAll<HTMLElement>(".overlap-cell")).filter(
      (cell) => !cell.classList.contains("actionable"),
    );
    expect(empty.every((cell) => cell.textContent === "")).toBe(true);

    cells[0].click();
    renderAll();
    const painted = state()
      .groupShares.slice(1)
      .reduce((sum, share) => sum + share, 0);
    expect(painted).toBeGreaterThan(0);
    // Painting only the overlap leaves both rows disagreeing with themselves.
    expect(cells[0].title).toContain("Click to paint the overlap");

    mutate((engine) => engine.clearFilters());
    restore();
  });

  test("the overlap matrix cuts made hands against draws, or shows everything", () => {
    onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    tab("overlap").click();
    renderAll();
    expect(output.element.querySelector(".overlap-table")).not.toBeNull();

    // The default answers the question people actually ask, so no made hand
    // appears on both axes and there is no diagonal to read.
    const columns = () =>
      Array.from(output.element.querySelectorAll(".overlap-col")).map((c) => c.textContent);
    const rows = () =>
      Array.from(output.element.querySelectorAll(".overlap-row")).map((c) => c.textContent);
    expect(rows()).toContain("top pair");
    expect(columns().length).toBeGreaterThan(0);
    // Whatever the board leaves, no made hand sits on the column axis.
    for (const made of ["top pair", "overpair", "no made hand", "two pair"]) {
      expect(columns()).not.toContain(made);
    }
    expect(output.element.querySelectorAll(".overlap-cell.diagonal")).toHaveLength(0);
    // Headers carry the whole label rather than a truncation.
    expect(columns().every((label) => !label?.includes("…"))).toBe(true);

    // The full square is a click away, and there the diagonal is always 100%.
    const axes = Array.from(output.element.querySelectorAll<HTMLButtonElement>(".axes-row .chip"));
    expect(axes.map((chip) => chip.textContent)).toEqual(["Made × draws", "All"]);
    axes[1].click();
    renderAll();
    expect(columns()).toContain("top pair");
    const diagonal = output.element.querySelectorAll(".overlap-cell.diagonal");
    expect(diagonal.length).toBeGreaterThan(0);
    diagonal.forEach((cell) => expect(cell.textContent).toBe("100"));

    axes[0].click();
    renderAll();
  });

  test("the equity views need something to measure against", () => {
    tab("eq-matrix").click();
    renderAll();
    const cells = output.element.querySelectorAll(".eq-cell");
    // Two dead cards were set earlier, so there is a hand to measure.
    if (state().equity) {
      expect(cells).toHaveLength(169);
      expect(output.element.querySelectorAll(".eq-cell.on").length).toBeGreaterThan(0);
      tab("eq-graph").click();
      renderAll();
      expect(output.element.querySelector(".eq-graph")).not.toBeNull();
      expect(output.element.textContent).toMatch(/hands, strongest first/);
    } else {
      expect(output.element.textContent).toMatch(/Needs a board/);
    }
  });

  test("the equity graph draws the opponent as well as the range", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    // Fill the other seat, so there are two distributions to compare.
    seatTab(1).click();
    setRange("TT+, AQs+, AKo");
    seatTab(0).click();
    renderAll();

    tab("eq-graph").click();
    renderAll();
    const graph = output.element.querySelector(".eq-graph")!;
    expect(graph.querySelectorAll(".curve")).toHaveLength(2);
    expect(graph.querySelector(".curve.seat-curve-1")).not.toBeNull();
    // The legend names both seats and what each averages.
    const legend = output.element.querySelector(".eq-legend")!;
    expect(legend.children).toHaveLength(2);
    expect(legend.textContent).toMatch(/Range B · \d+\.\d\d%/);

    // A dealt hand has no distribution of its own, so its curve comes out flat
    // - which is the level to read the other range against, and needs no second
    // kind of line to say it.
    mutate((engine) => engine.addHand("AsKs"));
    renderAll();
    expect(output.element.querySelectorAll(".eq-graph .curve")).toHaveLength(2);
    // And it is named as the hand it is, not as the seat it sits in. Suits are
    // pips on screen, not letters.
    expect(output.element.querySelector(".eq-legend")!.textContent).toMatch(/A♠K♠ · \d+\.\d\d%/);
    mutate((engine) => engine.removeSeat(state().active));

    restore();
  });

  test("hotness ranks every card still to come and counts the good ones", () => {
    const restore = onBoard("Kh 7h 2c");
    // A dealt hand and a range: hotness is about how the cards to come treat
    // that hand against that range.
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.addHand("AsKs"));
    tab("hotness").click();
    renderAll();

    // Thirteen ranks by four suits, with the cards the board took left blank so
    // the grid still reads as a deck.
    const cards = Array.from(output.element.querySelectorAll(".hot-card"));
    expect(cards).toHaveLength(52);
    const live = cards.filter((cell) => !cell.classList.contains("gone"));
    expect(live.length).toBe(52 - 5);
    // Each card carries its own number, not just a colour.
    expect(live[0].querySelector(".hot-value")!.textContent).toMatch(/^\d+\.\d\d$/);

    // And the ranked list underneath, best first, split by what helps.
    const rows = Array.from(output.element.querySelectorAll<HTMLElement>(".hot-row"));
    expect(rows.length).toBe(live.length);
    const values = rows.map((row) => Number(row.lastElementChild!.textContent!.replace("%", "")));
    expect(values).toEqual([...values].sort((a, b) => b - a));
    expect(rows.filter((row) => row.classList.contains("helps")).length).toBeGreaterThan(0);
    expect(output.element.textContent).toMatch(/\d+ cards increase equity, \d+ decrease it/);

    restore();
  });

  test("the equity graph lists every hand under the curve", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    seatTab(1).click();
    setRange("TT+, AQs+, AKo");
    seatTab(0).click();
    mutate((engine) => engine.setDead(""));
    tab("eq-graph").click();
    renderAll();

    const rows = Array.from(output.element.querySelectorAll<HTMLElement>(".eq-row:not(.eq-head)"));
    expect(rows.length).toBeGreaterThan(20);
    expect(output.element.querySelector(".eq-head")!.textContent).toContain("Win");
    // Strongest first, and win plus tie never exceeds the whole.
    const equities = rows.map((row) => Number(row.children[2].textContent));
    expect(equities).toEqual([...equities].sort((a, b) => b - a));
    for (const row of rows.slice(0, 5)) {
      const [equity, win, tie] = [2, 3, 4].map((index) => Number(row.children[index].textContent));
      expect(win + tie).toBeLessThanOrEqual(100.001);
      expect(equity).toBeCloseTo(win + tie / 2, 2);
    }

    seatTab(1).click();
    renderAll();
    mutate((engine) => engine.clearRange());
    seatTab(0).click();
    renderAll();
    restore();
  });
});

describe("the keyboard", () => {
  let sheet: ReturnType<typeof createHotkeySheet>;
  let remove: () => void;
  let said: string[];
  let pressed: string[];

  function press(key: string, init: KeyboardEventInit = {}): void {
    window.dispatchEvent(new window.KeyboardEvent("keydown", { key, bubbles: true, ...init }));
    renderAll();
  }

  beforeAll(() => {
    said = [];
    pressed = [];
    sheet = createHotkeySheet();
    document.body.append(sheet.element);
    remove = installHotkeys({
      randomBoard: () => {
        mutate((engine) => engine.setBoard("Qs Jd 4c"));
        chrome.boardCards = ["Qs", "Jd", "4c"];
        chrome.visible = 3;
      },
      stepStreet: (delta) => {
        chrome.visible = Math.max(0, Math.min(chrome.boardCards.length, chrome.visible + delta));
        mutate((engine) => engine.setBoard(chrome.boardCards.slice(0, chrome.visible).join(" ")));
      },
      toggleSheet: sheet.toggle,
      escape: () => sheet.close(),
      say: (message) => {
        said.push(message);
      },
      actions: Object.fromEntries(
        ["save", "load", "copyRange", "copyLink", "keys"].map((name) => [
          name,
          () => pressed.push(name),
        ]),
      ),
    });
  });

  afterAll(() => {
    remove();
    sheet.element.remove();
  });

  test("every binding is listed on the sheet, once", () => {
    // The sheet is built from the same table, so this catches a key added
    // without a line saying what it does.
    const rows = Array.from(sheet.element.querySelectorAll(".sheet-row kbd")).map(
      (node) => node.textContent,
    );
    expect(rows).toEqual(BINDINGS.map((binding) => binding.keys));
    expect(new Set(rows).size).toBe(rows.length);
    for (const row of sheet.element.querySelectorAll(".sheet-row")) {
      expect(row.children[1].textContent!.length).toBeGreaterThan(4);
    }
  });

  test("the tooltips name the same keys the sheet does", () => {
    const sheetKeys = new Map(
      Array.from(sheet.element.querySelectorAll(".sheet-row")).map((row) => [
        row.children[1].textContent!,
        row.children[0].textContent!,
      ]),
    );
    // Every button that claims a key in its tooltip claims one the sheet lists.
    const claimed = Array.from(document.querySelectorAll<HTMLElement>("[title]"))
      .map((element) => element.title.match(/\(([^)]{1,9})\)$/)?.[1])
      .filter((key): key is string => key !== undefined)
      .filter((key) => key !== "Esc");
    expect(claimed.length).toBeGreaterThan(4);
    const listed = new Set([...sheetKeys.values()].flatMap((keys) => keys.split(/\s+/)));
    for (const key of claimed) {
      expect(listed.has(key), `${key} is in a tooltip but not on the sheet`).toBe(true);
    }
  });

  test("? opens the sheet and Escape closes it", () => {
    expect(sheet.element.hidden).toBe(true);
    press("?");
    expect(sheet.element.hidden).toBe(false);
    press("Escape");
    expect(sheet.element.hidden).toBe(true);
  });

  test("keys are left alone while typing in a field", () => {
    const notation = range.element.querySelector<HTMLTextAreaElement>(".notation")!;
    const before = chrome.showCombos;
    notation.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
    renderAll();
    expect(chrome.showCombos).toBe(before);
  });

  test("a focused button keeps Space and Enter", () => {
    // Space is how a button is pressed without a mouse. Taking it would leave
    // anyone navigating by keyboard unable to press anything at all.
    const target = stats.element.querySelector<HTMLButtonElement>(".street-filter")!;
    const colour = state().colour;
    target.dispatchEvent(new window.KeyboardEvent("keydown", { key: " ", bubbles: true }));
    renderAll();
    expect(state().colour).toBe(colour);

    // Away from a button it does its own job again.
    press(" ");
    expect(state().colour).not.toBe(colour);
    mutate((engine) => engine.setColour(colour));
  });

  test("Flopzilla's own keys do what they do there", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    renderAll();

    // Tab: percentages or combinations.
    const before = chrome.showCombos;
    press("Tab");
    expect(chrome.showCombos).toBe(!before);
    press("Tab");

    // Up and down: the weight the matrix paints with.
    chrome.brush = 1;
    press("ArrowDown");
    expect(chrome.brush).toBeCloseTo(0.95, 6);
    press("ArrowUp");
    expect(chrome.brush).toBeCloseTo(1, 6);
    // And it stops at the ends rather than going past them.
    press("ArrowUp");
    expect(chrome.brush).toBeCloseTo(1, 6);

    // Alt+S: clear every filter.
    statRow("top pair").click();
    renderAll();
    stats.element.querySelectorAll<HTMLButtonElement>(".street-filter")[0].click();
    renderAll();
    expect(state().filtersEnabled).toBe(true);
    press("s", { altKey: true });
    expect(state().filtersEnabled).toBe(false);

    restore();
  });

  test("the streets step with the arrows", () => {
    const restore = onBoard("Kh 7h 2c");
    chrome.boardCards = ["Kh", "7h", "2c"];
    chrome.visible = 3;
    renderAll();

    pickCard(board, "2s");
    expect(state().street).toBe("turn");
    press("ArrowLeft");
    expect(state().street).toBe("flop");
    press("ArrowRight");
    expect(state().street).toBe("turn");

    restore();
  });

  test("the digits work the street filters and Space walks the palette", () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    mutate((engine) => engine.clearFilters());
    statRow("top pair").click();
    renderAll();

    press("1");
    expect(state().filtersEnabled).toBe(true);
    press("1");
    expect(state().filtersEnabled).toBe(false);
    // A street that has not been dealt has no filter to press.
    press("3");
    expect(state().filtersEnabled).toBe(false);

    const first = state().colour;
    press(" ");
    expect(state().colour).not.toBe(first);

    mutate((engine) => engine.clearFilters());
    mutate((engine) => engine.setColour(first));
    restore();
  });

  test("the session keys press the buttons rather than repeating them", () => {
    // One path to saving, so a key and its button cannot drift apart.
    pressed.length = 0;
    press("c");
    press("l");
    press("s", { ctrlKey: true });
    press("o", { ctrlKey: true });
    expect(pressed).toEqual(["copyRange", "copyLink", "save", "load"]);
  });

  test("R deals and Backspace clears", () => {
    const restore = onBoard("Kh 7h 2c");
    press("r");
    expect(state().board).toBe("Qs Jd 4c");
    press("Backspace");
    expect(state().board).toBe("");
    restore();
  });

  test("T copies the numbers and Shift+T the combinations behind them", async () => {
    const restore = onBoard("Kh 7h 2c");
    setRange("22+, A2s+, KJs+, AJo+");
    renderAll();

    const written: string[] = [];
    Object.defineProperty(window.navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: (text: string) => {
          written.push(text);
          return Promise.resolve();
        },
      },
    });

    press("t");
    await vi.waitFor(() => expect(written).toHaveLength(1));
    expect(written[0]).toContain("Kh 7h 2c");
    expect(written[0]).toContain("top pair");
    expect(said.at(-1)).toContain("copied");

    press("T", { shiftKey: true });
    await vi.waitFor(() => expect(written).toHaveLength(2));
    // A comma-separated list of real combinations, not a notation string.
    const combos = written[1].split(",");
    expect(combos.length).toBeGreaterThan(50);
    expect(combos[0]).toMatch(/^[2-9TJQKA][cdhs][2-9TJQKA][cdhs]$/);

    restore();
  });
});

describe("the layout", () => {
  test("the columns fit the window on a first load", () => {
    // Nothing hanging off the right-hand edge: that is the one thing a first
    // load must not do, and a fixed default width cannot promise it.
    const total = () =>
      Object.values(chrome.columns).reduce((sum, width) => sum + width, 0) + 4 * 7 + 16;

    const saved = { ...chrome.columns };
    for (const width of [1920, 1600, 1440, 1280, 1152]) {
      Object.assign(chrome.columns, { range: 620, board: 260, stats: 380, output: 420 });
      Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
      fitColumns();
      expect(total(), `at ${width}px`).toBeLessThanOrEqual(width);
      // And nothing squeezed below the point where it stops being usable.
      expect(chrome.columns.board).toBeGreaterThanOrEqual(180);
      expect(chrome.columns.range).toBeGreaterThanOrEqual(360);
    }

    // Narrower than that the stylesheet stacks the columns, so their widths
    // stop meaning anything and are left alone.
    Object.assign(chrome.columns, { range: 620, board: 260, stats: 380, output: 420 });
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 900 });
    fitColumns();
    expect(chrome.columns.range).toBe(620);

    // The floors have to fit the narrowest window that still uses four columns,
    // or the promise cannot be kept at the breakpoint itself.
    Object.assign(chrome.columns, { range: 620, board: 260, stats: 380, output: 420 });
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1081 });
    fitColumns();
    expect(total()).toBeLessThanOrEqual(1081);

    // A wide window is left alone rather than stretched.
    Object.assign(chrome.columns, { range: 620, board: 260, stats: 380, output: 420 });
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 2560 });
    fitColumns();
    expect(chrome.columns.range).toBe(620);

    Object.assign(chrome.columns, saved);
  });
});

describe("the flops panel", () => {
  test("partitions every flop and folds away", () => {
    chrome.flopsOpen = true;
    renderAll();
    const rows = Array.from(flops.element.querySelectorAll(".flop-row"));
    expect(rows.length).toBeGreaterThan(10);
    const text = flops.element.textContent ?? "";
    expect(text).toContain("Unpaired");
    expect(text).toContain("Rainbow");
    expect(text).toContain("Monotone");
    // Dead cards take flops out of the count, so match the shape, not the number.
    expect(text).toMatch(/[\d,]+ flops/);

    // Folded, it is a strip: the head stays, the rows go.
    flops.element.querySelector<HTMLButtonElement>(".fold")!.click();
    renderAll();
    expect(flops.element.querySelectorAll(".flop-row")).toHaveLength(0);
    expect(flops.element.querySelector(".panel-title")!.textContent).toBe("Flops");
    expect(flops.element.classList.contains("folded")).toBe(true);

    flops.element.querySelector<HTMLButtonElement>(".fold")!.click();
    renderAll();
    expect(flops.element.querySelectorAll(".flop-row").length).toBeGreaterThan(10);
  });

  test("every row deals a flop of its own kind", () => {
    chrome.flopsOpen = true;
    renderAll();
    const row = (label: string) =>
      Array.from(flops.element.querySelectorAll<HTMLElement>(".flop-row")).find(
        (candidate) => candidate.querySelector(".flop-label")?.textContent === label,
      )!;

    row("Monotone").querySelector<HTMLButtonElement>(".deal-flop")!.click();
    renderAll();
    let cards = state().boardCards;
    expect(cards).toHaveLength(3);
    expect(new Set(cards.map((card) => card[1])).size).toBe(1);
    expect(row("Monotone").classList.contains("current")).toBe(true);

    row("Trips").querySelector<HTMLButtonElement>(".deal-flop")!.click();
    renderAll();
    cards = state().boardCards;
    expect(new Set(cards.map((card) => card[0])).size).toBe(1);
    // Dealing again moves the marker with it.
    expect(row("Monotone").classList.contains("current")).toBe(false);
    expect(row("Trips").classList.contains("current")).toBe(true);

    // Two deals of the same kind should not keep giving the same flop.
    const seen = new Set<string>();
    for (let index = 0; index < 8; index += 1) {
      row("Unpaired").querySelector<HTMLButtonElement>(".deal-flop")!.click();
      renderAll();
      seen.add(state().board);
    }
    expect(seen.size).toBeGreaterThan(1);
  });
});

describe("the preflop mode", () => {
  test("swaps filters for checkmarks and reports how often the range hits", async () => {
    // Clear the board so the panel switches modes.
    board.element.querySelectorAll<HTMLButtonElement>(".board-actions .btn")[0].click();
    chrome.boardCards = [];
    chrome.visible = 0;
    renderAll();
    expect(state().board).toBe("");
    expect(stats.element.classList.contains("preflop-mode")).toBe(true);

    // Earlier tests left two dead cards; clear them so every flop is available.
    for (const card of [...state().dead]) pickCard(strip, card);
    expect(state().dead).toHaveLength(0);

    // One combo keeps the pass over 22,100 flops quick.
    setRange("AhKh");
    const button = stats.element.querySelector<HTMLButtonElement>(".filter-toggle")!;
    expect(button.textContent).toMatch(/Calculate over all 22,100 flops/);

    const topPair = statRow("top pair");
    topPair.click();
    renderAll();
    expect(state().checkmarks[statDefIndex("top-pair")]).toBe(true);
    expect(topPair.querySelector(".filter-mark")!.textContent).toBe("✓");
    // The palette and the street buttons are postflop ideas; they step aside.
    expect(stats.element.querySelector<HTMLElement>(".palette")!.hidden).toBe(true);
    expect(stats.element.querySelector<HTMLElement>(".streets")!.hidden).toBe(true);

    button.click();
    await vi.waitFor(() => expect(chrome.preflopRunning).toBe(false), { timeout: 20000 });
    renderAll();

    expect(chrome.preflop).not.toBeNull();
    expect(chrome.preflop!.flops).toBe(22100);
    expect(chrome.preflop!.hit).toBeGreaterThan(0);
    expect(chrome.preflop!.hit).toBeLessThan(1);
    expect(stats.element.querySelector(".effective")!.textContent).toMatch(/Hits \d/);
    // Suited ace-king flops a flush about two percent of the time.
    const flush = topPairRowValue("flush");
    expect(flush).toBeGreaterThan(0);
  });
});

describe("the colour scheme", () => {
  test("cycles between following the system, light and dark", () => {
    // The label names the scheme, so the button is not a mystery word.
    expect(themeLabel()).toMatch(/Auto$/);
    expect(document.documentElement.dataset.theme).toBeUndefined();

    cycleTheme();
    expect(themeLabel()).toMatch(/Light$/);
    expect(document.documentElement.dataset.theme).toBe("light");

    cycleTheme();
    expect(themeLabel()).toMatch(/Dark$/);
    expect(document.documentElement.dataset.theme).toBe("dark");

    cycleTheme();
    expect(themeLabel()).toMatch(/Auto$/);
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });
});
