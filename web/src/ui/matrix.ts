/**
 * The 13x13 starting-hand matrix.
 *
 * Pairs run down the diagonal, suited hands sit above it and offsuit hands below,
 * and each of the three regions has its own colour so the shape of a range reads
 * at a glance.
 *
 * A cell is filled **from the bottom in proportion to its weight**, which is how
 * Flopzilla shows a hand played only part of the time - a washed-out tint cannot
 * be told apart from a colour, but a half-filled cell can. The small number is the
 * count of combos selected in that cell.
 */

import {
  cellCombos,
  chrome,
  classLabels,
  highlight,
  colourSlot,
  comboStats,
  markColour,
  mutate,
  peekAt,
  repaint,
  revision,
  state,
  statDefs,
} from "../store";
import { SUIT_GLYPH } from "./cards";
import { press, touchOnly } from "./press";

let painting: number | null = null;

/** Suits in the order the popup lays them out, and the cards elsewhere. */
const SUITS = ["s", "h", "d", "c"] as const;

export function createMatrix(): HTMLElement {
  const frame = document.createElement("div");
  frame.className = "matrix-frame";

  const grid = document.createElement("div");
  grid.className = "matrix";
  grid.setAttribute("role", "grid");
  grid.setAttribute("aria-label", "Starting hands");

  for (let index = 0; index < 169; index += 1) {
    const cell = document.createElement("button");
    cell.type = "button";
    cell.className = "cell";
    cell.dataset.index = String(index);
    cell.tabIndex = -1;

    const row = Math.floor(index / 13);
    const col = index % 13;
    if (row === col) cell.classList.add("pair");
    else if (row < col) cell.classList.add("suited");
    else cell.classList.add("offsuit");

    const label = document.createElement("span");
    label.className = "cell-label";
    const count = document.createElement("span");
    count.className = "cell-count";
    // No hint on the cell itself: hovering it opens the suit breakdown, and the
    // breakdown says in words what shift-clicking would do. Two offers for one
    // key, one of them a bare glyph, is worse than one made properly.
    cell.append(label, count);

    press(cell, {
      act: (event) => {
        const erase = event.button === 2 || state().classWeights[index] > 0;
        painting = erase ? 0 : chrome.brush;
        apply(index);
        // A finger has no hover to open the breakdown with, so a tap leaves it
        // showing the cell it just painted.
        if (event.pointerType === "touch" && chrome.suitCell === null) {
          chrome.suitPeek = index;
          repaint();
        }
      },
      // Shift pins the suit breakdown open, so the brush can be dragged across
      // it; hovering already shows it, and pinning is what makes it usable. A
      // held finger is the same request from a hand that has no shift key.
      hold: () => {
        chrome.suitCell = chrome.suitCell === index ? null : index;
        chrome.suitPeek = chrome.suitCell;
        repaint();
      },
    });
    cell.addEventListener("pointerenter", () => {
      if (painting !== null) apply(index);
      // The breakdown follows the pointer unless one has been pinned.
      if (chrome.suitCell === null && chrome.suitPeek !== index) {
        chrome.suitPeek = index;
        chrome.peekClass = index;
        chrome.peekCombo = null;
        repaint();
      } else {
        peekAt(index);
      }
    });
    cell.addEventListener("contextmenu", (event) => event.preventDefault());

    grid.append(cell);
  }

  grid.addEventListener("pointerleave", () => {
    if (chrome.suitCell === null && chrome.suitPeek !== null) {
      chrome.suitPeek = null;
    }
    peekAt(null);
  });

  window.addEventListener("pointerup", () => {
    painting = null;
    suitBrush = null;
  });

  frame.append(grid, createSuitPopup());
  return frame;
}

function apply(index: number): void {
  const weight = painting ?? 0;
  mutate((engine) => engine.setClassWeight(index, weight));
}

/**
 * What the last full pass over the cells was drawn from, and which cells wear
 * the classes that follow the pointer.
 *
 * Moving the pointer one cell across changes three cells at most, and used to
 * rewrite all hundred and sixty-nine: every fill, every gradient, every class.
 * That is fifteen hundred writes to the page for a mouse moving a centimetre,
 * and it is what a hover felt like.
 */
