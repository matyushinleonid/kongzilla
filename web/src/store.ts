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
  /** Show combination counts instead of percentages. */
  showCombos: boolean;
  /** Which stack depth the library chips are showing. */
  libraryStack: string;
  /** The chart the range came from, while it still is that chart. */
  libraryChart: { id: string; notation: string } | null;
  /**
   * Whether a chart arrives without the hands whose EV is zero.
   *
   * A study choice rather than part of the range, so it stays here and out of
   * the link: what a shared link has to carry is the range that was loaded,
   * which it does either way.
   */
  libraryNoZeroEv: boolean;
  /** Whether the flops panel is open or folded to a strip. */
  flopsOpen: boolean;
  /** The bucket the board on the table was dealt from, while it still is it. */
  dealtBucket: { board: string; axis: string; group: string } | null;
  /** The statistic whose hands are open over the matrix for painting. */
  editing: number | null;
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

export const chrome: Chrome = {
  boardCards: [],
  visible: 0,
  brush: 1,
  window: { low: 0, high: 100 },
  hovered: null,
  showCombos: false,
  libraryStack: "",
  libraryChart: null,
  libraryNoZeroEv: false,
  flopsOpen: true,
  dealtBucket: null,
  editing: null,
  suitPeek: null,
  dealing: null,
  output: "groups",
  overlapAxes: "made-draws",
  continueShare: 0.5,
  cut: null,
  suitCell: null,
  preflop: null,
  preflopRunning: false,
  theme: null,
  // The matrix earns the most room; the statistics panel needs far less width
  // than a bar chart will happily take if you let it.
  // What the layout wants on a wide screen; fitted down to the window on a
  // first load. See ui/workspace.ts.
  columns: { range: 620, board: 260, stats: 380, output: 420 },
};

export let statDefs: StatDef[] = [];
export let blockLabels = new Map<string, string>();
export let rankings: Array<[string, string]> = [];
export let presets: Array<[key: string, label: string, isValueRange: boolean]> = [];
export let library: Library = { stacks: [], rows: [], entries: [] };
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
  const [stacks, rows, entries] = JSON.parse(Engine.library());
  library = { stacks, rows, entries };
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
      // A saved range is not necessarily a band of the ranking, but leaving the
      // handles spanning everything next to a narrow range reads as a bug. Sit
      // them on the range's realised size instead.
      chrome.window = { low: 0, high: view.players[view.active].percent };
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
    engine.loadLibrary("mtt-100bb-open-btn", false);
    chrome.libraryStack = "100bb";
    chrome.libraryChart = {
      id: "mtt-100bb-open-btn",
      notation: (JSON.parse(engine.view()) as View).players[0].notation,
    };
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
  if (chrome.hovered !== null) {
    hoverPanel = JSON.parse(engine.breakdownWithin(chrome.hovered));
  } else {
    hoverPanel = null;
  }
  // The second column is read under the same restriction as the first, so both
  // are always answering the same question.
  comparePanel = JSON.parse(engine.compareBreakdown(chrome.hovered ?? undefined));
  listeners.forEach((listener) => listener());
}

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

/** The hands in one matrix cell, with their colours. */
export function comboColours(cell: number): Array<[number, string, string]> {
  return JSON.parse(engine.comboColours(cell));
}

/** Per-cell matrix weights for the hovered statistic. */
export function highlight(stat: number): Float32Array {
  return engine.highlight(stat);
}

/** Paints the strongest share of the range by equity, and remembers the slice. */
export function setCut(share: number): void {
  mutate((engine) => {
    chrome.cut = JSON.parse(engine.setContinueByEquity(share)) as Cut | null;
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

/** How each remaining card changes the hand's equity. */
export function hotness(): HotCard[] | null {
  return JSON.parse(engine.hotness());
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
      chrome.preflop = JSON.parse(engine.preflop());
    } finally {
      chrome.preflopRunning = false;
      repaint();
    }
  }, 0);
}

/**
 * How many hand-flop pairs a pass would have to classify.
 *
 * The whole cost of the thing, and it is known before doing any of it: every
 * hand in the range against every flop being looked at.
 */
export function preflopWork(): number {
  const view = state();
  return view.liveCombos * view.filteredFlops;
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

function refresh(): void {
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
