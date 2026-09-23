/**
 * Application state.
 *
 * The engine owns the analysis; this module owns the handful of things that are
 * purely presentational - which cards have been entered but are hidden by the
 * street arrows, which statistic the pointer is over, whether the panel shows
 * percentages or combos - and re-renders every panel whenever anything changes.
 */

import init, { Engine } from "../wasm/kongzilla_wasm.js";
import type {
  Breakdown,
  CellCombo,
  ClassCombo,
  Cut,
  EquityBucket,
  FlopBreakdown,
  HotCard,
  Library,
  OverlapMatrix,
  PreflopBreakdown,
  StatDef,
  View,
} from "./types";
import { readHash, writeHash } from "./share";

export interface Chrome {
  /** Cards entered for the board, including any hidden by the street arrows. */
  boardCards: string[];
  /** How many of them the engine is currently analysing. */
  visible: number;
  /** Weight painted by a matrix drag, in 0..1. Starts at full weight. */
  brush: number;
  /** Where the two handles of the slider under the matrix sit, in percent. */
  window: { low: number; high: number };
  /** Statistic the pointer is over, or null. */
  hovered: number | null;
  /**
   * Band of equity the pointer is over, or null.
   *
   * Apart from `hovered` rather than folded into it: a band is not a statistic
   * and has no place in the registry, and giving it a made-up index there was
   * how hovering one came to light nothing at all.
   */
  hoveredBand: string | null;
  /** Band of equity opened for painting hand by hand, or null. */
  editingBand: string | null;
  /** Show combination counts instead of percentages. */
  showCombos: boolean;
  /** Which stack depth the library chips are showing. */
  libraryStack: string;
  /** The chart the range came from, while it still is that chart. */
  /**
   * Whether a chart arrives without the hands whose EV is zero.
   *
   * A study choice rather than part of the range, so it stays here and out of
   * the link: what a shared link has to carry is the range that was loaded,
   * which it does either way.
   */
  libraryNoZeroEv: boolean;
  /** Which open the three-bet row is showing, by seat key. */
  libraryOpener: string;
  /**
   * Which of what a solver does in a spot a chart chip puts on the table, by
   * action key. Absent or true means take it; only an explicit false leaves it
   * out, so an action nobody has said anything about is in.
   */
  actions: Record<string, boolean>;
  /** Whether the flops panel is open or folded to a strip. */
  flopsOpen: boolean;
  /** The bucket the board on the table was dealt from, while it still is it. */
  dealtBucket: { board: string; axis: string; group: string } | null;
  /** The statistic whose hands are open over the matrix for painting. */
  editing: number | null;
  /**
   * The hand the reader is pointing at, wherever they are pointing from.
   *
   * One notion shared by every panel rather than one per panel: the matrix, the
   * suit breakdown, the equity matrix and the equity graph all speak about
   * hands, and pointing at a hand in any of them is the same question - what is
   * this hand, here, on this board. Whoever the pointer is over sets it, and
   * everybody else lights the part of themselves that is about it.
   *
   * The cell and the combination are kept apart because they are different
   * questions: a cell is up to sixteen hands and answers in shares, and one
   * combination answers yes or no.
   */
  peekClass: number | null;
  peekCombo: number | null;
  /** The cell whose suit breakdown is showing, and whether it is pinned open. */
  suitPeek: number | null;
  /**
   * The cards picked so far while dealing a hand, or `null` when not dealing.
   *
   * Dealing a hand borrows the dead-card grid, so the mode has to be somewhere
   * the grid can see: without it a click there means "take this card out of the
   * deck", which is what it has always meant and still does.
   */
  dealing: string[] | null;
  /** Which tab the output panel is showing. */
  output: "groups" | "overlap" | "eq-matrix" | "eq-graph" | "hotness";
  /** Whether the overlap matrix shows made hands against draws, or everything. */
  overlapAxes: "made-draws" | "all";
  /** The share of the range the filter slider is asking to continue with. */
  continueShare: number;
  /** The slice the slider last took, or null when nothing is cut. */
  cut: Cut | null;
  /** The matrix cell whose individual combos are open for editing, if any. */
  suitCell: number | null;
  /** The last preflop pass, which is computed on request rather than live. */
  preflop: PreflopBreakdown | null;
  /**
   * Which part of a row a press paints, by equity, strongest first.
   *
   * The whole of it is `0..100`. Anything narrower takes a slice: the best
   * fifth of the trash is `0..20`, the worst quarter of top pair is `75..100`.
   * A rung says what a hand made and two hands on one rung can be a long way
   * apart, so this is how a reader reaches the part they meant.
   */
  rowBand: { low: number; high: number };
  /** What is in the middle, their bet included, and that bet. */
  pot: number;
  bet: number;
  /** Which slice of the range the equity slider is painting, as shares. */
  slice: { from: number; to: number };
  /** Whether a preflop pass is running. */
  preflopRunning: boolean;
  /** Colour scheme: `null` follows the system. */
  theme: "light" | "dark" | null;
  /** Width of each workspace column, in pixels. */
  columns: { range: number; board: number; stats: number; output: number };
}