/*
 * Kept per matrix rather than per module.
 *
 * One of these on the page is the ordinary case, and a cache in a module
 * variable is right up until there are two - then the second one asks "have I
 * drawn this?", is told yes about the first one's cells, and draws nothing. Its
 * own cells stay as they were built: a grid of a hundred and sixty-nine blanks.
 */
const drawn = new WeakMap<HTMLElement, { from: string; wearing: Wearing }>();

/** Which cell is wearing each of the classes that follow the pointer. */
interface Wearing {
  open: number;
  peeking: number;
  peek: number;
}

export function renderMatrix(frame: HTMLElement): void {
  const grid = frame.querySelector<HTMLElement>(".matrix")!;
  const view = state();
  const cells = grid.children;
  const before = drawn.get(frame) ?? { from: "", wearing: { open: -1, peeking: -1, peek: -1 } };
  const wearing = before.wearing;
  drawn.set(frame, before);

  // The classes that follow the pointer, moved from the cells that had them to
  // the cells that want them and nowhere else.
  const wants = {
    open: chrome.suitCell ?? -1,
    peeking: chrome.suitPeek ?? -1,
    peek: chrome.peekClass !== null && chrome.peekClass !== chrome.suitPeek ? chrome.peekClass : -1,
  };
  for (const name of ["open", "peeking", "peek"] as const) {
    if (wearing[name] === wants[name]) continue;
    (cells[wearing[name]] as HTMLElement | undefined)?.classList.remove(name);
    (cells[wants[name]] as HTMLElement | undefined)?.classList.add(name);
    wearing[name] = wants[name];
  }

  // The breakdown is about the cell under the pointer, so it follows it.
  const popup = frame.querySelector<HTMLElement>(".suit-popup")!;
  renderSuitPopup(popup, chrome.suitCell ?? chrome.suitPeek, view.filtersEnabled);
  if (!popup.hidden) {
    // Anchored to the cell it is about, and nudged back inside the panel when
    // the cell is near an edge.
    const which = chrome.suitCell ?? chrome.suitPeek ?? 0;
    popup.style.setProperty("--row", String(Math.floor(which / 13)));
    popup.style.setProperty("--col", String(which % 13));
    popup.classList.toggle("to-left", which % 13 > 6);
    popup.classList.toggle("to-top", Math.floor(which / 13) > 6);
  }

  // Everything else is about the range and the board, so it is redrawn when
  // those move and not when the pointer does.
  const from = `${revision()}/${chrome.hovered}/${view.filtersEnabled}/${chrome.visible}`;
  if (from === before.from) return;
  before.from = from;

  // Hovering a statistic is a question being asked right now, so it wins the
  // glow; the cut keeps its own marker underneath either way.
  const lit = chrome.hovered !== null ? highlight(chrome.hovered) : null;

  for (let index = 0; index < 169; index += 1) {
    const cell = cells[index] as HTMLElement;
    const weight = view.classWeights[index] ?? 0;
    const combos = view.classCombos[index] ?? 0;
    const [label, count] = cell.children as unknown as [HTMLElement, HTMLElement];

    label.textContent = classLabels[index] ?? "";
    count.textContent = combos > 0 ? formatCombos(combos) : "";
    cell.title = `${classLabels[index]} — ${formatCombos(combos)} combos`;

    cell.classList.toggle("on", weight > 0);
    cell.classList.toggle("partial", weight > 0 && weight < 0.999);
    cell.style.setProperty("--fill", `${(weight * 100).toFixed(1)}%`);

    // The glow covers the share of the cell that matches, at full strength,
    // rather than washing the whole cell at a fraction of it: one flushdraw in
    // twelve was a tint nobody could see. Down from the top, so it does not
    // collide with the weight filling up or the filter coming in from the left.
    const glow = lit ? (lit[index] ?? 0) : 0;
    cell.classList.toggle("lit", glow > 0);
    cell.style.setProperty("--glow", `${(glow * 100).toFixed(1)}%`);

    // Two divisions, at right angles, so they never have to compete. The
    // horizontal one is how much of the cell is in the range; the vertical one
    // is how much of that survived the street filters.
    const passing = view.classPassing[index] ?? 0;
    cell.classList.toggle("filtered", view.filtersEnabled && weight > 0 && passing < 0.9995);
    cell.style.setProperty("--passing", `${(passing * 100).toFixed(1)}%`);

    // The colour mark only earns its space once there is more than one colour
    // to tell apart. With a single colour it says nothing the cell does not.
    const colours = view.classColours[index] ?? [];
    const marked = view.coloursUsed > 1 && colours.slice(1).some((share) => share > 0);
    cell.classList.toggle("grouped", marked);
    cell.style.setProperty("--groups", marked ? stripe(colours) : "transparent");

    // A suited cell holding only some of its suits says which, in pips: the one
    // place a suit is unambiguous, since each combination of a suited hand is
    // one suit. They overlap, because four pips at a readable size do not fit a
    // cell side by side and the set is what is being read, not each pip.
    const suits = view.classSuits[index] ?? "";
    const holder = cell.querySelector<HTMLElement>(".cell-suits");
    if (suits === "") {
      holder?.remove();
    } else {
      const into = holder ?? document.createElement("span");
      into.className = "cell-suits";
      into.replaceChildren(
        ...[...suits].map((suit) => {
          const pip = document.createElement("i");
          pip.className = `pip suit-${suit}`;
          pip.textContent = SUIT_GLYPH[suit] ?? suit;
          return pip;
        }),
      );
      if (!holder) cell.append(into);
    }
  }
}

