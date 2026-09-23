/**
 * The output panel.
 *
 * Flopzilla keeps a strip of thumbnails - groups, equity graph, equity matrix,
 * hotness - that swap the big panel on the right. Same idea: one column, one tab
 * strip, and each view is a pure read of the engine, computed only while it is on
 * screen.
 */

import {
  chrome,
  classLabels,
  cycleVersusSeat,
  comboColour,
  comboStats,
  colourByCombo,
  colourSlot,
  describeCombo,
  equityByCombo,
  hotness,
  markColour,
  mutate,
  opponentEquityByCombo,
  peekAt,
  overlap,
  palette,
  preflopEquityMissing,
  preflopEquityReady,
  preflopTiers,
  rankByCombo,
  revision,
  repaint,
  state,
  statDefs,
} from "../store";
import {
  RANKS,
  SUITS,
  SUIT_GLYPH,
  comboClass as classOfCombo,
  comboName,
  pips,
  seatName,
  seatTag,
} from "./cards";
import type { PreflopBreakdown } from "../types";

type Tab = typeof chrome.output;

/**
 * The views, and which of them are measured against another range.
 *
 * Four of the five ask the question the opponent control answers. The pie does
 * because its wedges are drawn hand by hand, strongest first, and "strongest"
 * is strongest against somebody. The overlap is about statistics alone, and a
 * control that changed nothing on it would be a control that taught the reader
 * it does nothing.
 */
const TABS: Array<[Tab, string, string, boolean, boolean]> = [
  ["groups", "Groups", "How much of the range sits in each colour", true, true],
  ["overlap", "Overlap", "How often each statistic comes with each other one", false, false],
  ["eq-matrix", "Eq. matrix", "Equity of every hand in the range", true, true],
  ["eq-graph", "Eq. graph", "The range's equity, strongest hand first", true, true],
  ["hotness", "Hotness", "How each remaining card changes the equity", true, false],
];

/** Which views read the chosen opponent. */
const MEASURED = new Set<Tab>(TABS.filter(([, , , against]) => against).map(([key]) => key));

/**
 * Which views have something to say before a flop is dealt.
 *
 * The two equity ones do, now that a preflop pass works them out - so the
 * control that picks who they are measured against belongs there too. Hotness
 * needs a card still to come, so it stays behind a board.
 */
const BEFORE_THE_FLOP = new Set<Tab>(
  TABS.filter(([, , , , preflop]) => preflop).map(([key]) => key),
);

export function createOutputPanel(): { element: HTMLElement; render: () => void } {
  const panel = document.createElement("section");
  panel.className = "panel panel-output";

  const head = document.createElement("div");
  head.className = "panel-head";
  const title = document.createElement("h2");
  title.className = "panel-title";
  title.textContent = "Output";

  // Which range these views measure against. With two seats it is the other
  // one and there is nothing to choose; with three there is, and reordering the
  // seats to get at it - which is the only thing that would work otherwise -
  // is a strange way to ask a question about two of them.
  const versusButton = document.createElement("button");
  versusButton.type = "button";
  versusButton.className = "btn versus-button versus-output";
  // Round the seats in order and then back to the field, which is where it
  // starts and what most readers want most of the time.
  versusButton.addEventListener("click", cycleVersusSeat);
  head.append(title, versusButton);

  const tabs = document.createElement("div");
  tabs.className = "output-tabs";
  const buttons = new Map<Tab, HTMLButtonElement>();
  for (const [key, label, hint] of TABS) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "btn output-tab";
    button.textContent = label;
    button.title = hint;
    button.dataset.tab = key;
    button.addEventListener("click", () => {
      chrome.output = key;
      repaint();
    });
    buttons.set(key, button);
    tabs.append(button);
  }

  const body = document.createElement("div");
  body.className = "output-body";

  panel.append(head, tabs, body);

  const render = () => {
    const seen = state();
    // Only where it changes what is drawn. Preflop there is no board to be
    // measured on, and the two views that read it are empty anyway.
    const others = seen.players.map((_, index) => index).filter((index) => index !== seen.active);
    versusButton.hidden =
      others.length === 0 ||
      !MEASURED.has(chrome.output) ||
      (seen.board === "" && !BEFORE_THE_FLOP.has(chrome.output));
    if (!versusButton.hidden) {
      const other = seen.versusSeat === null ? null : seen.players[seen.versusSeat];
      const named = other ? seatName(other) : null;
      versusButton.textContent = other ? `vs ${seatTag(other)}` : "vs all";
      versusButton.classList.toggle("active", named !== null);
      versusButton.style.setProperty("--seat", `var(--seat-${seen.versusSeat ?? 0})`);
      versusButton.title = named
        ? `The equity views measure against ${named}. Press for the next range.`
        : others.length > 1
          ? "The equity views measure against every other range at once — a hand drawn from the table. Press to pick one range instead."
          : "The equity views measure against the other range. Press to name it.";
    }
    buttons.forEach((button, key) => button.classList.toggle("active", key === chrome.output));

    // Rebuilding the view is the expensive part of a repaint - the table under
    // the equity graph runs to hundreds of rows - and most repaints have
    // nothing new in them: the pointer crossed a row, and all that changed is
    // which hand is being pointed at.
    //
    // Rebuilding for that was worse than slow. The table scrolls, a fresh one
    // comes in scrolled to the top, and a wheel that moves the rows under a
    // still pointer sets off a repaint per row it crosses - so scrolling it
    // fought back. Pointing at a hand now moves a class and nothing else.
    // Everything these views are drawn from. The equity is in here on its own
    // account: it rides along with a pass but goes stale on its own terms, so
    // a pass that was already standing can gain one without anything else here
    // moving - and the views would have gone on showing the note that asked
    // for it.
    const from = `${chrome.output}/${revision()}/${chrome.showCombos}/${chrome.overlapAxes}/${
      chrome.preflop === null ? "none" : "pass"
    }/${chrome.preflopRunning}/${preflopEquityReady()}`;
    if (built.get(body) !== from) {
      built.set(body, from);
      body.replaceChildren(...view());
    }
    markPeek(body);
  };

  return { element: panel, render };
}

/**
 * What each panel was last built from, so an unchanged view is left alone.
 *
 * Per panel rather than per module: two of them on the page and the second
 * would be told its view was already drawn, when what had been drawn was the
 * first one's.
 */
