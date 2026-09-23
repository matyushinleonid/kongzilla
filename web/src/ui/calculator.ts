/**
 * The price of a call, under the views that work out whether it is worth it.
 *
 * A panel of its own rather than a footer to the output panel: what a call is
 * worth is a question about the spot, not about whichever way of looking at it
 * happens to be on screen, and a block that changed with the tab above it
 * would read as part of that tab.
 *
 * Three numbers, and they answer one question between them. Pot odds say what
 * a call needs to be worth making; MDF says how much of a range has to carry
 * on for the bet not to be free money; EV says what calling is worth with the
 * equity this range actually has. The first two are arithmetic a reader could
 * do in their head and mostly does not; the third is the one that needs the
 * rest of the app, because "the equity this range actually has" is the whole
 * thing the app is for.
 *
 * So the equity is not typed in. It is the active range against whichever
 * other range the reader names - the same choice the equity views are drawn
 * against, and the same button to change it, because two settings that mean
 * the same thing is one setting too many.
 *
 * And it is about whatever the reader is looking at. A range facing a bet is
 * not one decision but a hundred, so the whole range's equity answers a
 * question nobody has: point at a hand and the line is about that hand, point
 * at a matrix cell and it is about that cell, point at a rung of the ladder
 * and it is about the hands on it. Let go and it is the range again.
 *
 * Two numbers to give it: the pot as it stands with their bet already in it,
 * and the bet itself. That is what a reader looking at a table can read off
 * the screen without arithmetic, so it is what the boxes ask for.
 *
 * Everything follows from the pair:
 *
 *     pot odds = bet / (pot + bet)         what a call has to be worth
 *     MDF      = (pot − bet) / pot         what has to carry on
 *     EV(call) = eq·pot − (1−eq)·bet
 *
 * Each box is what the reader typed into it and nothing else. The two do
 * constrain each other - a bet is standing in the pot, so it cannot be bigger
 * than it - but that is said rather than fixed: a box that quietly moved the
 * other one is a box that argues with whoever is typing in it.
 *
 * The size buttons are the one place both move, because that is what they are
 * for: "they bet half the pot" is a sizing being built out of what was in the
 * middle before it, and the middle grows by what went into it.
 */

import {
  chrome,
  classLabels,
  cycleVersusSeat,
  equityByCombo,
  preflopEquityReady,
  repaint,
  revision,
  state,
  statCombos,
  statDefs,
} from "../store";
import { classCombos, comboName, pipText, seatName, seatTag } from "./cards";

/** What the EV line is about, when it has nothing else to say. */
const EV_HINT = "What calling is worth, at this range's equity.";

/** The bets worth a button, as a share of the pot. */
const SIZES: Array<[label: string, share: number]> = [
  ["⅓", 1 / 3],
  ["½", 0.5],
  ["⅔", 2 / 3],
  ["pot", 1],
];