let engine: Engine;
let view: View;
let hoverPanel: Breakdown | null = null;
let comparePanel: Breakdown | null = null;

/**
 * What the chrome is on a fresh visit.
 *
 * Kept as a value rather than written straight into `chrome`, because starting
 * over is a thing that happens twice: once when the page loads, and once per
 * end-to-end test, which opens a new session without reloading the page. The
 * tests used to keep their own list of what to put back, and a new piece of
 * chrome that nobody added to it leaked from one test into the next.
 */
const FRESH: Chrome = {
  boardCards: [],
  visible: 0,
  brush: 1,
  window: { low: 0, high: 100 },
  hovered: null,
  hoveredBand: null,
  editingBand: null,
  showCombos: false,
  libraryStack: "",
  libraryNoZeroEv: false,
  libraryOpener: "",
  actions: {},
  flopsOpen: true,
  dealtBucket: null,
  editing: null,
  peekClass: null,
  peekCombo: null,
  suitPeek: null,
  dealing: null,
  output: "groups",
  overlapAxes: "made-draws",
  continueShare: 0.5,
  cut: null,
  suitCell: null,
  preflop: null,
  rowBand: { low: 0, high: 100 },
  // Nothing in the boxes until a reader puts a spot there: a calculator that
  // opens with a hundred and fifty in the pot is answering a question nobody
  // asked, and the answer looks like it is about the range on screen.
  pot: 0,
  bet: 0,
  slice: { from: 0, to: 1 },
  preflopRunning: false,
  theme: null,
  // The matrix earns the most room; the statistics panel needs far less width
  // than a bar chart will happily take if you let it.
  // What the layout wants on a wide screen; fitted down to the window on a
  // first load. See ui/workspace.ts.
  columns: { range: 620, board: 260, stats: 380, output: 420 },
};

export const chrome: Chrome = structuredClone(FRESH);

/**
 * Puts the chrome back to how a fresh visit finds it.
 *
 * Not the theme, which is the reader's and outlives any one session, and not
 * the column widths, which are read from storage by the workspace.
 */
export function resetChrome(): void {
  const { theme, columns } = chrome;
  Object.assign(chrome, structuredClone(FRESH), { theme, columns });
}

export let statDefs: StatDef[] = [];
export let blockLabels = new Map<string, string>();
export let rankings: Array<[string, string]> = [];
export let presets: Array<[key: string, label: string, isValueRange: boolean]> = [];
export let library: Library = { stacks: [], rows: [], entries: [], seats: [] };
export let classLabels: string[] = [];
let paletteKeys: string[] = [];

const listeners = new Set<() => void>();