const built = new WeakMap<HTMLElement, string>();

/**
 * Marks the hand being pointed at, without rebuilding anything.
 *
 * The one part of these views that changes as the pointer moves, so it is the
 * one part a pointer move is allowed to touch.
 */
function markPeek(body: HTMLElement): void {
  for (const cell of body.querySelectorAll<HTMLElement>(".eq-cell")) {
    cell.classList.toggle("peek", chrome.peekClass === Number(cell.dataset.klass));
  }
  for (const row of body.querySelectorAll<HTMLElement>(".eq-row[data-combo]")) {
    row.classList.toggle("peek", chrome.peekCombo === Number(row.dataset.combo));
  }
}

function view(): Node[] {
  switch (chrome.output) {
    case "groups":
      return groupsView();
    case "overlap":
      return overlapView();
    case "eq-matrix":
      return equityMatrixView();
    case "eq-graph":
      return equityGraphView();
    case "hotness":
      return hotnessView();
  }
}

/**
 * What the two equity views show before there is a board.
 *
 * With cards down, equity is enumerated: every run-out, exactly, in a few tens
 * of milliseconds, so it happens on every redraw and nobody is asked anything.
 * With no board there are 2.6 million run-outs, so the answer is sampled - a
 * second of work, which is too much to spend on a redraw.
 *
 * It is not a second button, though. It is the same question the statistics
 * panel already asks of the same flops, over the same ticked groups, so it is
 * answered by the same pass: run that one and these fill in.
 *
 * Returns `null` once a pass is standing, which is the caller's cue to draw it.
 */
function preflopEquityGate(): Node[] | null {
  if (state().board !== "") return null;
  if (preflopEquityReady()) return null;
  if (chrome.preflopRunning) return [note("Working through the flops…")];
  // A session always has a second seat; what matters is whether anything is in
  // it. Counting seats said "run the pass" to a reader who had nothing to
  // measure against, and the pass would have come back with nothing.
  if (!preflopEquityMissing()) {
    return [
      note("Needs something to measure against: a hand in the dead cards, or both seats filled."),
    ];
  }
  return [
    note(
      "Before the flop this comes from the pass over flops — run it in the statistics panel and it works out every hand's equity too, over whichever flop groups are ticked.",
    ),
  ];
}

function note(text: string): HTMLElement {
  const hint = document.createElement("p");
  hint.className = "hint";
  hint.textContent = text;
  return hint;
}

/* ----- overlap ------------------------------------------------------------ */

function overlapView(): Node[] {
  if (!state().board) return [note("Pick a flop: the overlap is between statistics on a board.")];
  const matrix = overlap();
  const block = new Map(statDefs.map((def) => [def.index, def.block]));
  const nonEmpty = (index: number) => matrix.totals[index] > 0;

  // The question this answers most often is "which made hands are also draws",
  // so that is the default. The full square still has answers of its own - how
  // often a flushdraw is also a gutshot, say - so it is a switch, not a rule.
  const madeOnly = chrome.overlapAxes === "made-draws";
  const stats = matrix.stats;
  const rows = stats
    .map((_, position) => position)
    .filter((position) => nonEmpty(position))
    .filter((position) => !madeOnly || block.get(stats[position]) === "made");
  const columns = stats
    .map((_, position) => position)
    .filter((position) => nonEmpty(position))
    .filter((position) => !madeOnly || block.get(stats[position]) !== "made");

  const switcher = document.createElement("div");
  switcher.className = "row axes-row";
  for (const [key, label, hint] of [
    ["made-draws", "Made × draws", "Which made hands are also drawing"],
    ["all", "All", "Every statistic against every other"],
  ] as const) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "btn chip";
    button.textContent = label;
    button.title = hint;
    button.classList.toggle("active", chrome.overlapAxes === key);
    button.addEventListener("click", () => {
      chrome.overlapAxes = key;
      repaint();
    });
    switcher.append(button);
  }

  if (rows.length === 0 || columns.length === 0) {
    return [switcher, note("Nothing in the range holds both sides of this cut.")];
  }

  const table = document.createElement("table");
  table.className = "overlap-table";

  const header = document.createElement("tr");
  header.append(document.createElement("th"));
  for (const column of columns) {
    const cell = document.createElement("th");
    cell.className = "overlap-col";
    const text = document.createElement("span");
    text.textContent = matrix.labels[column];
    cell.append(text);
    cell.title = matrix.labels[column];
    header.append(cell);
  }
  table.append(header);

  for (const row of rows) {
    const line = document.createElement("tr");
    const name = document.createElement("th");
    name.scope = "row";
    name.className = "overlap-row";
    name.textContent = matrix.labels[row];
    name.title = `${matrix.labels[row]}: ${matrix.totals[row].toFixed(1)} combos`;
    line.append(name);
    for (const column of columns) {
      const value = matrix.rows[row][column];
      const cell = document.createElement("td");
      cell.className = "overlap-cell num";
      cell.style.setProperty("--strength", value.toFixed(3));
      cell.textContent = value > 0 ? (value * 100).toFixed(0) : "";
      cell.title =
        `When the range holds ${matrix.labels[row]}, it also holds ${matrix.labels[column]} ` +
        `${(value * 100).toFixed(1)}% of the time. Click to paint the overlap.`;
      if (row === column) cell.classList.add("diagonal");
      // The overlap is where the interesting hands are - a flushdraw that is
      // also a pair - and reaching them from the two rows separately means
      // painting one and then unpainting most of it. Flopzilla puts a key on
      // this; here the cell you are already reading is the button.
      if (value > 0) {
        cell.classList.add("actionable");
        // The statistics themselves, not where they sit in this table. The two
        // were the same number until the ladder stopped being numbered in the
        // order it is shown in, and then this painted whatever happened to be
        // numbered where these two were sitting.
        const [first, second] = [stats[row], stats[column]];
        cell.addEventListener("click", () =>
          mutate((engine) => engine.paintIntersection(first, second, state().colour)),
        );
      }
      line.append(cell);
    }
    table.append(line);
  }

  const wrapper = document.createElement("div");
  wrapper.className = "scroll-x";
  wrapper.append(table);
  return [
    switcher,
    wrapper,
    note(
      "Read a row: given the row's statistic, how often the column's comes with it. " +
        "Click a cell to paint the hands that are both.",
    ),
  ];
}