export function createCalculator(): { element: HTMLElement; render: () => void } {
  const calc = document.createElement("section");
  calc.className = "panel panel-calc";

  const head = document.createElement("div");
  head.className = "panel-head";
  const title = document.createElement("h2");
  title.className = "panel-title";
  title.textContent = "Calculator";

  const top = document.createElement("div");
  top.className = "row calc-row";
  const potField = field("Pot", "calc-pot", "What is in the middle, their bet included");
  const betField = field("Bet", "calc-bet", "The bet this range is facing");
  top.append(potField.label, betField.label);

  const below = document.createElement("div");
  below.className = "row calc-row";
  const sizes = document.createElement("div");
  sizes.className = "calc-sizes";
  for (const [label, share] of SIZES) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "btn calc-size";
    button.textContent = label;
    button.dataset.share = String(share);
    button.title = `A bet of ${label === "pot" ? "the pot" : `${label} of the pot`}`;
    button.addEventListener("click", () => {
      // A share of what was in the middle before the bet, which is what a bet
      // is named after - and the pot moves by the difference, because that is
      // what betting more does to it.
      const before = Math.max(0, chrome.pot - chrome.bet);
      const bet = Math.round(before * share * 100) / 100;
      chrome.pot = before + bet;
      chrome.bet = bet;
      repaint();
    });
    sizes.append(button);
  }
  const versus = document.createElement("button");
  versus.type = "button";
  versus.className = "btn versus-button versus-calc";
  versus.addEventListener("click", cycleVersusSeat);
  head.append(title, versus);
  below.append(sizes);

  const rows = document.createElement("div");
  rows.className = "calc-rows";
  const odds = readout(rows, "Pot odds", "");
  const mdf = readout(
    rows,
    "MDF",
    "Minimum defence frequency: carry on with less than this and a bet of any two cards shows a profit.",
  );
  const ev = readout(rows, "EV call", EV_HINT);
  calc.append(head, top, below, rows);

  /** What a box was typed into, kept as the pot and the bet whichever it was. */
  const took = (input: HTMLInputElement, use: (value: number) => void) => {
    input.addEventListener("input", () => {
      const value = Number(input.value);
      use(Number.isFinite(value) && value >= 0 ? value : 0);
      repaint();
    });
  };
  took(potField.input, (value) => (chrome.pot = value));
  took(betField.input, (value) => (chrome.bet = value));

  // The equity is a pass over every combination against every combination, so
  // the numbers are asked for when the session or the opponent moves and not
  // when the pointer does. What the pointer moves is which of them are read.
  let equityAt = "";
  let equity: number | null = null;
  let held: Equities | null = null;
  // The hands on a rung come back as a list, which is a parse: worth doing
  // when the pointer arrives on the rung, not on every redraw while it sits
  // there.
  let rungAt = "";
  let rung: number[] = [];

  const render = () => {
    const view = state();
    for (const [input, value] of [
      [potField.input, chrome.pot],
      [betField.input, chrome.bet],
    ] as const) {
      if (document.activeElement !== input) input.value = String(value);
    }

    const others = view.players.map((_, index) => index).filter((index) => index !== view.active);
    versus.hidden = others.length === 0;
    const against = view.versusSeat === null ? null : view.players[view.versusSeat];
    const facing = against ? seatName(against) : null;
    versus.textContent = against ? `vs ${seatTag(against)}` : "vs all";
    versus.classList.toggle("active", facing !== null);
    versus.style.setProperty("--seat", `var(--seat-${view.versusSeat ?? 0})`);
    versus.title = facing
      ? `Calling against ${facing}. Press for the next range.`
      : "Calling against every other range at once. Press to pick one instead.";

    const { pot, bet } = chrome;
    // A bet bigger than the pot it is standing in is not a spot, it is a
    // half-typed number, and answering it with arithmetic would be answering
    // a question nobody asked.
    const priced = pot > 0 && bet >= 0 && bet <= pot;
    const over = bet > pot;
    // The two numbers the percentage is the ratio of, written as one.
    //
    // It used to say what the call wins - "calling 22.20 to win 60.20" - which
    // is true and is the wrong pair of numbers to have under a percentage:
    // 22.20 of 60.20 is 37%, and the answer beside it said 27%. Equity is a
    // share of the pot the call *makes*, so that is the pot to name.
    const final = pot + bet;
    odds.value.textContent = priced ? `${((bet / final) * 100).toFixed(1)}%` : "—";
    odds.note.textContent = over
      ? "the bet is bigger than the pot"
      : priced
        ? `${format(bet)} / ${format(final)} after the call`
        : "";
    odds.row.title = priced
      ? `The equity a call needs to break even: ${format(bet)} as a share of the ${format(final)} pot the call makes — ${bet > 0 ? `${(pot / bet).toFixed(1)} to 1` : "nothing to call"}.`
      : "The equity a call needs to break even.";

    mdf.value.textContent = priced ? `${(((pot - bet) / pot) * 100).toFixed(1)}%` : "—";
    // What the reader has actually marked as carrying on, which is the number
    // MDF is a verdict on. Only once they have marked something: a range with
    // no paint on it is not a range that folds everything, it is a range
    // nobody has decided about yet.
    const total = view.groupShares.reduce((sum, share) => sum + share, 0);
    const painted = total - (view.groupShares[0] ?? 0);
    mdf.note.textContent =
      priced && painted > 0 ? `painted ${((painted / total) * 100).toFixed(1)}%` : "";
    mdf.row.classList.toggle(
      "under",
      priced && painted > 0 && painted / total < (pot - bet) / pot - 1e-9,
    );

    // The pass over the flops changes nothing about the session, so the
    // revision does not move with it - and watching the revision alone meant
    // the calculator went on showing the "no equity yet" it had worked out
    // before the pass, on any seat the reader did not leave and come back to.
    const asked = `${revision()}/${view.versusSeat ?? "all"}/${preflopEquityReady()}`;
    if (asked !== equityAt) {
      equityAt = asked;
      held = equityByCombo();
      equity = rangeEquity(held);
    }

    // What the pointer is on, in the order the reader would expect: one
    // combination beats the cell it is in, and the cell beats the rung.
    let about = "";
    let mine = equity;
    if (held) {
      const rungOf = chrome.hovered;
      if (rungOf !== null && rungOf !== undefined) {
        const key = `${asked}/${rungOf}`;
        if (key !== rungAt) {
          rungAt = key;
          rung = statCombos(rungOf).map(([combo]) => combo);
        }
      }
      const on: [string, number[]] | null =
        chrome.peekCombo !== null
          ? [pipText(comboName(chrome.peekCombo)), [chrome.peekCombo]]
          : chrome.peekClass !== null
            ? [classLabels[chrome.peekClass] ?? "", classCombos(chrome.peekClass)]
            : rungOf !== null && rungOf !== undefined
              ? [statDefs.find((def) => def.index === rungOf)?.label ?? "", rung]
              : null;
      // A hand the range does not hold has no equity of its own, and inventing
      // one for it would be worse than going on saying what the range is worth.
      const some = on ? meanOver(held, on[1]) : null;
      if (on && some !== null) {
        about = `${on[0]} `;
        mine = some;
      }
    }
    if (!priced || mine === null) {
      ev.value.textContent = "—";
      // Three ways of having nothing to say, and each says what would fix it.
      // A spot the other two lines have refused is not one this line should
      // answer: a bet that will not fit in the pot has no price, so it has no
      // value either. Otherwise it is the equity that is missing - either
      // there is nobody to measure against, or there is somebody named out of
      // a table of three and, before the flop, that answer only comes from a
      // pass over the flops.
      const named = view.versusSeat !== null && view.players.length > 2;
      const waiting = named && view.board === "";
      ev.note.textContent = !priced
        ? over
          ? "the bet is bigger than the pot"
          : ""
        : waiting
          ? "press Calculate in the statistics panel"
          : "needs a range to measure against";
      ev.row.title = waiting
        ? `Measuring against ${facing ?? "one range"} rather than against the whole table needs every hand's equity, and before the flop that comes from the pass over the flops — the Calculate button in the statistics panel.`
        : EV_HINT;
      ev.row.classList.remove("good", "bad");
      return;
    }
    ev.row.title = EV_HINT;
    const worth = mine * pot - (1 - mine) * bet;
    ev.value.textContent = `${worth >= 0 ? "+" : "−"}${format(Math.abs(worth))}`;
    // Which equity, and over what. The board is on screen when there is one;
    // when there is not, the number is an average over every flop still to
    // come, and saying so is the difference between a stale reading and an
    // answer to a different question.
    ev.note.textContent =
      `${about}at ${(mine * 100).toFixed(2)}% ` +
      (facing ? `vs ${seatTag(against!)}` : "against the field") +
      (view.board === "" ? ", before the flop" : "");
    ev.row.classList.toggle("good", worth > 0);
    ev.row.classList.toggle("bad", worth < 0);
  };

  return { element: calc, render };
}