/**
 * Boots the WebAssembly engine and loads any state carried in the URL.
 *
 * `wasmSource` exists so the test suite can hand the module over directly; in the
 * browser the default fetch beside the bundle is what runs.
 */
export async function boot(wasmSource?: BufferSource): Promise<void> {
  await init(wasmSource ? { module_or_path: wasmSource } : undefined);
  engine = new Engine();
  statDefs = JSON.parse(Engine.statDefinitions());
  blockLabels = new Map(JSON.parse(Engine.blockLabels()) as Array<[string, string]>);
  rankings = JSON.parse(Engine.rankings());
  presets = JSON.parse(Engine.presets());
  const [stacks, rows, entries, seats] = JSON.parse(Engine.library());
  library = { stacks, rows, entries, seats };
  chrome.libraryStack = stacks[0]?.[0] ?? "";
  classLabels = JSON.parse(Engine.classLabels());
  paletteKeys = JSON.parse(Engine.palette());

  const saved = readHash();
  let restored = false;
  if (saved) {
    try {
      engine.restore(saved);
      const view: View = JSON.parse(engine.view());
      chrome.boardCards = [...view.boardCards];
      chrome.visible = view.boardCards.length;
      restored = true;
    } catch {
      // A malformed link should not stop the app from starting.
    }
  }
  if (!restored) {
    // A button opening range is the most-looked-at spot there is, so the app
    // opens on one rather than on an empty matrix.
    // Whole, because the app opens on the solution rather than on a reading of
    // it; the switch in the panel is the reader's to press.
    engine.loadLibrary("mtt-100bb-open-btn", "call,raise,allin", false);
    chrome.libraryStack = "100bb";
  }
  refresh();
}

/** Subscribes to state changes. Returns an unsubscribe function. */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The latest view model. */
export function state(): View {
  return view;
}

/** The statistics panel restricted to the hovered statistic, if any. */
export function hoverBreakdown(): Breakdown | null {
  return hoverPanel;
}

/** Runs a mutation against the engine and re-renders. */
export function mutate(change: (engine: Engine) => void): void {
  try {
    change(engine);
  } catch (error) {
    report(error);
    return;
  }
  refresh();
}

/** Re-renders without touching the engine - for presentation-only changes. */
export function repaint(): void {
  // Two passes over the range and two JSON documents, and a pointer moving
  // across the matrix asks for a repaint on every cell it crosses. Neither of
  // these depends on where the pointer is except through the statistic it is
  // over, so they are worked out again only when that or the session moves -
  // which is most pointer moves not paying for them at all.
  const asked = `${chrome.hovered ?? "none"}/${changes}/${state().versusSeat ?? "all"}`;
  if (asked !== panelsFor) {
    panelsFor = asked;
    hoverPanel =
      chrome.hovered === null ? null : JSON.parse(engine.breakdownWithin(chrome.hovered));
    // The second column is read under the same restriction as the first, so
    // both are always answering the same question.
    comparePanel = JSON.parse(engine.compareBreakdown(chrome.hovered ?? undefined));
  }
  listeners.forEach((listener) => listener());
}

/** What the two side panels were last worked out for. */
let panelsFor = "";

/** The seat the statistics panel is comparing against, as a second column. */
export function compareBreakdown(): Breakdown | null {
  return comparePanel;
}

/** Chooses which seat to compare against, or clears the comparison. */
export function setCompareSeat(seat: number | null): void {
  mutate((instance) => instance.setCompareSeat(seat ?? undefined));
}

/** Chooses which seat the per-hand equity views measure against. */
export function setVersusSeat(seat: number | null): void {
  mutate((instance) => instance.setVersusSeat(seat ?? undefined));
}

/**
 * Moves the opponent on: each other seat in turn, then the whole field again.
 *
 * One setting with more than one button on it - the output panel's and the
 * calculator's - so the rounding lives here rather than once per button.
 */