/* ----- equity matrix and graph -------------------------------------------- */

function perClassEquity(): { equity: number[]; weight: number[] } | null {
  const data = equityByCombo();
  if (!data) return null;
  const equity = new Array<number>(169).fill(0);
  const weight = new Array<number>(169).fill(0);
  for (let combo = 0; combo < data.equity.length; combo += 1) {
    const value = data.equity[combo];
    if (value < 0) continue;
    const cell = comboClass(combo);
    equity[cell] += value * data.weight[combo];
    weight[cell] += data.weight[combo];
  }
  for (let cell = 0; cell < 169; cell += 1) {
    if (weight[cell] > 0) equity[cell] /= weight[cell];
  }
  return { equity, weight };
}

function comboClass(combo: number): number {
  return classOfCombo(combo);
}

function equityMatrixView(): Node[] {
  const gate = preflopEquityGate();
  if (gate) return gate;
  const data = perClassEquity();
  if (!data)
    return [
      note(
        "Needs a board and something to measure against: a hand in the dead cards, or both seats filled.",
      ),
    ];

  const grid = document.createElement("div");
  grid.className = "eq-matrix";
  for (let index = 0; index < 169; index += 1) {
    const cell = document.createElement("div");
    cell.className = "eq-cell";
    cell.dataset.klass = String(index);
    cell.addEventListener("pointerenter", () => peekAt(index));
    cell.addEventListener("pointerleave", () => {
      if (chrome.peekClass === index) peekAt(null);
    });
    // Laid out like the range matrix and labelled like it, so a hand is found
    // in the same place by the same name; the number goes in the corner where
    // the combination count goes over there.
    const label = document.createElement("span");
    label.className = "eq-name";
    label.textContent = classLabels[index] ?? "";
    cell.append(label);
    if (data.weight[index] > 0) {
      const value = data.equity[index];
      cell.classList.add("on");
      cell.style.setProperty("--equity", value.toFixed(3));
      const number = document.createElement("span");
      number.className = "eq-value num";
      number.textContent = (value * 100).toFixed(0);
      cell.append(number);
      cell.title = `${classLabels[index]}: ${(value * 100).toFixed(1)}%`;
    }
    grid.append(cell);
  }
  return [
    grid,
    note("Every hand in the range, coloured by its equity. Red is behind, green is ahead."),
  ];
}

interface Point {
  combo: number;
  equity: number;
  weight: number;
  win: number;
  tie: number;
}

/**
 * Orders hands strongest first, and settles ties by the hand itself.
 *
 * Equity against one opponent runs out of things to say long before the hands
 * do: against a set of tens every flush wins exactly as often, so a queen-high
 * flush and a jack-high one come back with the same number. They are not the
 * same hand, and a list that put the jack above the queen looked like it had
 * got the poker wrong. The board settles it - the better five cards go first -
 * and before the flop, where there is no board, the ranks are empty and the
 * order is whatever the equity said.
 */
function strongestFirst<T extends { combo: number; equity: number }>(hands: T[]): T[] {
  const ranks = rankByCombo();
  hands.sort(
    (a, b) =>
      b.equity - a.equity || (ranks[b.combo] ?? 0) - (ranks[a.combo] ?? 0) || a.combo - b.combo,
  );
  return hands;
}

/** The combos of a range on this board, strongest first. */
function curveOf(data: {
  equity: Float32Array;
  weight: Float32Array;
  win?: Float32Array;
  tie?: Float32Array;
}): Point[] {
  const points: Point[] = [];
  for (let combo = 0; combo < data.equity.length; combo += 1) {
    if (data.equity[combo] >= 0 && data.weight[combo] > 0) {
      points.push({
        combo,
        equity: data.equity[combo],
        weight: data.weight[combo],
        win: data.win?.[combo] ?? 0,
        tie: data.tie?.[combo] ?? 0,
      });
    }
  }
  return strongestFirst(points);
}

