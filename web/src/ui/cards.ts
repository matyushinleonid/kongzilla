/**
 * Card widgets.
 *
 * Flopzilla uses a four-colour deck and tints the whole cell, not just the glyph,
 * so a board or a dead-card grid reads at a glance. Everything that draws a card
 * goes through here so the two grids and the board slots stay identical.
 */

export const RANKS = ["A", "K", "Q", "J", "T", "9", "8", "7", "6", "5", "4", "3", "2"] as const;
export const SUITS = ["h", "c", "d", "s"] as const;
export const SUIT_GLYPH: Record<string, string> = { s: "♠", h: "♥", d: "♦", c: "♣" };

/**
 * A hand or a board with its suits as pips, into an element.
 *
 * `Ts Th` is two letters doing a symbol's job: everywhere else on screen a suit
 * is a coloured pip, and reading one notation in the matrix and another in a
 * table is a small tax on every glance. The text notation keeps its letters,
 * because that is what gets pasted and parsed.
 */
export function pips(text: string, into?: HTMLElement): HTMLElement {
  const element = into ?? document.createElement("span");
  element.replaceChildren();
  for (const card of text.trim().split(/\s+/)) {
    if (element.childNodes.length > 0) element.append(document.createTextNode(" "));
    // Cards run two characters each, so a combination is one word of four.
    for (let at = 0; at + 1 < card.length; at += 2) {
      const glyph = SUIT_GLYPH[card[at + 1]];
      element.append(document.createTextNode(card[at]));
      if (!glyph) {
        element.append(document.createTextNode(card[at + 1]));
        continue;
      }
      const pip = document.createElement("i");
      pip.className = `pip suit-${card[at + 1]}`;
      pip.textContent = glyph;
      element.append(pip);
    }
  }
  return element;
}

/** The same, as a string, for a tooltip or a label that cannot hold elements. */
export function pipText(text: string): string {
  return text.replace(
    /([2-9TJQKA])([cdhs])/g,
    (_match, rank, suit) => `${rank}${SUIT_GLYPH[suit]}`,
  );
}

/**
 * A die, for the buttons that roll one.
 *
 * Drawn rather than set as a character: the dice glyphs live in a font block
 * plenty of systems do not have, and a missing one renders as an empty box.
 */
export function die(): SVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("class", "die");
  svg.setAttribute("aria-hidden", "true");
  const body = document.createElementNS(ns, "rect");
  body.setAttribute("x", "1.5");
  body.setAttribute("y", "1.5");
  body.setAttribute("width", "13");
  body.setAttribute("height", "13");
  body.setAttribute("rx", "3");
  body.setAttribute("class", "die-body");
  svg.append(body);
  for (const [x, y] of [
    [5, 5],
    [11, 5],
    [8, 8],
    [5, 11],
    [11, 11],
  ]) {
    const pip = document.createElementNS(ns, "circle");
    pip.setAttribute("cx", String(x));
    pip.setAttribute("cy", String(y));
    pip.setAttribute("r", "1.4");
    pip.setAttribute("class", "die-pip");
    svg.append(pip);
  }
  return svg;
}

/** A pickable card in one of the 4x13 grids. */
export function cardButton(card: string, onPick: (card: string) => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = `card-cell suit-${card[1]}`;
  button.dataset.card = card;
  button.title = card;
  button.append(rankGlyph(card));
  button.addEventListener("click", () => onPick(card));
  return button;
}

/** A filled or empty board slot. */
export function cardSlot(card: string | undefined): HTMLButtonElement {
  const slot = document.createElement("button");
  slot.type = "button";
  slot.className = "board-slot";
  if (!card) {
    slot.classList.add("empty");
    slot.disabled = true;
    return slot;
  }
  slot.classList.add(`suit-${card[1]}`);
  slot.dataset.card = card;
  slot.append(rankGlyph(card));
  return slot;
}

function rankGlyph(card: string): DocumentFragment {
  const fragment = document.createDocumentFragment();
  fragment.append(document.createTextNode(card[0]));
  const suit = document.createElement("i");
  suit.textContent = SUIT_GLYPH[card[1]] ?? card[1];
  fragment.append(suit);
  return fragment;
}

/** A miniature 13x13 matrix, used for the seat thumbnails. */
export function thumbnail(): { element: HTMLElement; paint: (weights: number[]) => void } {
  const element = document.createElement("div");
  element.className = "thumb-matrix";
  const cells: HTMLElement[] = [];
  for (let index = 0; index < 169; index += 1) {
    const cell = document.createElement("i");
    cells.push(cell);
    element.append(cell);
  }
  const paint = (weights: number[]) => {
    cells.forEach((cell, index) => {
      const weight = weights[index] ?? 0;
      cell.classList.toggle("on", weight > 0);
      cell.style.setProperty("--weight", weight.toFixed(3));
    });
  };
  return { element, paint };
}

const RANK_CHARS = "23456789TJQKA";
const SUIT_CHARS = "cdhs";

/** Maps every combo index to its two card indices, mirroring the engine. */
const COMBO_CARDS: Array<[number, number]> = (() => {
  const table: Array<[number, number]> = new Array(1326);
  for (let high = 1; high < 52; high += 1) {
    for (let low = 0; low < high; low += 1) {
      table[(high * (high - 1)) / 2 + low] = [low, high];
    }
  }
  return table;
})();

function cardName(index: number): string {
  return RANK_CHARS[index >> 2] + SUIT_CHARS[index & 3];
}

/**
 * Which matrix cell a combo belongs to, mirroring the engine's layout.
 *
 * Beside the table that names a combo because it is the same arithmetic: both
 * take an index apart into two cards, and two copies of that are two chances
 * to get it wrong.
 */
const COMBO_CLASS: number[] = (() => {
  const table = new Array<number>(1326);
  for (let index = 0; index < 1326; index += 1) {
    const [low, high] = COMBO_CARDS[index];
    const suited = (high & 3) === (low & 3);
    const hi = Math.max(high >> 2, low >> 2);
    const lo = Math.min(high >> 2, low >> 2);
    const hiCell = 12 - hi;
    const loCell = 12 - lo;
    table[index] = suited && hi !== lo ? hiCell * 13 + loCell : loCell * 13 + hiCell;
  }
  return table;
})();

/** The matrix cell one combination sits in. */
export function comboClass(combo: number): number {
  return COMBO_CLASS[combo] ?? 0;
}

/** Every combination that sits in one matrix cell. */
export function classCombos(klass: number): number[] {
  const out: number[] = [];
  for (let combo = 0; combo < COMBO_CLASS.length; combo += 1) {
    if (COMBO_CLASS[combo] === klass) out.push(combo);
  }
  return out;
}

/** A combo's name, high card first: `AhKh`. */
export function comboName(index: number): string {
  const pair = COMBO_CARDS[index];
  if (!pair) return "";
  const [low, high] = pair;
  const [first, second] = low >> 2 > high >> 2 ? [low, high] : [high, low];
  return cardName(first) + cardName(second);
}

/**
 * What to call a seat.
 *
 * A seat holding one hand is that hand, and is named as it everywhere: "A♠K♠"
 * is what the reader put there, and "Range B" is a label that hides it. One
 * place decides, so the strip, the readout, the legend and the two comparison
 * controls cannot end up calling the same seat different things.
 */
export function seatName(player: { name: string; hand: string | null }): string {
  return player.hand ? pipText(player.hand) : player.name;
}

/** The same, short enough for a button: "A♠K♠" or "B" rather than "Range B". */
export function seatTag(player: { name: string; hand: string | null }): string {
  return player.hand ? pipText(player.hand) : player.name.replace("Range ", "");
}