export function cycleVersusSeat(): void {
  const view = state();
  const others = view.players.map((_, index) => index).filter((index) => index !== view.active);
  if (others.length === 0) return;
  const at = view.versusSeat === null ? -1 : others.indexOf(view.versusSeat);
  setVersusSeat(at + 1 < others.length ? others[at + 1] : null);
}

/** The palette, in the order it is offered. */
export function palette(): string[] {
  return paletteKeys;
}

/**
 * Which `--group-N` token a colour wears. Unpainted, and anything that is not a
 * colour at all, is nought.
 *
 * The palette lives in the engine, which is the one place that decides what the
 * colours are and what order they come in. This turns a name into the slot it
 * occupies so nothing else has to keep a second copy of that list.
 */
export function colourSlot(colour: string): number {
  const index = paletteKeys.indexOf(colour);
  return index < 0 ? 0 : index + 1;
}

/**
 * Binds an element to a paint colour.
 *
 * Everything that wears one - a swatch, a row's marker, a chip, a cell in the
 * suit breakdown, a slice of the pie - says so through `--mark`, and one rule
 * per shape reads it. Without this the stylesheet keeps its own table of which
 * name is which colour, in as many copies as there are shapes, and an eighth
 * colour means finding all of them.
 */
export function markColour(element: HTMLElement, colour: string): void {
  element.dataset.colour = colour;
  element.style.setProperty("--mark", `var(--group-${colourSlot(colour)})`);
}

/** The statistics panel as text, restricted to one statistic when given. */
export function statisticsText(within?: number): string {
  return engine.statisticsText(within);
}

/** Every combination the statistics panel is speaking about. */
export function statisticsCombos(within?: number): string {
  return engine.statisticsCombos(within);
}

/** Everything one matrix cell holds, combination by combination. */
export function cellCombos(cell: number): CellCombo[] {
  return JSON.parse(engine.cellCombos(cell, chrome.hovered ?? undefined));
}

/** The hands carrying one statistic, with their colours. */
export function statCombos(stat: number): Array<[number, string, string]> {
  return JSON.parse(engine.statCombos(stat));
}

/** The same, for the hands in one band of equity rather than on one rung. */
export function bandCombos(band: string): Array<[number, string, string]> {
  return JSON.parse(engine.bandCombos(band));
}

/**
 * How much of each matrix cell is worth this much against the other range.
 *
 * The same reading [`highlight`] takes for a statistic, for a band of equity:
 * hovering "best hands" has to light the hands it means, and those are not a
 * category anybody can name - they are whatever is worth 75% today.
 */
export function bandShares(band: string): Float32Array {
  return engine.bandShares(band);
}

/**
 * Which statistics one hand is about, as ones and noughts.
 *
 * The same reading [`peekStats`] takes, for a hand the pointer is nowhere near:
 * the equity table asks it of every row.
 */
export function comboStats(combo: number): Float32Array {
  return engine.comboStats(combo);
}

/** What colour one hand was painted, as a palette key or "none". */
export function comboColour(combo: number): string {
  return engine.comboColour(combo);
}

/**
 * How strong every hand is on this board, by combo index.
 *
 * Empty before the flop, where there is no board to be strong on. Used to
 * settle ties: two hands worth the same equity against one opponent are still
 * not the same hand, and the better one belongs first.
 */
export function rankByCombo(): Uint32Array {
  return engine.rankByCombo();
}

/**
 * The palette slot of every hand, by combo index.
 *
 * The pie reads all of them at once. Asking one at a time crosses the wasm
 * boundary a thousand times for one drawing.
 */
export function colourByCombo(): Uint8Array {
  return engine.colourByCombo();
}

/**
 * Which hands are behind each tier of the last pass over the flops.
 *
 * `combo * 4 + tier`, strongest tier first, and empty when no pass is standing.
 */
export function preflopTiers(): Float32Array {
  return engine.preflopTiers();
}