/** The suit breakdown of one cell, which follows the pointer. */
function createSuitPopup(): HTMLElement {
  const popup = document.createElement("div");
  popup.className = "suit-popup";
  popup.hidden = true;
  // Moving onto the popup must not count as leaving the cell, or it would close
  // itself the moment you reached for it.
  popup.addEventListener("pointerleave", () => {
    if (chrome.suitCell === null && chrome.suitPeek !== null) {
      chrome.suitPeek = null;
      repaint();
    }
  });
  return popup;
}

/** The weight being dragged across the popup, if any. */
let suitBrush: number | null = null;

/**
 * Draws the suit breakdown for one cell.
 *
 * The same cell, one dimension further in, and each little cell filled by
 * exactly the rules the big one uses - weight from the bottom, what the filters
 * left from the left, the group colour along the foot, the hover band from the
 * top - because it is the same fact at a finer grain and a second set of rules
 * to learn would defeat the point.
 *
 * A hand with two suits needs two axes: the higher card's suit down the side,
 * the lower card's across the top, which puts a pair in one triangle and an
 * offsuit hand everywhere off the diagonal. A suited hand has only one suit, so
 * it gets one row rather than a grid with twelve empty slots in it - and that
 * row is labelled by suit alone, because the suit is both cards'.
 */