/**
 * The range's equity against whoever it is being measured against.
 *
 * Two ways of knowing it, and the better one is not always to hand. Hand by
 * hand is exact and answers about any named seat, but before the flop it comes
 * from the pass over the flops, which is a second of work nobody asked for
 * here. The pot's own report is worked out whenever the seats are, on any
 * street - it is the number already on the seat's tile - and it answers about
 * the whole pot rather than about one seat in it. So it stands in wherever
 * that is the same question: against everybody, or at a table of two, where
 * everybody is the one other range.
 */
function rangeEquity(held: Equities | null): number | null {
  return (held && meanOver(held, null)) ?? reportedEquity();
}

/** What the pot's own equity report says the active seat is worth. */
function reportedEquity(): number | null {
  const view = state();
  if (!view.equity) return null;
  // With three ranges in the pot, "against B" is not a question the report
  // answers: it measures each seat against everyone else at once.
  if (view.versusSeat !== null && view.players.length > 2) return null;
  const at = view.equitySeats.indexOf(view.active);
  return at < 0 ? null : (view.equity.players[at]?.equity ?? null);
}

/** Equity and weight per combination, as the engine hands them over. */
type Equities = NonNullable<ReturnType<typeof equityByCombo>>;

/**
 * What some part of the range is worth, hand by hand - exact, and about a
 * named seat.
 *
 * `null` for the whole range; a list of combinations for a part of it. Either
 * way it is the mean weighted by how much of each hand the range holds, so a
 * half-weighted hand counts half.
 */