function equityGraphView(): Node[] {
  const gate = preflopEquityGate();
  if (gate) return gate;
  const data = equityByCombo();
  if (!data)
    return [
      note(
        "Needs a board and something to measure against: a hand in the dead cards, or both seats filled.",
      ),
    ];

  const view = state();
  const points = curveOf(data);
  if (points.length === 0) return [note("No hands left in the range.")];

  // The other side of the same enumeration. One curve says how a range is
  // distributed; two say which range is ahead, and where the two cross.
  const villain = opponentEquityByCombo();
  const theirs = villain ? curveOf(villain) : [];

  const total = points.reduce((sum, point) => sum + point.weight, 0);
  const width = 100;
  const height = 60;

  // Walk the curve once, keeping where each hand starts so a hover can find it.
  const path: string[] = [];
  const starts: number[] = [];
  let cumulative = 0;
  for (const point of points) {
    const x = (cumulative / total) * width;
    starts.push(x);
    path.push(`${x.toFixed(2)},${((1 - point.equity) * height).toFixed(2)}`);
    cumulative += point.weight;
  }
  path.push(`${width},${((1 - points[points.length - 1].equity) * height).toFixed(2)}`);

  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  svg.setAttribute("preserveAspectRatio", "none");
  svg.classList.add("eq-graph");
  for (const level of [0.25, 0.5, 0.75]) {
    const line = document.createElementNS(ns, "line");
    line.setAttribute("x1", "0");
    line.setAttribute("x2", String(width));
    line.setAttribute("y1", String(level * height));
    line.setAttribute("y2", String(level * height));
    line.setAttribute("class", level === 0.5 ? "grid mid" : "grid");
    svg.append(line);
  }

  if (theirs.length > 0) {
    const otherTotal = theirs.reduce((sum, point) => sum + point.weight, 0);
    const other: string[] = [];
    let walked = 0;
    for (const point of theirs) {
      other.push(
        `${((walked / otherTotal) * width).toFixed(2)},${((1 - point.equity) * height).toFixed(2)}`,
      );
      walked += point.weight;
    }
    other.push(`${width},${((1 - theirs[theirs.length - 1].equity) * height).toFixed(2)}`);
    const line = document.createElementNS(ns, "polyline");
    line.setAttribute("points", other.join(" "));
    // Coloured by which seat it is, not by which one is selected: the strip at
    // the top has already told the reader that A is blue and B is red, and
    // swapping them when the selection changes makes the graph unreadable.
    // Measured against the whole table rather than one seat, the curve is not
    // any seat's, so it wears nobody's colour and says so in the legend.
    const seat = state().versusSeat;
    // Dashed either way: it is the curve drawn under the active seat's, and two
    // ranges agreeing about their best hands would otherwise draw one line that
    // appears to change colour where they part.
    line.setAttribute(
      "class",
      seat === null ? "curve curve-versus curve-field" : `curve curve-versus seat-curve-${seat}`,
    );
    if (seat !== null) line.style.setProperty("--seat", `var(--seat-${seat})`);
    svg.append(line);
  }

  const curve = document.createElementNS(ns, "polyline");
  curve.setAttribute("points", path.join(" "));
  curve.setAttribute("class", `curve seat-curve-${view.active}`);
  curve.style.setProperty("--seat", `var(--seat-${view.active})`);
  svg.append(curve);

  const marker = document.createElementNS(ns, "line");
  marker.setAttribute("class", "cursor");
  marker.setAttribute("y1", "0");
  marker.setAttribute("y2", String(height));
  marker.setAttribute("x1", "0");
  marker.setAttribute("x2", "0");
  marker.setAttribute("visibility", "hidden");
  svg.append(marker);

  const average = points.reduce((sum, p) => sum + p.equity * p.weight, 0) / total;
  const readout = document.createElement("p");
  readout.className = "eq-readout";
  const rest = () => {
    readout.textContent = `${points.length} hands, strongest first · average ${(average * 100).toFixed(2)}%`;
    readout.classList.remove("live");
  };
  rest();

  svg.addEventListener("pointermove", (event) => {
    const box = svg.getBoundingClientRect();
    if (box.width === 0) return;
    const x = ((event.clientX - box.left) / box.width) * width;
    // The last hand whose slice starts at or before the cursor.
    let index = 0;
    while (index + 1 < starts.length && starts[index + 1] <= x) index += 1;
    const point = points[index];
    marker.setAttribute("visibility", "visible");
    marker.setAttribute("x1", starts[index].toFixed(2));
    marker.setAttribute("x2", starts[index].toFixed(2));
    // Say what the hand actually is, not just which two cards it holds.
    // The hand under the cursor here is the hand under the cursor everywhere:
    // the matrix outlines it and the statistics light what it makes.
    peekAt(classOfCombo(point.combo), point.combo);
    const what = describeCombo(point.combo);
    readout.replaceChildren(
      pips(comboName(point.combo)),
      document.createTextNode(
        ` · ${what.length ? what.join(", ") : "preflop"}` +
          ` · ${(point.equity * 100).toFixed(2)}%` +
          ` · ${((starts[index] / width) * 100).toFixed(0)}% of the range is ahead`,
      ),
    );
    readout.classList.add("live");
  });
  svg.addEventListener("pointerleave", () => {
    marker.setAttribute("visibility", "hidden");
    peekAt(null);
    rest();
  });

  const legend = document.createElement("p");
  legend.className = "eq-legend";
  const seats = view.players.map(seatName);
  const other = view.versusSeat;
  legend.append(
    key(
      `seat-${view.active}`,
      `${seats[view.active] ?? "This range"} · ${(average * 100).toFixed(2)}%`,
      view.active,
      "Each hand of this range against the whole of the other one.",
    ),
  );
  if (theirs.length > 0) {
    const otherTotal = theirs.reduce((sum, p) => sum + p.weight, 0);
    const otherAverage = theirs.reduce((sum, p) => sum + p.equity * p.weight, 0) / otherTotal;
    // The two curves are drawn on one axis and do not answer one question.
    // Each is a range's hands against the *other* whole range, so the pair of
    // averages adds up to a hundred - and a side holding one hand is one flat
    // line, sitting where that hand stands against everything this range
    // holds. Which is not a bar this range's hands have to clear: a flush that
    // beats a set three times in four still comes in under a line drawn at the
    // set's equity against a range that is mostly air. Said here, because the
    // graph draws it and cannot say it.
    const single = theirs.length === 1;
    legend.append(
      key(
        other === null ? "field" : `seat-${other}`,
        `${other === null ? "The field" : (seats[other] ?? "Opponent")} · ${(otherAverage * 100).toFixed(2)}%`,
        other ?? undefined,
        single
          ? `${comboName(theirs[0].combo)} against the whole of this range. One hand is one flat line — it says how that hand does against everything here, not how much a hand of this range needs to beat it.`
          : "Each of their hands against the whole of this range. The two averages add up to 100%.",
      ),
    );
  }

  // The markers down the statistics panel, read once rather than per row: they
  // are what says which colour a category carries.
  const marks = state().marks;

  // Under the curve, every hand it is made of. The graph shows the shape; the
  // table is where you look up the hand you actually hold.
  const table = document.createElement("div");
  table.className = "eq-table";
  const header = document.createElement("div");
  header.className = "eq-row eq-head";
  header.innerHTML =
    "<span></span><span>Hand</span><span>Equity</span><span>Win</span><span>Tie</span>";
  table.append(header);
  points.forEach((point, place) => {
    const row = document.createElement("div");
    row.className = "eq-row";
    row.style.setProperty("--heat", point.equity.toFixed(3));
    const rank = document.createElement("span");
    rank.className = "num";
    rank.textContent = `${place + 1}.`;
    const hand = document.createElement("span");
    hand.className = "eq-hand";
    pips(comboName(point.combo), hand);
    hand.title = describeCombo(point.combo).join(", ");
    // Washed with the colours of the categories it belongs to, so the table
    // reads as the same range rather than as a list of strangers. Colours,
    // plural: a hand can be top pair and a flushdraw at once, and if those two
    // are painted differently the reader decided two things about it - so the
    // row is shared between them rather than picking a winner.
    wash(hand, marksOf(point.combo, marks));
    const equity = document.createElement("span");
    equity.className = "num";
    equity.textContent = `${(point.equity * 100).toFixed(3)}`;
    const win = document.createElement("span");
    win.className = "num";
    win.textContent = `${(point.win * 100).toFixed(3)}`;
    const tie = document.createElement("span");
    tie.className = "num";
    tie.textContent = `${(point.tie * 100).toFixed(3)}`;
    row.append(rank, hand, equity, win, tie);
    row.dataset.combo = String(point.combo);
    row.addEventListener("pointerenter", () => peekAt(classOfCombo(point.combo), point.combo));
    row.addEventListener("pointerleave", () => {
      if (chrome.peekCombo === point.combo) peekAt(null);
    });
    table.append(row);
  });

  return [svg, readout, legend, table];
}