function renderSuitPopup(popup: HTMLElement, cell: number | null, filtersOn: boolean): void {
  if (cell === null) {
    popup.hidden = true;
    popup.replaceChildren();
    return;
  }
  popup.hidden = false;
  popup.classList.toggle("pinned", chrome.suitCell === cell);

  // The same three region colours the matrix uses, so the breakdown of a gold
  // cell is gold: a colour that changed on the way in would read as a different
  // kind of thing rather than the same one, closer up.
  const [row, column] = [Math.floor(cell / 13), cell % 13];
  const suited = row < column;
  popup.style.setProperty(
    "--paint",
    `var(--paint-${row === column ? "pair" : suited ? "suited" : "offsuit"})`,
  );

  // A combination is named high card first, so its two suit letters are already
  // the row and the column. A pair names its two suits in a fixed order, so all
  // six of them land in one triangle and the diagonal stays empty - there is no
  // such hand as two queens of spades.
  const combos = cellCombos(cell);
  const byKey = new Map(combos.map((combo) => [`${combo.name[1]}${combo.name[3]}`, combo]));

  const label = classLabels[cell] ?? "";
  const pinned = chrome.suitCell === cell;

  const head = document.createElement("div");
  head.className = "suit-popup-head";
  const name = document.createElement("span");
  name.textContent = label;
  const hint = document.createElement("i");
  hint.className = "suit-popup-hint";
  hint.textContent = pinned
    ? "Esc or ✕ to close"
    : touchOnly()
      ? "hold a cell to pin"
      : "⇧-click to pin";
  head.append(name, hint);
  if (pinned) {
    // The same way out the combo editor has, because it is the same kind of
    // window: a click on the cross, a click anywhere outside, or Escape.
    const close = document.createElement("button");
    close.type = "button";
    close.className = "btn suit-close";
    close.textContent = "✕";
    close.title = "Close";
    close.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      event.stopPropagation();
      chrome.suitCell = null;
      chrome.suitPeek = null;
      repaint();
    });
    head.append(close);
  }

  const table = document.createElement("div");
  if (suited) {
    // One suit per combination, so one row - and the header is the bare pip.
    // Both cards wear that suit, which is what makes the hand suited, so
    // hanging a rank off it would name one of the two and quietly claim the
    // suit belongs to that one.
    table.className = "suit-grid one-row";
    for (const suit of SUITS) table.append(header(suit));
    for (const suit of SUITS) table.append(suitCell(byKey.get(`${suit}${suit}`), filtersOn));
  } else {
    // Two cards, two axes - and which axis is which card has to be written
    // down. A bare pip down the side and a bare pip across the top leaves the
    // reader to guess whether the row is the ace or the king, so each header
    // carries its own rank and the grid reads off as the hand it names.
    const [high, low] = [label[0], label[1]];
    table.className = "suit-grid";
    table.append(corner());
    for (const suit of SUITS) table.append(header(suit, low));
    for (const first of SUITS) {
      table.append(header(first, high));
      for (const second of SUITS) {
        table.append(suitCell(byKey.get(`${first}${second}`), filtersOn));
      }
    }
  }

  popup.replaceChildren(head, table);
}

/** The empty top-left slot, where the two axes meet. */
function corner(): HTMLElement {
  const element = document.createElement("span");
  element.className = "suit-head corner";
  return element;
}

/**
 * One axis label: a suit, and the rank that wears it where there is one.
 *
 * Two cards of different suits need saying which axis is which card. One suit
 * across both cards does not, and naming a card there would be worse than
 * saying nothing.
 */
function header(suit: string, rank?: string): HTMLElement {
  const element = document.createElement("span");
  element.className = `suit-head suit-${suit}`;
  element.textContent = `${rank ?? ""}${SUIT_GLYPH[suit] ?? suit}`;
  return element;
}

/**
 * One combination of the cell, or an empty slot where the class has none.
 *
 * An empty slot is drawn as nothing rather than as an empty cell: the gaps are
 * the shape of the hand, and a grid of sixteen boxes for a hand with six
 * combinations would invite counting the boxes.
 */