function meanOver(data: Equities, only: number[] | null): number | null {
  let held = 0;
  let sum = 0;
  for (const combo of only ?? data.equity.keys()) {
    const weight = data.weight[combo] ?? 0;
    if (weight <= 0 || (data.equity[combo] ?? -1) < 0) continue;
    held += weight;
    sum += weight * data.equity[combo];
  }
  return held > 0 ? sum / held : null;
}

/**
 * A number the reader types in, with its name beside it.
 *
 * Text rather than a number box, and the keys sorted out here instead. A
 * number box takes what the keyboard sends and refuses what it does not
 * understand, which on a Russian layout means the full stop - it is where "ю"
 * is - vanishes: the key goes down, nothing appears, and the reader is left
 * pressing it again. Here every key that is not a digit is read as the point
 * the reader was reaching for, whichever letter the layout put on it.
 */
function field(name: string, className: string, hint: string) {
  const label = document.createElement("label");
  label.className = "calc-field";
  label.title = hint;
  const text = document.createElement("span");
  text.className = "field-label";
  text.textContent = name;
  const input = document.createElement("input");
  input.type = "text";
  input.inputMode = "decimal";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.className = `num ${className}`;
  input.setAttribute("aria-label", hint);

  input.addEventListener("keydown", (event) => {
    // Anything the keyboard is doing rather than typing - a shortcut, an
    // arrow, a delete - is left alone.
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.key.length !== 1 || /[0-9]/.test(event.key)) return;
    event.preventDefault();
    point(input);
  });
  // A paste, or anything else that got in another way, keeps the digits and a
  // single point out of whatever arrived.
  input.addEventListener("input", () => {
    const clean = tidy(input.value);
    if (clean === input.value) return;
    const at = Math.max(
      0,
      (input.selectionStart ?? clean.length) - (input.value.length - clean.length),
    );
    input.value = clean;
    input.setSelectionRange(at, at);
  });

  label.append(text, input);
  return { label, input };
}

/** Types a decimal point at the caret, where there is not one already. */
function point(input: HTMLInputElement): void {
  const from = input.selectionStart ?? input.value.length;
  const to = input.selectionEnd ?? from;
  const left = input.value.slice(0, from);
  const right = input.value.slice(to);
  if (`${left}${right}`.includes(".")) return;
  input.value = `${left}.${right}`;
  input.setSelectionRange(from + 1, from + 1);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

/**
 * Digits and at most one point, in the order they arrived.
 *
 * A comma is a decimal point in half the world and a thousands separator in
 * the other half, and which one it is here is readable from the company it
 * keeps: `1 200,5` has no point of its own so the comma is one, while
 * `1,200.5` already has its point and the commas are spacing.
 */
function tidy(text: string): string {
  const kept = text.includes(".") ? text.replace(/,/g, "") : text.replace(/,/g, ".");
  const digits = kept.replace(/[^0-9.]/g, "");
  const at = digits.indexOf(".");
  return at < 0 ? digits : `${digits.slice(0, at + 1)}${digits.slice(at + 1).replace(/\./g, "")}`;
}

/** A number the app works out, with its name and what it is about. */
function readout(into: HTMLElement, name: string, hint: string) {
  const row = document.createElement("div");
  row.className = "calc-readout";
  row.title = hint;
  const label = document.createElement("span");
  label.className = "field-label";
  label.textContent = name;
  const value = document.createElement("span");
  value.className = "calc-value num";
  const note = document.createElement("span");
  note.className = "calc-note";
  row.append(label, value, note);
  into.append(row);
  return { row, value, note };
}

/** Chips, to as many places as they are actually in. */
function format(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(2);
}