/** The hands in one matrix cell, with their colours. */
export function comboColours(cell: number): Array<[number, string, string]> {
  return JSON.parse(engine.comboColours(cell));
}

/**
 * Where the top-of-the-range slider has anywhere to stop, as shares.
 *
 * Empty before a flop, and empty when there is nothing to measure against.
 */
export function equitySteps(): Float32Array {
  return engine.equitySteps();
}

/** Per-cell matrix weights for the hovered statistic. */
export function highlight(stat: number): Float32Array {
  return engine.highlight(stat);
}

/** Paints a slice of the range by equity, and remembers it. */
export function setCut(from: number, to: number): void {
  mutate((engine) => {
    chrome.cut = JSON.parse(engine.setContinueBetween(from, to)) as Cut | null;
  });
}

/** Undoes the cut, putting back the painting it was laid over. */
export function clearCut(): void {
  chrome.cut = null;
  mutate((engine) => engine.clearCut());
}

/** How much of each matrix cell the cut keeps, for painting the selection. */
export function cutShares(): Float32Array | undefined {
  return engine.cutShares();
}

/** How often each statistic comes with each other one. */
export function overlap(): OverlapMatrix {
  return JSON.parse(engine.overlap());
}

/** Deals a random flop from one bucket, and remembers which bucket it was. */
export function dealFlop(): void {
  mutate((engine) => {
    if (!engine.dealFlop()) return;
    const view: View = JSON.parse(engine.view());
    chrome.boardCards = [...view.boardCards];
    chrome.visible = view.boardCards.length;
    // It came from the whole selection rather than from one row, so no row
    // gets to claim it.
    chrome.dealtBucket = null;
  });
}

export function dealFlopFrom(axis: string, group: string): void {
  mutate((engine) => {
    if (!engine.dealFlopFrom(axis, group)) return;
    const view: View = JSON.parse(engine.view());
    chrome.boardCards = [...view.boardCards];
    chrome.visible = view.boardCards.length;
    chrome.dealtBucket = { board: view.board, axis, group };
  });
}

/** How often each kind of flop comes. */
export function flopBreakdown(): FlopBreakdown {
  return JSON.parse(engine.flopBreakdown());
}

/**
 * The range split by how much equity each hand has.
 *
 * Empty where there is nothing to measure against. Worked out from the same
 * per-hand equity the matrix and the graph read, so asking for it costs
 * nothing once one of those has.
 */
export function equityBuckets(): EquityBucket[] {
  return JSON.parse(engine.equityBuckets());
}

/**
 * Paints part of a category, taken by equity.
 *
 * The whole of it is painted as a category and keeps following the board;
 * anything narrower is a fact about this board and is painted hand by hand.
 */
export function paintStatPart(stat: number, colour: string): boolean {
  const { low, high } = chrome.rowBand;
  let painted = false;
  mutate((engine) => {
    painted = engine.paintStatPart(stat, low, high, colour);
  });
  return painted;
}

/** The same, for one band of the equity range. */
export function paintEquityBand(band: string, colour: string): boolean {
  const { low, high } = chrome.rowBand;
  let painted = false;
  mutate((engine) => {
    painted = engine.paintEquityBand(band, low, high, colour);
  });
  return painted;
}

/** How each remaining card changes the hand's equity. */
export function hotness(): HotCard[] | null {
  return JSON.parse(engine.hotness());
}

/**
 * Where the range slider's handles have somewhere to stop.
 *
 * The edges of the matrix cells, in the order the slider walks them. Between
 * two of them there is nothing to choose - the same hands are selected either
 * way - so this is what the handles snap to.
 */
export function sliderStops(): Float32Array {
  return engine.sliderStops();
}