/**
 * The colours of the painted categories one hand belongs to.
 *
 * In the panel's own order, and without repeats: two categories painted the
 * same colour are one colour to look at. A category whose hands disagree - a
 * gear - carries no colour of its own, so what is left for a hand in one is the
 * colour it was painted itself, which is the fallback below.
 */
function marksOf(combo: number, marks: string[]): string[] {
  const shares = comboStats(combo);
  const found: string[] = [];
  for (const definition of statDefs) {
    if ((shares[definition.index] ?? 0) <= 0) continue;
    const mark = marks[definition.index];
    if (!mark || mark === "none" || mark === "mixed") continue;
    if (!found.includes(mark)) found.push(mark);
  }
  if (found.length === 0) {
    const own = comboColour(combo);
    if (own !== "none") found.push(own);
  }
  return found;
}

/** Paints a strip of one colour, or a band each where there are several. */
function wash(element: HTMLElement, colours: string[]): void {
  element.dataset.colours = String(colours.length);
  if (colours.length === 0) {
    element.style.removeProperty("background");
    return;
  }
  if (colours.length === 1) {
    markColour(element, colours[0]);
    return;
  }
  const step = 100 / colours.length;
  const bands = colours.map(
    (colour, at) =>
      `color-mix(in srgb, var(--group-${colourSlot(colour)}) 26%, transparent) ${(at * step).toFixed(2)}% ${((at + 1) * step).toFixed(2)}%`,
  );
  element.style.background = `linear-gradient(90deg, ${bands.join(", ")})`;
}

/** One swatch and label in the graph's legend. */
function key(kind: string, label: string, seat?: number, hint?: string): HTMLElement {
  const item = document.createElement("span");
  item.className = `eq-key key-${kind}`;
  if (seat !== undefined) item.style.setProperty("--seat", `var(--seat-${seat})`);
  if (hint) item.title = hint;
  item.textContent = label;
  return item;
}

/* ----- groups ------------------------------------------------------------- */

/**
 * The pie: how much of the range sits in each colour.
 *
 * The one reading that says whether a plan is balanced. "I continue with 40% of
 * this range" is a sentence you can act on; a list of which categories you
 * marked is not.
 */
function groupsView(): Node[] {
  const view = state();
  // With no board there is no paint - the palette is not even on screen - so
  // the colours have nothing to divide, and the pass over every flop is what
  // there is to cut up instead.
  if (view.board === "") return preflopGroupsView();

  const shares = view.groupShares;
  const total = shares.reduce((sum, share) => sum + share, 0);
  if (total <= 0) return [note("Nothing in the range to group.")];

  const names = ["unpainted", ...palette()];
  const grains = paintedGrains(shares.length);
  const slices: Slice[] = shares
    .map((weight, colour) => ({ colour, weight, share: weight / total }))
    .filter((slice) => slice.weight > 0)
    .map((slice) => ({
      mark: `var(--group-${slice.colour})`,
      name: names[slice.colour] ?? "",
      share: slice.share,
      value: chrome.showCombos ? slice.weight.toFixed(1) : `${(slice.share * 100).toFixed(2)}%`,
      title: `${names[slice.colour]}: ${(slice.share * 100).toFixed(2)}%`,
      grains: grains?.[slice.colour],
    }));

  const painted = total - (shares[0] ?? 0);
  const drawn = grains ? grainyPie(slices) : null;
  return [
    drawn ? drawn.svg : pie(slices),
    ...(drawn ? [drawn.readout] : []),
    pieLegend(slices),
    note(
      view.filtersEnabled
        ? `${total.toFixed(1)} combos left after the street filters.`
        : `${total.toFixed(1)} combos, ${((painted / total) * 100).toFixed(2)}% painted. Press a street filter to keep only those.`,
    ),
  ];
}

/** One wedge: the colour to draw it in, what it is, and how much of the whole. */
interface Slice {
  mark: string;
  name: string;
  share: number;
  /** What the legend puts in its right-hand column. */
  value: string;
  /** What the wedge and its key say under the pointer. */
  title: string;
  /** The hands the wedge is made of, strongest first. Empty when unknown. */
  grains?: Grain[];
}

/** One hand inside a wedge: how much of it is here, and what it is worth. */
interface Grain {
  combo: number;
  weight: number;
  equity: number;
}

/** How finely the gradient is drawn: past this, two neighbours are one colour. */
const SHADES = 256;

/** Where the hands stop and the band naming their group begins. */
const INNER = 0.84;

/**
 * The hands behind each colour, strongest first, or `null`.
 *
 * `null` is the honest answer whenever there is nothing to sort them by -
 * nobody to measure against, or no answer worked out yet - and the pie goes
 * back to being the flat wedges it always was.
 */
function paintedGrains(count: number): Grain[][] | null {
  const data = equityByCombo();
  if (!data) return null;
  const slots = colourByCombo();
  const groups: Grain[][] = Array.from({ length: count }, () => []);
  for (let combo = 0; combo < data.equity.length; combo += 1) {
    const weight = data.weight[combo];
    if (weight <= 0 || data.equity[combo] < 0) continue;
    groups[slots[combo] ?? 0]?.push({ combo, weight, equity: data.equity[combo] });
  }
  return sorted(groups);
}

/**
 * The hands behind each tier of a pass over the flops, strongest first.
 *
 * A hand is not in one tier here: aces are an overpair on most flops and a set
 * on some, so the same hand is in several wedges at the weight it carries in
 * each. That is what the pass counted, and it is what makes the wedge readable
 * - "which of my hands make this, and what are they worth".
 */
function tierGrains(count: number): Grain[][] | null {
  const data = equityByCombo();
  const tiers = preflopTiers();
  if (!data || tiers.length === 0) return null;
  const groups: Grain[][] = Array.from({ length: count }, () => []);
  for (let combo = 0; combo < data.equity.length; combo += 1) {
    if (data.equity[combo] < 0) continue;
    for (let tier = 0; tier < count; tier += 1) {
      const weight = tiers[combo * count + tier] ?? 0;
      if (weight > 0) groups[tier].push({ combo, weight, equity: data.equity[combo] });
    }
  }
  return sorted(groups);
}

