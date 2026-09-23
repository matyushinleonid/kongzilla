/**
 * The strip across the top: one thumbnail per seat, the dead-card grid, and the
 * equity readout — the same three things Flopzilla puts above its main panels.
 */

import { chrome, equityByCombo, mutate, state, repaint } from "../store";
import { RANKS, SUITS, cardButton, seatName, thumbnail } from "./cards";

/** What each light is about, in the order the streets come. */
const STREETS = ["Flop", "Turn", "River"];

export function createTopStrip(): { element: HTMLElement; render: () => void } {
  const strip = document.createElement("div");
  strip.className = "topstrip";

  // ----- seats --------------------------------------------------------------
  const seats = document.createElement("div");
  seats.className = "seats";
  const thumbs: Array<ReturnType<typeof thumbnail>> = [];
  const seatCards: HTMLElement[] = [];

  // ----- dead cards ---------------------------------------------------------
  const deadPanel = document.createElement("div");
  deadPanel.className = "panel dead-panel";
  const deadHead = document.createElement("div");
  deadHead.className = "panel-head";
  const deadTitle = document.createElement("h2");
  deadTitle.className = "panel-title";
  deadTitle.textContent = "Dead cards";
  // Dealing a hand needs the same grid the dead cards use, so the way in sits
  // beside them - but it is its own button and its own mode. Clicking cards
  // without pressing it removes them from the deck, as it always has.
  const dealHand = document.createElement("button");
  dealHand.type = "button";
  dealHand.className = "btn deal-hand";
  dealHand.addEventListener("click", () => {
    chrome.dealing = chrome.dealing === null ? [] : null;
    repaint();
  });
  const deadClear = document.createElement("button");
  deadClear.type = "button";
  deadClear.className = "btn";
  deadClear.textContent = "Clear";
  deadClear.addEventListener("click", () => mutate((engine) => engine.setDead("")));
  deadHead.append(deadTitle, dealHand, deadClear);

  const deadGrid = document.createElement("div");
  deadGrid.className = "card-grid dead-grid";
  const deadCells = new Map<string, HTMLElement>();
  for (const suit of SUITS) {
    for (const rank of RANKS) {
      const card = `${rank}${suit}`;
      const cell = cardButton(card, (picked) => {
        // In dealing mode the grid is picking a hand, not thinning the deck.
        if (chrome.dealing === null) {
          mutate((engine) => engine.toggleDead(picked));
          return;
        }
        const picks = chrome.dealing.includes(picked)
          ? chrome.dealing.filter((one) => one !== picked)
          : [...chrome.dealing, picked];
        if (picks.length < 2) {
          chrome.dealing = picks;
          repaint();
          return;
        }
        chrome.dealing = null;
        mutate((engine) => engine.addHand(picks.join("")));
      });
      deadCells.set(card, cell);
      deadGrid.append(cell);
    }
  }
  deadPanel.append(deadHead, deadGrid);

  // ----- equity -------------------------------------------------------------
  const equityPanel = document.createElement("div");
  equityPanel.className = "panel equity-panel";

  strip.append(seats, deadPanel, equityPanel);

  const render = () => {
    const view = state();

    if (seatCards.length !== view.players.length) {
      seats.replaceChildren();
      seatCards.length = 0;
      thumbs.length = 0;
      view.players.forEach((_, index) => {
        const card = document.createElement("button");
        card.type = "button";
        card.className = "seat";
        const thumb = thumbnail();
        const badge = document.createElement("span");
        badge.className = "seat-badge";
        const lights = document.createElement("span");
        lights.className = "seat-streets";
        for (let street = 0; street < 3; street += 1) {
          const light = document.createElement("i");
          light.className = "street-light";
          lights.append(light);
        }
        const bar = document.createElement("span");
        bar.className = `seat-bar seat-${index}`;
        bar.style.setProperty("--seat", `var(--seat-${index})`);
        card.append(thumb.element, badge, lights, bar);
        card.classList.add("has-lights");
        card.addEventListener("click", () => mutate((engine) => engine.setActive(index)));
        // A cross, not a shift-click. Dropping a range is the one action here
        // that cannot be undone, and hiding it behind a modifier makes it both
        // impossible to find on purpose and possible to hit by accident.
        const drop = document.createElement("button");
        drop.type = "button";
        drop.className = "seat-drop";
        drop.textContent = "✕";
        drop.setAttribute("aria-label", "Remove this range");
        drop.addEventListener("click", (event) => {
          event.stopPropagation();
          mutate((engine) => engine.removeSeat(index));
        });
        // A copy of the range beside the one that drops it: the two are the
        // pair of things you do to a seat, and a reader trying a change wants
        // somewhere to try it that is not the range they already had.
        const copy = document.createElement("button");
        copy.type = "button";
        copy.className = "seat-copy";
        copy.textContent = "⧉";
        copy.setAttribute("aria-label", "Copy this range into a new one");
        copy.addEventListener("click", (event) => {
          event.stopPropagation();
          mutate((engine) => engine.duplicateSeat(index));
        });
        card.append(copy, drop);
        seats.append(card);
        seatCards.push(card);
        thumbs.push(thumb);
      });
      if (view.players.length < view.maxSeats) {
        const add = document.createElement("button");
        add.type = "button";
        add.className = "seat-add";
        add.textContent = "+";
        add.title = "Add a range to the pot \u2014 up to six";
        add.setAttribute("aria-label", add.title);
        add.addEventListener("click", () => mutate((engine) => engine.addSeat()));
        seats.append(add);
      }
    }

    view.players.forEach((player, index) => {
      const card = seatCards[index];
      card.classList.toggle("active", index === view.active);
      // A hand is a different kind of participant from a range, and looks it:
      // there is nothing to edit on it and its cards are out of the deck.
      card.classList.toggle("is-hand", player.hand !== null);
      const drop = card.querySelector<HTMLButtonElement>(".seat-drop");
      if (drop) drop.hidden = view.players.length <= 2;
      // No copy of a dealt hand - its cards are out of the deck, and a second
      // seat holding them would be the same two cards dealt twice - and none
      // when the table is full.
      const copy = card.querySelector<HTMLButtonElement>(".seat-copy");
      if (copy) copy.hidden = player.hand !== null || view.players.length >= 6;
      card.title = `${seatName(player)}: ${player.combos.toFixed(0)} combos, ${player.percent.toFixed(1)}%`;
      thumbs[index].paint(player.classWeights, player.classPassing);

      /*
       * The streets this seat has filtered, one light each.
       *
       * As many lights as there are streets to filter - one on a flop, three
       * by the river - so the row says how far the hand has got as well as how
       * far this range has been taken through it. Lit where the filter is on.
       */
      const lights = card.querySelector<HTMLElement>(".seat-streets")!;
      lights.hidden = view.streetsDealt === 0 || player.hand !== null;
      Array.from(lights.children).forEach((light, street) => {
        const there = street < view.streetsDealt;
        (light as HTMLElement).hidden = !there;
        const on = player.streets[street] === true;
        light.classList.toggle("on", on);
        (light as HTMLElement).title = `${STREETS[street]}: ${on ? "filtered" : "not filtered"}`;
      });
      const badge = card.querySelector(".seat-badge")!;
      const equity = seatEquity(index);
      badge.textContent = equity ? `${(equity.equity * 100).toFixed(3)}%` : seatName(player);
      badge.classList.toggle("muted-badge", !equity);
    });

    const dealing = chrome.dealing;
    dealHand.textContent = dealing === null ? "Deal a hand" : "Pick two cards…";
    dealHand.classList.toggle("active", dealing !== null);
    dealHand.title =
      dealing === null
        ? "Pick two cards as a hand somebody holds. Its cards leave the deck for everyone else."
        : "Pick two cards, or press again to stop.";
    dealHand.disabled = dealing === null && view.players.length >= view.maxSeats;
    deadPanel.classList.toggle("dealing", dealing !== null);

    const used = new Set(view.dead);
    const onBoard = new Set(view.boardCards);
    const dealt = new Set(view.dealt);
    const picking = new Set(dealing ?? []);
    deadCells.forEach((cell, card) => {
      cell.classList.toggle("picked", used.has(card));
      // A card somebody has been dealt is gone for everyone, the same way a
      // board card is - and it says which hand has it.
      cell.classList.toggle("taken", onBoard.has(card) || dealt.has(card));
      cell.classList.toggle("dealt", dealt.has(card));
      cell.classList.toggle("picking", picking.has(card));
    });

    equityPanel.replaceChildren(...equityContent());
  };

  return { element: strip, render };
}

