/** Shapes crossing the WebAssembly boundary. These mirror the Rust serde output. */

export type Block = "made" | "draw" | "combination";
export type BreakdownMode = "absolute" | "cumulative";

export interface StatDef {
  index: number;
  key: string;
  label: string;
  block: Block;
  optional: boolean;
}

export interface StatRow {
  key: string;
  label: string;
  block: Block;
  index: number;
  combos: number;
  fraction: number;
}

export interface Breakdown {
  totalCombos: number;
  rows: StatRow[];
  mode: BreakdownMode;
}

export interface Equity {
  win: number;
  tie: number;
  equity: number;
}

export interface EquityReport {
  players: Equity[];
  exact: boolean;
  trials: number;
}

export interface PlayerView {
  name: string;
  /** The one hand this seat holds, if it holds exactly one. */
  hand: string | null;
  notation: string;
  combos: number;
  percent: number;
  /** Mean weight per matrix cell, for the seat's thumbnail. */
  classWeights: number[];
  /** Where the range slider's two handles sit, as percentages of the deck. */
  sliderLow: number;
  sliderHigh: number;
  /** The library chart this seat was loaded from, if it was. */
  chart: string | null;
  /** Whether the matrix still holds that chart rather than an edit of it. */
  chartEdited: boolean;
}

export interface ClassifyOptions {
  oneCardBackdoorFlushdraw: boolean;
}

export interface View {
  board: string;
  boardCards: string[];
  street: string;
  dead: string[];
  /** The one hand the active seat holds, if it holds exactly one. */
  hand: string | null;
  active: number;
  players: PlayerView[];
  classWeights: number[];
  classCombos: number[];
  breakdown: Breakdown;
  /** One marker per statistic: a colour key, "mixed" for a gear, or "empty". */
  marks: string[];
  /** Per matrix cell, the share of the cell in each colour, unpainted first. */
  classColours: number[][];
  /** The weight of the range in each colour, unpainted first. */
  groupShares: number[];
  /** How many colours are in use; one needs no key to tell it apart. */
  coloursUsed: number;
  /** The palette colour currently held. */
  colour: string;
  /** Per matrix cell, how much of it survives the applied street filters. */
  classPassing: number[];
  /** The suits a suited cell holds, when it holds only some of them. */
  classSuits: string[];
  /** Which seats the equity report is about, in the order it lists them. */
  equitySeats: number[];
  /** How many seats the table will hold. */
  maxSeats: number;
  /** Which seat the statistics panel compares against, if any. */
  compareSeat: number | null;
  /** Which seat the per-hand equity views measure against, or null for the field. */
  versusSeat: number | null;
  /** Whether the selected seat can be changed. A hand has been dealt, so it cannot. */
  editable: boolean;
  /** The cards the seats have been dealt, which nobody else can hold. */
  dealt: string[];
  /** What share of each colour the street filters let through, unpainted first. */
  colourShares: number[];
  /** Which streets have had their filter pressed. */
  streetsOn: boolean[];
  /** Combos left after the filters up to and including each street. */
  streetCounts: number[];
  /** What a street filter would leave if it were pressed now. */
  wouldPass: number;
  /** How many street buttons the board has room for. */
  streetsDealt: number;
  filtersEnabled: boolean;
  passFraction: number;
  checkmarks: boolean[];
  /** The groups of flops a pass is narrowed to, as `axis/group` keys. */
  flopGroups: string[];
  /** How many flops a pass would look at, after the dead cards and the groups. */
  filteredFlops: number;
  /** Combos of the active range left after the board and the dead cards. */
  liveCombos: number;
  mode: BreakdownMode;
  ranking: string;
  options: ClassifyOptions;
  equity: EquityReport | null;
  effectiveNotation: string;
  effectiveCombos: number;
}

/** One chart in the preflop library. */
export interface LibraryEntry {
  id: string;
  stack: string;
  /** What it is a strategy for: `open`, `rfi`, `defend` or `isolate`. */
  spot: string;
  /** Whose strategy it is. */
  seat: string;
  seatLabel: string;
  /** The seat being answered, where there is one: the opener, or the raiser. */
  versus: string | null;
  versusLabel: string | null;
  label: string;
  description: string;
  percent: number;
  /**
   * What the solver actually does in this spot, as key, name and how much of
   * the deck it does it with. The panel puts a switch against each.
   */
  actions: Array<[string, string, number]>;
  sizeBb: number;
  /** Whether this chart plays anything at no gain. */
  hasZeroEv: boolean;
}

/** The library, as the engine ships it. */
export interface Library {
  /** Each group of charts: chip label, what it is, and which game. */
  stacks: Array<[string, string, string]>;
  rows: Array<[string, string]>;
  entries: LibraryEntry[];
  /** Every seat, in the order it acts. */
  seats: Array<[string, string]>;
}

/** How often each statistic comes with each other one. */
export interface OverlapMatrix {
  stats: number[];
  labels: string[];
  totals: number[];
  rows: number[][];
}

/** A range classified against every one of the 22,100 flops. */
export interface PreflopBreakdown {
  flops: number;
  total: number;
  rows: StatRow[];
  hit: number;
}

/** One bucket of the flop breakdown. */
export interface FlopGroup {
  key: string;
  label: string;
  flops: number;
  fraction: number;
  /** How many of those also satisfy the ticks on the *other* axes. */
  kept: number;
  keptFraction: number;
}

/** One way of cutting up the flops. */
export interface FlopAxis {
  key: string;
  label: string;
  groups: FlopGroup[];
}

/** Every axis of the flop breakdown. */
export interface FlopBreakdown {
  total: number;
  /** How many flops satisfy every tick, which is what a pass runs over. */
  kept: number;
  axes: FlopAxis[];
}

/** How one card changes a hand's equity. */
export interface HotCard {
  card: string;
  equity: number;
}

/** One combo of a matrix cell: index, name, weight. */
export type ClassCombo = [number, string, number];

/** The strongest slice of a range, taken by equity on one board. */
export interface Cut {
  /** The share that was asked for. */
  share: number;
  /** The share the slice actually holds. */
  covered: number;
  /** The equity of the weakest combo that continues. */
  threshold: number;
  /** The board the slice was taken on. */
  board: string;
  /** How many combos continue. */
  combos: number;
}

/** One combination of one matrix cell, as the suit popup draws it. */
export interface CellCombo {
  index: number;
  name: string;
  /** How much of it the range holds, `0..=1`. */
  weight: number;
  /** The colour it carries, or `"none"`. */
  colour: string;
  /** How much of it survives the street filters. */
  passing: number;
  /** Whether a hovered statistic is about this hand. */
  matches: boolean;
  /** Whether the board or the dead cards have taken one of its cards. */
  dealt: boolean;
}