/** Strongest hand first, which is the order every wedge is drawn in. */
function sorted(groups: Grain[][]): Grain[][] {
  for (const grains of groups) strongestFirst(grains);
  return groups;
}

/** The path of one wedge, as a share of the circle from `from` to `to`. */
function wedge(from: number, to: number, radius = 1): string {
  const a = from * Math.PI * 2 - Math.PI / 2;
  const b = to * Math.PI * 2 - Math.PI / 2;
  const large = b - a > Math.PI ? 1 : 0;
  const r = radius.toFixed(4);
  return (
    `M 0 0 L ${(Math.cos(a) * radius).toFixed(5)} ${(Math.sin(a) * radius).toFixed(5)} ` +
    `A ${r} ${r} 0 ${large} 1 ${(Math.cos(b) * radius).toFixed(5)} ${(Math.sin(b) * radius).toFixed(5)} Z`
  );
}

/** The arc alone, for the band that names a group. */
function arc(from: number, to: number, radius: number): string {
  const a = from * Math.PI * 2 - Math.PI / 2;
  const b = to * Math.PI * 2 - Math.PI / 2;
  const large = b - a > Math.PI ? 1 : 0;
  const r = radius.toFixed(4);
  return (
    `M ${(Math.cos(a) * radius).toFixed(5)} ${(Math.sin(a) * radius).toFixed(5)} ` +
    `A ${r} ${r} 0 ${large} 1 ${(Math.cos(b) * radius).toFixed(5)} ${(Math.sin(b) * radius).toFixed(5)}`
  );
}

function pie(slices: Slice[]): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "-1.05 -1.05 2.1 2.1");
  svg.classList.add("pie");

  // A single slice is a whole circle, which no arc path can draw.
  if (slices.length === 1) {
    const circle = document.createElementNS(ns, "circle");
    circle.setAttribute("r", "1");
    circle.setAttribute("class", "slice");
    circle.style.setProperty("--mark", slices[0].mark);
    const label = document.createElementNS(ns, "title");
    label.textContent = slices[0].title;
    circle.append(label);
    svg.append(circle);
    return svg;
  }

  let at = 0;
  for (const slice of slices) {
    const path = document.createElementNS(ns, "path");
    path.setAttribute("d", wedge(at, at + slice.share));
    path.setAttribute("class", "slice");
    path.style.setProperty("--mark", slice.mark);
    const label = document.createElementNS(ns, "title");
    label.textContent = slice.title;
    path.append(label);
    svg.append(path);
    at += slice.share;
  }
  return svg;
}

/* ----- the pie read hand by hand ------------------------------------------ */

/**
 * The same pie, drawn out of the hands it is made of.
 *
 * A wedge says how much of the range is in a group. It does not say what is in
 * there, and two groups of the same size can be nothing alike: one a run of
 * hands that are all worth about the same, the other the best and the worst of
 * the range with nothing in between. That second one is the shape a reader
 * most wants to see - a group that polar is usually a mistake, or a bluffing
 * range - and a flat wedge hides it completely.
 *
 * So each wedge is drawn hand by hand, strongest first, in the colour the
 * equity views already use: green ahead, red behind, mixed in proportion. A
 * group of like hands is one even colour; a polar group runs from green to red
 * inside a single wedge, and the place it turns over is the place the reader
 * is looking for. Nothing is drawn on top to point at it - the colour is the
 * thing being read.
 *
 * Which group a wedge is stays at the rim, as a band in the group's own
 * colour. It is the outline of the answer rather than the answer, and keeping
 * it out of the middle leaves the middle to say one thing only.
 *
 * The scale is absolute: a dark red hand is a weak hand here and on the next
 * board too.
 */