/**
 * The equity a seat's badge should show.
 *
 * The report has no names in it: it lists the seats that are in the pot, in
 * seat order, and a seat its filters have emptied is not one of them.
 */
function seatEquity(index: number) {
  const view = state();
  if (!view.equity) return null;
  const at = view.equitySeats.indexOf(index);
  return at < 0 ? null : (view.equity.players[at] ?? null);
}

function equityContent(): Node[] {
  const view = state();
  if (!view.equity) {
    const hint = document.createElement("p");
    hint.className = "hint";
    // Two ranges, and a hand is a range with one combination in it - so this
    // says the one thing that is missing rather than offering two ways in.
    hint.textContent = "Fill two ranges to measure them against each other.";
    return [hint];
  }

  const rows: Node[] = [];
  const head = document.createElement("div");
  head.className = "row equity-head";
  const who = document.createElement("p");
  who.className = "equity-who";
  head.append(who);

  const multiway = view.equitySeats.length > 2;
  if (!multiway) {
    // Two players: the whole of it - who wins, who splits - and about the seat
    // the reader has open, which is the one they are asking about. A seat its
    // own filters have emptied is not in the pot, so it is not one of the two
    // being named either: then this is about the pot as it stands.
    const at = view.equitySeats.indexOf(view.active);
    const mine = view.equity.players[Math.max(at, 0)];
    const order =
      at < 0
        ? [...view.equitySeats]
        : [view.active, ...view.equitySeats.filter((seat) => seat !== view.active)];
    rows.push(
      equityLine("Equity", mine.equity, "strong"),
      equityLine("Win", mine.win),
      equityLine("Tie", mine.tie),
    );
    who.textContent = order.map((seat) => seatName(view.players[seat])).join(" vs ");
  } else {
    // Three or more: one line each, because the question is now who has the
    // best of it rather than how one hand splits with one other. The seat the
    // reader has open goes first, whatever order the pot is in.
    who.textContent = `${view.equitySeats.length}-way`;
    const order = [...view.equitySeats].sort(
      (a, b) => Number(b === view.active) - Number(a === view.active),
    );
    for (const seat of order) {
      const player = view.equity.players[view.equitySeats.indexOf(seat)];
      if (!player) continue;
      rows.push(
        equityLine(
          seatName(view.players[seat]),
          player.equity,
          seat === view.active ? "strong" : "",
          seat,
        ),
      );
    }
  }

  // Multiway there is a second question - how this range does against one of
  // them rather than against all of them - and the reader has already said
  // which one over in the output panel. So it is answered here when there is
  // an answer, as a line rather than as a second way of reading the panel: a
  // button that swapped the four lines for one would be a button that hid
  // what the reader came here for.
  const named = multiway && view.versusSeat !== null ? view.versusSeat : null;
  const pair = named === null ? null : headsUp();
  if (pair && named !== null) {
    const line = equityLine(
      `vs ${seatName(view.players[named])}`,
      pair.equity,
      "equity-pair",
      named,
    );
    line.title = `${seatName(view.players[view.active])} against ${seatName(view.players[named])} alone, rather than against the whole pot. Which range that is is chosen in the output panel.`;
    rows.push(line);
  }

  const note = document.createElement("p");
  note.className = "hint";
  note.textContent = view.equity.exact
    ? `exact · ${view.equity.trials.toLocaleString()} run-outs`
    : `sampled · ${view.equity.trials.toLocaleString()} trials`;

  return [head, ...rows, note];
}

/** The active range against the one seat it is measured against. */
function headsUp(): { equity: number; win: number; tie: number } | null {
  const data = equityByCombo();
  if (!data) return null;
  let held = 0;
  const sum = { equity: 0, win: 0, tie: 0 };
  for (let combo = 0; combo < data.equity.length; combo += 1) {
    const weight = data.weight[combo];
    if (weight <= 0 || data.equity[combo] < 0) continue;
    held += weight;
    sum.equity += weight * data.equity[combo];
    sum.win += weight * data.win[combo];
    sum.tie += weight * data.tie[combo];
  }
  if (held <= 0) return null;
  return { equity: sum.equity / held, win: sum.win / held, tie: sum.tie / held };
}

function equityLine(label: string, value: number, extra = "", seat?: number): HTMLElement {
  const line = document.createElement("p");
  line.className = `equity-line ${extra}`;
  if (seat !== undefined) {
    line.classList.add("equity-seat");
    line.style.setProperty("--seat", `var(--seat-${seat})`);
  }
  const name = document.createElement("span");
  name.textContent = `${label}:`;
  const amount = document.createElement("span");
  amount.className = "num";
  amount.textContent = `${(value * 100).toFixed(3)}%`;
  line.append(name, amount);
  return line;
}