/** Equity and weight per combo index, for the equity matrix and graph. */
export function equityByCombo(): {
  equity: Float32Array;
  weight: Float32Array;
  win: Float32Array;
  tie: Float32Array;
} | null {
  const equity = engine.equityByCombo();
  if (equity.length === 0) return null;
  return {
    equity,
    weight: engine.comboWeights(),
    win: engine.comboWins(),
    tie: engine.comboTies(),
  };
}

/** The same, for the seat the active range is measured against. */
export function opponentEquityByCombo(): {
  equity: Float32Array;
  weight: Float32Array;
} | null {
  const equity = engine.opponentEquityByCombo();
  if (equity.length === 0) return null;
  return { equity, weight: engine.opponentComboWeights() };
}

/** What one combo is on the current board, as statistic labels. */
export function describeCombo(combo: number): string[] {
  return JSON.parse(engine.describeCombo(combo));
}

/**
 * Which statistics the hand under the pointer is about, as a share of it.
 *
 * `null` when nothing is being pointed at, or when the board and the dead cards
 * have taken every combination of it away - which is not the same as a hand
 * that makes nothing, and says so by lighting nothing at all.
 */
export function peekStats(): Float32Array | null {
  if (chrome.peekCombo !== null) {
    const shares = engine.comboStats(chrome.peekCombo);
    return shares.length > 0 ? shares : null;
  }
  if (chrome.peekClass !== null) {
    const shares = engine.classStats(chrome.peekClass);
    return shares.length > 0 ? shares : null;
  }
  return null;
}

/** Points at a hand, and tells everyone - unless it is the one already pointed at. */
export function peekAt(klass: number | null, combo: number | null = null): void {
  if (chrome.peekClass === klass && chrome.peekCombo === combo) return;
  chrome.peekClass = klass;
  chrome.peekCombo = combo;
  repaint();
}

/** The individual combos of one matrix cell. */
export function classCombos(index: number): ClassCombo[] {
  return JSON.parse(engine.classCombos(index));
}

/**
 * Runs the preflop pass over all 22,100 flops.
 *
 * A wide range is tens of millions of classifications, so this is deliberately a
 * button rather than something that happens on every keystroke - the same choice
 * Flopzilla makes. The repaint before the work lets the interface say so.
 */
export function runPreflop(): void {
  chrome.preflopRunning = true;
  repaint();
  window.setTimeout(() => {
    try {
      // Every seat that is waiting, not just the one being looked at. A reader
      // comparing two ranges wants both answered, and a pass already standing
      // costs nothing to ask for again - so this is only ever as slow as what
      // was actually outstanding.
      engine.preflopAll();
      // And the seat being looked at, whether or not it was one of them. A
      // seat with nothing in it has no pass worth running, so `preflopAll`
      // leaves it alone - and this used to leave the panel with no pass to
      // show, which is the state that asks for one. It asked, got nothing
      // again, and asked again: the button and the note blinked every second
      // and a half for as long as the reader sat there.
      chrome.preflop = JSON.parse(engine.preflop());
    } finally {
      chrome.preflopRunning = false;
      repaint();
    }
  }, 0);
}

/** How many seats are waiting on a pass over the flops. */
export function preflopOutstanding(): number {
  return engine.preflopOutstanding();
}

/** Whether the pass over the flops has left per-hand equity standing. */
export function preflopEquityReady(): boolean {
  return engine.preflopEquityReady();
}

/**
 * Whether a pass would work out per-hand equity that is not worked out yet.
 *
 * The breakdown and the equity ride along together but go stale apart: a pass
 * is about one seat's range, and the equity is about that range against
 * another one. Fill the second seat after running a pass and the breakdown is
 * still good while the equity was never worked out at all - so there is
 * something to ask for, even though there is a pass.
 */
export function preflopEquityMissing(): boolean {
  return engine.preflopEquityWork() > 0 && !engine.preflopEquityReady();
}

/**
 * How many hand-flop pairs a pass would have to classify.
 *
 * The whole cost of the thing, and it is known before doing any of it: every
 * hand in the range against every flop being looked at.
 */