function grainyPie(slices: Slice[]): { svg: SVGSVGElement; readout: HTMLElement } {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "-1.05 -1.05 2.1 2.1");
  svg.classList.add("pie", "pie-grainy");

  // Where every hand sits on the circle, so a pointer can be turned back into
  // one. Built as the wedges are drawn, because the two have to agree.
  const stops: number[] = [];
  const hands: Array<{ grain: Grain; slice: Slice }> = [];
  const whole = slices.length === 1;

  let at = 0;
  for (const slice of slices) {
    const grains = slice.grains ?? [];
    const held = grains.reduce((sum, grain) => sum + grain.weight, 0);
    const group = document.createElementNS(ns, "g");
    group.setAttribute("class", "slice-group");

    if (held <= 0) {
      const path = document.createElementNS(ns, "path");
      path.setAttribute("d", wedge(at, at + slice.share, INNER));
      path.setAttribute("class", "slice");
      path.style.setProperty("--mark", slice.mark);
      group.append(path);
    } else {
      // One path per shade rather than one per hand: past a certain fineness
      // two neighbours are the same colour, and a wide range is a thousand
      // hands. Fine enough that the joins cannot be seen - what is drawn is a
      // gradient, not a flight of steps.
      let runFrom = at;
      let runShade = -1;
      let runEquity = 0;
      const close = (to: number) => {
        if (to <= runFrom || runShade < 0) return;
        const path = document.createElementNS(ns, "path");
        path.setAttribute(
          "d",
          whole && to - runFrom >= 0.9999
            ? `M 0 ${(-INNER).toFixed(4)} A ${INNER.toFixed(4)} ${INNER.toFixed(4)} 0 1 1 0 ${INNER.toFixed(4)} A ${INNER.toFixed(4)} ${INNER.toFixed(4)} 0 1 1 0 ${(-INNER).toFixed(4)} Z`
            : wedge(runFrom, to, INNER),
        );
        path.setAttribute("class", "grain");
        path.style.setProperty("--heat", runEquity.toFixed(4));
        group.append(path);
      };

      let walked = at;
      for (const grain of grains) {
        const width = (grain.weight / held) * slice.share;
        stops.push(walked);
        hands.push({ grain, slice });
        const shade = Math.round(Math.min(1, Math.max(0, grain.equity)) * SHADES);
        if (shade !== runShade) {
          close(walked);
          runFrom = walked;
          runShade = shade;
          runEquity = grain.equity;
        }
        walked += width;
      }
      close(at + slice.share);
    }

    // The band at the rim, which is the one place the group's own colour is
    // said. A gap at each end so neighbouring bands do not read as one.
    const gap = Math.min(0.004, slice.share / 6);
    const band = document.createElementNS(ns, "path");
    band.setAttribute(
      "d",
      whole
        ? `M 0 ${(-(INNER + 1) / 2).toFixed(4)} A ${((INNER + 1) / 2).toFixed(4)} ${((INNER + 1) / 2).toFixed(4)} 0 1 1 0 ${((INNER + 1) / 2).toFixed(4)} A ${((INNER + 1) / 2).toFixed(4)} ${((INNER + 1) / 2).toFixed(4)} 0 1 1 0 ${(-(INNER + 1) / 2).toFixed(4)} Z`
        : arc(at + gap, at + slice.share - gap, (INNER + 1) / 2),
    );
    band.setAttribute("class", "slice-rim");
    band.style.setProperty("--mark", slice.mark);
    const label = document.createElementNS(ns, "title");
    label.textContent = slice.title;
    band.append(label);
    group.append(band);
    svg.append(group);
    at += slice.share;
  }

  const cursor = document.createElementNS(ns, "line");
  cursor.setAttribute("class", "pie-cursor");
  cursor.setAttribute("x1", "0");
  cursor.setAttribute("y1", "0");
  cursor.setAttribute("x2", "0");
  cursor.setAttribute("y2", String(-INNER));
  cursor.setAttribute("visibility", "hidden");
  svg.append(cursor);

  const readout = document.createElement("p");
  readout.className = "eq-readout pie-readout";
  const rest = () => {
    readout.textContent = `${hands.length} hands in ${slices.length} group${slices.length === 1 ? "" : "s"} · strongest first, coloured by equity`;
    readout.classList.remove("live");
  };
  rest();

  svg.addEventListener("pointermove", (event) => {
    const box = svg.getBoundingClientRect();
    if (box.width === 0 || hands.length === 0) return;
    // Where the pointer is on the circle, as a share of the way round from
    // twelve o'clock - which is where the first wedge starts.
    const x = ((event.clientX - box.left) / box.width) * 2.1 - 1.05;
    const y = ((event.clientY - box.top) / box.height) * 2.1 - 1.05;
    if (Math.hypot(x, y) > 1.05) {
      leave();
      return;
    }
    let angle = Math.atan2(y, x) + Math.PI / 2;
    if (angle < 0) angle += Math.PI * 2;
    const share = angle / (Math.PI * 2);
    let index = 0;
    while (index + 1 < stops.length && stops[index + 1] <= share) index += 1;
    const { grain, slice } = hands[index];
    cursor.setAttribute("visibility", "visible");
    const line = share * Math.PI * 2 - Math.PI / 2;
    cursor.setAttribute("x2", (Math.cos(line) * INNER).toFixed(5));
    cursor.setAttribute("y2", (Math.sin(line) * INNER).toFixed(5));
    // The hand under the pointer here is the hand under the pointer
    // everywhere: the matrix outlines it and the statistics light what it
    // makes.
    peekAt(classOfCombo(grain.combo), grain.combo);
    const what = describeCombo(grain.combo);
    readout.replaceChildren(
      pips(comboName(grain.combo)),
      document.createTextNode(
        ` · ${slice.name}` +
          (what.length ? ` · ${what.join(", ")}` : "") +
          ` · ${(grain.equity * 100).toFixed(2)}%`,
      ),
    );
    readout.classList.add("live");
  });
  const leave = () => {
    cursor.setAttribute("visibility", "hidden");
    peekAt(null);
    rest();
  };
  svg.addEventListener("pointerleave", leave);

  return { svg, readout };
}

function pieLegend(slices: Slice[]): HTMLElement {
  const legend = document.createElement("div");
  legend.className = "pie-legend";
  for (const slice of slices) {
    const row = document.createElement("div");
    row.className = "pie-key";
    row.style.setProperty("--mark", slice.mark);
    row.title = slice.title;
    const name = document.createElement("span");
    name.className = "pie-name";
    name.textContent = slice.name;
    const value = document.createElement("span");
    value.className = "num";
    value.textContent = slice.value;
    row.append(name, value);
    legend.append(row);
  }
  return legend;
}

/* ----- the pie before a flop ---------------------------------------------- */

/**
 * The made ladder cut into four, strongest first.
 *
 * Seventeen rungs is a table, not a pie: half of them are slivers a reader
 * cannot see, let alone compare. Four tiers are the cuts a preflop decision
 * actually turns on, and each is a run of the ladder rather than a list of
 * rungs, so a rung added between two boundaries falls in the tier it was added
 * to rather than going missing.
 */
const LADDER_TIERS: Array<{ from: string; name: string; mark: string }> = [
  { from: "straight-flush", name: "two pair or better", mark: "var(--group-2)" },
  { from: "overpair", name: "top pair or an overpair", mark: "var(--group-1)" },
  { from: "pp-below-top-card", name: "a weaker pair", mark: "var(--group-5)" },
  { from: "ace-high", name: "no pair", mark: "var(--group-0)" },
];

/**
 * The pie with no board dealt.
 *
 * Preflop the panel next door is a different tool and so is this: the slices
 * come from the pass over every flop rather than from paint. With nothing
 * ticked it is the hand the range makes on a flop it has not seen; with ticks
 * it is the hit and the miss they define, which is the question ticking asks.
 *
 * The unit button has nothing to switch to here, because a count would be
 * hand-and-flop pairs rather than combinations, which is not a number anyone
 * holds.
 */