function suitCell(
  combo: ReturnType<typeof cellCombos>[number] | undefined,
  filtersOn: boolean,
): HTMLElement {
  const element = document.createElement("button");
  element.type = "button";
  element.className = "suit-cell";
  if (!combo) {
    element.classList.add("absent");
    element.disabled = true;
    return element;
  }
  element.dataset.combo = String(combo.index);
  element.dataset.name = combo.name;
  const held = combo.weight > 0;
  element.classList.toggle("on", held);
  element.classList.toggle("dealt", combo.dealt);
  // Both the glow and the colour are about hands in the range: one answers
  // "where is this hand of mine", the other "what did I decide about it". A
  // combination that is not in the range has neither answer, and lighting it
  // anyway would say the range holds something it does not.
  element.classList.toggle("lit", held && combo.matches);
  // The one combination being pointed at, when the pointing is happening in
  // another panel and this window is open on the cell it belongs to.
  element.classList.toggle("peek", chrome.peekCombo === combo.index);
  element.classList.toggle(
    "filtered",
    filtersOn && combo.weight > 0 && combo.passing < combo.weight - 1e-6,
  );
  element.style.setProperty("--fill", `${(combo.weight * 100).toFixed(1)}%`);
  element.style.setProperty(
    "--passing",
    `${(combo.weight > 0 ? (combo.passing / combo.weight) * 100 : 0).toFixed(1)}%`,
  );
  // A hand can be in more than one painted category - top pair and a flushdraw
  // at once - and it wears one colour, because one of them is what the filters
  // act on. The band says so and says the rest too: the colour that decides
  // comes first, the others follow it.
  const groups = held ? marksOf(combo.index, combo.colour) : [];
  element.classList.toggle("grouped", groups.length > 0);
  markColour(element, groups[0] ?? "none");
  if (groups.length > 1) element.style.setProperty("--groups", bands(groups));
  else element.style.removeProperty("--groups");
  element.title = combo.dealt
    ? `${combo.name} — the board or the dead cards have taken one of its cards`
    : `${combo.name} — ${(combo.weight * 100).toFixed(0)}%${
        held && combo.colour !== "none" ? `, ${combo.colour}` : ""
      }`;

  const paint = () => {
    const wanted = suitBrush ?? 0;
    mutate((engine) => engine.setComboWeight(combo.index, wanted));
  };
  press(element, {
    act: (event) => {
      event.stopPropagation();
      // The same rule the matrix uses: press a hand that is in to take it out.
      suitBrush = combo.weight > 0 ? 0 : chrome.brush;
      paint();
    },
  });
  element.addEventListener("pointerenter", () => {
    if (suitBrush !== null) paint();
    // One combination rather than the cell: pointing at the ace of spades with
    // the king of spades asks about that hand and no other.
    peekAt(chrome.suitCell ?? chrome.suitPeek, combo.index);
  });
  element.addEventListener("pointerleave", () => {
    if (chrome.peekCombo === combo.index) peekAt(chrome.suitCell ?? chrome.suitPeek);
  });
  return element;
}

/**
 * The colours of the painted categories one hand belongs to.
 *
 * The colour it actually wears first - that is the one the street filters act
 * on, and it has to stay findable - then any other painted category it is also
 * in. A category whose hands disagree carries no colour of its own, so it is
 * left out; what a hand in one of those wears is its own colour, which is
 * already first.
 */
function marksOf(combo: number, own: string): string[] {
  const marks = state().marks;
  const shares = comboStats(combo);
  const found = own === "none" ? [] : [own];
  for (const definition of statDefs) {
    if ((shares[definition.index] ?? 0) <= 0) continue;
    const mark = marks[definition.index];
    if (!mark || mark === "none" || mark === "mixed" || found.includes(mark)) continue;
    found.push(mark);
  }
  return found;
}

/** One band per colour, left to right. */
function bands(colours: string[]): string {
  const step = 100 / colours.length;
  const stops = colours.map(
    (colour, at) =>
      `var(--group-${colourSlot(colour)}) ${(at * step).toFixed(2)}% ${((at + 1) * step).toFixed(2)}%`,
  );
  return `linear-gradient(to right, ${stops.join(", ")})`;
}

/** The colour strip along a cell's foot, as a CSS gradient. */
function stripe(shares: number[]): string {
  const stops: string[] = [];
  let at = 0;
  for (let colour = 1; colour < shares.length; colour += 1) {
    const share = shares[colour] ?? 0;
    if (share <= 0) continue;
    const to = at + share * 100;
    stops.push(`var(--group-${colour}) ${at.toFixed(2)}% ${to.toFixed(2)}%`);
    at = to;
  }
  if (stops.length === 0) return "transparent";
  if (at < 100) stops.push(`transparent ${at.toFixed(2)}% 100%`);
  return `linear-gradient(to right, ${stops.join(", ")})`;
}

function formatCombos(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}
