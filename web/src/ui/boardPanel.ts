/**
 * The board panel.
 *
 * Flopzilla stacks the dealt cards above a tall picker with ranks down the rows
 * and suits across the columns, and puts the filter tally underneath. Same here.
 */

import { track } from "../analytics";
import { chrome, mutate, state } from "../store";
import { RANKS, SUITS, cardButton, cardSlot, die } from "./cards";

export function createBoardPanel(): {
  element: HTMLElement;
  render: () => void;
  randomBoard: () => void;
  stepStreet: (delta: number) => void;
} {
  const panel = document.createElement("section");
  panel.className = "panel panel-board";

  const head = document.createElement("div");
  head.className = "panel-head";
  const back = arrow("◀", "Show one card less");
  const title = document.createElement("h2");
  title.className = "panel-title street-name";
  const forward = arrow("▶", "Show one card more");
  head.append(back, title, forward);

  const slots = document.createElement("div");
  slots.className = "board-slots";

  const grid = document.createElement("div");
  grid.className = "card-grid board-grid";
  const cells = new Map<string, HTMLElement>();
  for (const rank of RANKS) {
    for (const suit of SUITS) {
      const card = `${rank}${suit}`;
      const cell = cardButton(card, pick);
      cells.set(card, cell);
      grid.append(cell);
    }
  }

  const actions = document.createElement("div");
  actions.className = "row board-actions";
  const clear = document.createElement("button");
  clear.type = "button";
  clear.className = "btn";
  clear.textContent = "Clear";
  clear.title = "Clear the board (Backspace)";
  clear.addEventListener("click", () => {
    chrome.boardCards = [];
    chrome.visible = 0;
    syncBoard();
    track("board_cleared");
  });
  const random = document.createElement("button");
  random.type = "button";
  random.className = "btn";
  random.append(die(), document.createTextNode("Random"));
  random.title = "Deal a random flop (R)";
  random.title = "Deal a random flop";
  const randomBoard = () => {
    chrome.boardCards = randomCards(3, new Set(state().dead));
    chrome.visible = 3;
    syncBoard();
  };
  random.addEventListener("click", randomBoard);

  // One street back or forward, which is what the arrows either side of the
  // street name do and what the arrow keys do.
  const stepStreet = (delta: number) => {
    const wanted = chrome.visible + delta;
    if (wanted < 0 || wanted > chrome.boardCards.length) return;
    chrome.visible = wanted;
    syncBoard();
  };

  back.addEventListener("click", () => stepStreet(-1));
  forward.addEventListener("click", () => stepStreet(1));
  actions.append(clear, random);

  const tally = document.createElement("p");
  tally.className = "tally";

  panel.append(head, slots, grid, actions, tally);

  function syncBoard(): void {
    const cards = chrome.boardCards.slice(0, chrome.visible);
    mutate((engine) => engine.setBoard(cards.join(" ")));
  }

  function pick(card: string): void {
    if (chrome.boardCards.includes(card)) {
      remove(card);
      return;
    }
    if (state().dead.includes(card) || chrome.boardCards.length >= 5) return;
    chrome.boardCards.push(card);
    chrome.visible = chrome.boardCards.length;
    syncBoard();
  }

  function remove(card: string): void {
    const index = chrome.boardCards.indexOf(card);
    if (index < 0) return;
    chrome.boardCards.splice(index, 1);
    chrome.visible = Math.min(chrome.visible, chrome.boardCards.length);
    syncBoard();
  }

  const render = () => {
    const view = state();

    title.textContent = view.street;
    back.disabled = chrome.visible === 0;
    forward.disabled = chrome.visible >= chrome.boardCards.length;

    slots.replaceChildren(
      ...Array.from({ length: 5 }, (_, index) => {
        const card = chrome.boardCards[index];
        const slot = cardSlot(card);
        if (card) {
          slot.classList.toggle("hidden-card", index >= chrome.visible);
          slot.title = `Remove ${card}`;
          slot.addEventListener("click", () => remove(card));
          if (index === 3) slot.dataset.street = "T";
          if (index === 4) slot.dataset.street = "R";
        }
        return slot;
      }),
    );

    const onBoard = new Set(chrome.boardCards);
    const dead = new Set(view.dead);
    cells.forEach((cell, card) => {
      cell.classList.toggle("picked", onBoard.has(card));
      cell.classList.toggle("taken", dead.has(card));
    });

    // The denominator is what the board and the dead cards have left, which is
    // what every percentage in the statistics panel is measured against.
    const passing = view.liveCombos * view.passFraction;
    const streets = view.streetsOn.filter(Boolean).length;
    tally.textContent = view.board
      ? view.filtersEnabled
        ? `${combos(passing)} out of ${combos(view.liveCombos)} combos left after ${streets} street filter${streets === 1 ? "" : "s"}.`
        : `${combos(view.liveCombos)} combos, none filtered out yet.`
      : "Pick a flop to see how the range hits it.";
  };

  return { element: panel, render, randomBoard, stepStreet };
}

/**
 * A combination count, the way every other panel prints one.
 *
 * Rounding to a whole number here while the statistics panel printed a tenth
 * had the two disagreeing about the same range: a mixed strategy is genuinely
 * seven and a half combinations, and saying eight is wrong twice over.
 */
function combos(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

function randomCards(count: number, exclude: Set<string>): string[] {
  const deck: string[] = [];
  for (const rank of RANKS) {
    for (const suit of SUITS) {
      const card = `${rank}${suit}`;
      if (!exclude.has(card)) deck.push(card);
    }
  }
  for (let i = deck.length - 1; i > 0; i -= 1) {
    const j = Math.floor(Math.random() * (i + 1));
    [deck[i], deck[j]] = [deck[j], deck[i]];
  }
  return deck.slice(0, count);
}

function arrow(label: string, title: string): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "btn street-arrow";
  button.textContent = label;
  button.title = title;
  return button;
}