function preflopGroupsView(): Node[] {
  const view = state();
  const waiting = view.filteredFlops.toLocaleString();
  if (chrome.preflopRunning) return [note(`Working through ${waiting} flops…`)];

  const pass = chrome.preflop;
  // A circle of one colour reading "unpainted 100%" is a drawing of nothing.
  // Saying there is nothing yet, and what would put something there, is worth
  // more than the circle.
  if (!pass) {
    return [
      note(
        `Nothing to divide up yet. Run the pass over ${
          view.flopGroups.length > 0 ? `the ${waiting} flops picked` : `all ${waiting} flops`
        } from the statistics panel, and this becomes what the range makes on a flop it has ` +
          "not seen.",
      ),
    ];
  }
  if (pass.total <= 0) return [note("Nothing in the range to group.")];

  const checkmarks = view.checkmarks;
  const ticks = statDefs.filter((def) => checkmarks[def.index]).map((def) => def.label);
  const slices = ticks.length > 0 ? hitSlices(pass) : ladderSlices(pass);
  const flops = pass.flops.toLocaleString();
  // Hit and miss are not tiers: whether a hand hit depends on the flop it saw,
  // so there is no list of hands behind either wedge to sort. The ladder does
  // have one, and the pass counted it hand by hand.
  const drawn = slices.some((slice) => (slice.grains?.length ?? 0) > 0) ? grainyPie(slices) : null;
  return [
    drawn ? drawn.svg : pie(slices),
    ...(drawn ? [drawn.readout] : []),
    pieLegend(slices),
    note(
      ticks.length > 0
        ? `A hand of the range against one of the ${flops} flops, both drawn at random, ` +
            `holds one of the ticked statistics — ${ticks.join(", ")} — ` +
            `${(pass.hit * 100).toFixed(2)}% of the time.`
        : `What a hand of the range makes against one of the ${flops} flops, both drawn ` +
            "at random. Draws are not rungs of the ladder, so a flushdraw with no pair " +
            "counts as no pair. Tick what you would call a hit in the statistics panel " +
            "and the pie becomes how often the range hits.",
    ),
  ];
}

/** The made ladder, averaged over every flop and gathered into its four tiers. */
function ladderSlices(pass: PreflopBreakdown): Slice[] {
  const tiers = LADDER_TIERS.map((tier) => ({ ...tier, share: 0, rungs: [] as string[] }));
  let at = 0;
  for (const row of pass.rows) {
    if (row.block !== "made") continue;
    const boundary = LADDER_TIERS.findIndex((tier) => tier.from === row.key);
    if (boundary >= 0) at = boundary;
    tiers[at].share += row.fraction;
    if (row.fraction > 0) tiers[at].rungs.push(`${row.label} ${(row.fraction * 100).toFixed(2)}%`);
  }
  // Attached before the empty tiers are dropped: the pass counts hands by tier
  // number, and a tier nothing reached would shift every one after it.
  const grains = tierGrains(LADDER_TIERS.length);
  return tiers
    .map((tier, index) => ({
      mark: tier.mark,
      name: tier.name,
      share: tier.share,
      value: `${(tier.share * 100).toFixed(2)}%`,
      title: `${tier.name}: ${(tier.share * 100).toFixed(2)}% — ${tier.rungs.join(", ")}`,
      grains: grains?.[index],
    }))
    .filter((tier) => tier.share > 0);
}

/** The split the checkmarks define: hands that hit one of them, and the rest. */
function hitSlices(pass: PreflopBreakdown): Slice[] {
  return [
    { mark: "var(--group-2)", name: "hits", share: pass.hit },
    { mark: "var(--group-0)", name: "misses", share: 1 - pass.hit },
  ]
    .filter((slice) => slice.share > 0)
    .map((slice) => ({
      ...slice,
      value: `${(slice.share * 100).toFixed(2)}%`,
      title: `${slice.name}: ${(slice.share * 100).toFixed(2)}%`,
    }));
}

/* ----- hotness ------------------------------------------------------------ */

function hotnessView(): Node[] {
  const cards = hotness();
  if (!cards) {
    return [
      note(
        "Needs something to measure against, and a flop or a turn so there is a card still to " +
          "come. It is about the range on this seat - a dealt hand is one of those, and reads " +
          "the same way.",
      ),
    ];
  }
  const sorted = [...cards].sort((a, b) => b.equity - a.equity);
  const low = sorted[sorted.length - 1].equity;
  const high = sorted[0].equity;
  const span = Math.max(high - low, 1e-6);

  // The equity as it stands is the line the cards are read against: above it a
  // card helps, below it a card hurts, and the count of each is the summary.
  const view = state();
  const current = view.equity?.players[0]?.equity ?? (high + low) / 2;
  const better = cards.filter((card) => card.equity > current).length;

  // Thirteen ranks down, four suits across, which is how Flopzilla lays it out
  // and the only way the number under each card has room to be read. A card the
  // board or the dead cards have taken leaves its place empty rather than
  // shifting everything after it.
  const byCard = new Map(cards.map((card) => [card.card, card]));
  const grid = document.createElement("div");
  grid.className = "hot-grid";
  for (const rank of RANKS) {
    for (const suit of SUITS) {
      const card = byCard.get(`${rank}${suit}`);
      const cell = document.createElement("div");
      if (!card) {
        cell.className = "hot-card gone";
        grid.append(cell);
        continue;
      }
      cell.className = `hot-card suit-${suit}`;
      cell.style.setProperty("--heat", ((card.equity - low) / span).toFixed(3));
      // The number under the card is the whole point: "green" is a direction,
      // "54.27%" is an amount.
      cell.innerHTML =
        `<span class="hot-name">${rank}<i>${SUIT_GLYPH[suit]}</i></span>` +
        `<span class="hot-value num">${(card.equity * 100).toFixed(2)}</span>`;
      cell.title = `${card.card}: ${(card.equity * 100).toFixed(2)}%`;
      grid.append(cell);
    }
  }

  const table = document.createElement("div");
  table.className = "hot-table";
  sorted.forEach((card, place) => {
    const row = document.createElement("div");
    row.className = `hot-row suit-${card.card[1]}`;
    row.classList.toggle("helps", card.equity > current);
    const rank = document.createElement("span");
    rank.className = "hot-rank num";
    rank.textContent = `${place + 1}.`;
    const name = document.createElement("span");
    name.className = "hot-name";
    name.innerHTML = `${card.card[0]}<i>${SUIT_GLYPH[card.card[1]]}</i>`;
    const value = document.createElement("span");
    value.className = "num";
    value.textContent = `${(card.equity * 100).toFixed(3)}%`;
    row.append(rank, name, value);
    table.append(row);
  });

  const line = document.createElement("p");
  line.className = "hot-current";
  line.textContent = `Now ${(current * 100).toFixed(3)}%`;

  return [
    grid,
    line,
    table,
    note(
      `${better} cards increase equity, ${cards.length - better} decrease it. ` +
        `Best ${sorted[0].card} at ${(sorted[0].equity * 100).toFixed(2)}%, ` +
        `worst ${sorted[sorted.length - 1].card} at ${(sorted[sorted.length - 1].equity * 100).toFixed(2)}%.`,
    ),
  ];
}

/* ----- flop breakdown ----------------------------------------------------- */