export function preflopWork(): number {
  const view = state();
  // The per-hand equity rides along with the pass when there are two ranges and
  // no board, and it is not free - so the panel that decides whether to run a
  // pass unasked counts it too. A hand evaluated and a hand classified cost
  // about the same, measured on the same machine, so they simply add up.
  return view.liveCombos * view.filteredFlops + engine.preflopEquityWork();
}

/**
 * How much work may happen without being asked for.
 *
 * Measured rather than guessed: the engine classifies about 7.5 million pairs a
 * second in WebAssembly, so this is roughly a fifth of a second - long enough
 * to cover a narrowed set of flops or a hand or two over all of them, short
 * enough not to be felt after the range stops moving. Above it the reader gets
 * a button, because three and a half seconds of a frozen page is not something
 * to do to somebody who was only typing.
 */
const AUTOMATIC_WORK = 1_500_000;

/** Whether the pass is cheap enough to just run. */
export function preflopIsCheap(): boolean {
  return preflopWork() <= AUTOMATIC_WORK;
}

/** Waiting for the range to stop moving before running a cheap pass. */
let settling = 0;

/**
 * Runs the pass once the range has stopped changing, if it is cheap enough.
 *
 * Painting a range is a drag across cells, and every cell is a change: without
 * the wait this would run a pass per cell and the drag would stutter. With it,
 * the numbers appear a moment after the hand leaves the mouse.
 */
export function runPreflopIfCheap(): void {
  if (chrome.preflop || chrome.preflopRunning || !preflopIsCheap()) return;
  if (settling) window.clearTimeout(settling);
  settling = window.setTimeout(() => {
    settling = 0;
    if (chrome.preflop || chrome.preflopRunning || !preflopIsCheap()) return;
    runPreflop();
  }, 250);
}

/** Adds or removes one group of flops from what a pass looks at. */
export function toggleFlopGroup(axis: string, group: string): void {
  mutate((e) => e.toggleFlopGroup(axis, group));
}

/** Puts every flop back into what a pass looks at. */
export function clearFlopFilter(): void {
  mutate((e) => e.clearFlopFilter());
}

/** The session as JSON, for saving. */
export function snapshot(): string {
  return engine.snapshot();
}

/** Loads a session from JSON. */
export function restore(json: string): void {
  mutate((e) => {
    e.restore(json);
    const restored: View = JSON.parse(e.view());
    chrome.boardCards = [...restored.boardCards];
    chrome.visible = restored.boardCards.length;
  });
}

/**
 * How many times the session has actually changed.
 *
 * Repaints happen for two reasons: something changed, or the pointer moved.
 * The second kind is far more frequent and rebuilds nothing new, so a panel
 * that is expensive to build can compare this with what it last built from and
 * skip the work. It moves on every refresh and never otherwise.
 */
let changes = 0;

export function revision(): number {
  return changes;
}

function refresh(): void {
  changes += 1;
  view = JSON.parse(engine.view());
  // A pass over the flops survives anything that does not change what it was a
  // pass over. Ticking a statistic is the thing a reader does most often right
  // after running one, and throwing the answer away for it - with nothing on
  // screen to say so - is how the tick came to look as though it did nothing.
  //
  // The engine holds one pass per seat, so the engine decides which applies:
  // asking it after every change puts back whatever this seat already knows,
  // which is how moving to the other range to compare and moving back again
  // stopped costing a second and a half.
  const cached = engine.preflopCached();
  chrome.preflop = cached === undefined || cached === null ? null : JSON.parse(cached);
  writeHash(engine.snapshot());
  repaint();
}

let errorSink: (message: string) => void = () => {};

/** Registers where parse errors are shown. */
export function onError(sink: (message: string) => void): void {
  errorSink = sink;
}

function report(error: unknown): void {
  errorSink(error instanceof Error ? error.message : String(error));
}
