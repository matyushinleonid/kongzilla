/**
 * The keyboard.
 *
 * Flopzilla's own keys where they map onto something we have - Tab for
 * percentages against combos, the arrows for the streets and for the paint
 * weight, Alt+S to wipe the filters - because anybody arriving from it should
 * not have to relearn the ones they use without thinking.
 *
 * A few of Flopzilla's cannot be used in a browser at all: Ctrl+T opens a tab,
 * Ctrl+Tab switches one, Ctrl+I opens the bookmarks sidebar, and no page is
 * allowed to intercept them. Those get the nearest web-safe key instead, and the
 * cheat sheet says which ones moved.
 *
 * The whole map lives in one table so the sheet cannot drift from the bindings:
 * it is generated from the same list that handles the presses.
 */

import { track } from "../analytics";
import {
  chrome,
  mutate,
  palette,
  repaint,
  state,
  statDefs,
  statisticsCombos,
  statisticsText,
} from "../store";

/** What a key does, and what to say about it. */
export interface Binding {
  /** As typed, for the sheet: "Tab", "Alt+S", "⇧T". */
  keys: string;
  /** Which group of the sheet it belongs in. */
  group: string;
  /** What it does, in the imperative. */
  does: string;
  /** `event.key`, lower-cased for letters. */
  match: (event: KeyboardEvent) => boolean;
  run: (context: Context) => void;
}

/** What the keyboard needs from the rest of the interface. */
export interface Context {
  /** Deals a random flop. */
  randomBoard: () => void;
  /** Hides or restores one street. */
  stepStreet: (delta: number) => void;
  /** Opens or closes the cheat sheet. */
  toggleSheet: () => void;
  /** Closes whatever is open, if anything. */
  escape: () => boolean;
  /** Reports something to the reader without a dialog. */
  say: (message: string) => void;
  /** The title bar's buttons, pressed rather than reimplemented. */
  actions: Record<string, () => void>;
  /** Shows the one thing in here that does nothing. */
  mascot: () => void;
}

/**
 * What a press means, in the alphabet the shortcuts are written in.
 *
 * `event.key` is what the key *types*, which is the right answer on any Latin
 * keyboard - a French reader pressing the cap marked A means A, wherever that
 * cap happens to sit. It is no answer at all on a Cyrillic or Greek layout,
 * where that same cap types ф and none of the shortcuts exist.
 *
 * `event.code` is where the key *sits*, which is the right answer there. So:
 * the typed character when it is one the shortcuts could be written with, and
 * the position otherwise. Between them every layout gets the keyboard, and a
 * reader never has to switch layouts to deal a flop.
 */
function meaning(event: KeyboardEvent): string {
  // A printable ASCII character is one the shortcuts might name, so it is taken
  // at its word. Space is excluded from the range on purpose: it is named by
  // its position below, so " " and Space cannot disagree.
  if (/^[!-~]$/.test(event.key)) return event.key.toLowerCase();
  return POSITIONS[event.code] ?? event.key;
}

/** What each physical key types on the layout the shortcuts were written for. */
const POSITIONS: Record<string, string> = {
  Space: " ",
  BracketLeft: "[",
  BracketRight: "]",
  Slash: "/",
  ...Object.fromEntries(
    Array.from("abcdefghijklmnopqrstuvwxyz", (letter) => [`Key${letter.toUpperCase()}`, letter]),
  ),
  ...Object.fromEntries(Array.from("0123456789", (digit) => [`Digit${digit}`, digit])),
};

/** Whether the key is a plain letter or digit, with no modifier held. */
function plain(event: KeyboardEvent, key: string): boolean {
  return (
    !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey && meaning(event) === key
  );
}

function shifted(event: KeyboardEvent, key: string): boolean {
  return (
    !event.ctrlKey && !event.metaKey && !event.altKey && event.shiftKey && meaning(event) === key
  );
}

function control(event: KeyboardEvent, key: string): boolean {
  return (event.ctrlKey || event.metaKey) && !event.altKey && meaning(event) === key;
}

/** Copies text, saying so, because a copy with no feedback looks like nothing. */
function copy(context: Context, text: string, what: string): void {
  void navigator.clipboard
    ?.writeText(text)
    .then(() => context.say(`${what} copied`))
    .catch(() => context.say("Could not reach the clipboard"));
}

/** The statistic the panel is currently narrowed to, if any. */
function hovered(): number | undefined {
  return chrome.hovered ?? undefined;
}

export const BINDINGS: Binding[] = [
  {
    keys: "?",
    group: "Everywhere",
    does: "Show this list",
    match: (event) => event.key === "?" || (event.shiftKey && meaning(event) === "/"),
    run: (context) => context.toggleSheet(),
  },
  {
    keys: "Esc",
    group: "Everywhere",
    does: "Close what is open",
    match: (event) => event.key === "Escape",
    run: (context) => {
      context.escape();
    },
  },

  {
    keys: "←  →",
    group: "Board",
    does: "Hide or restore a street",
    match: (event) => event.key === "ArrowLeft" || event.key === "ArrowRight",
    // Handled before the table is consulted, because the direction decides it.
    run: () => {},
  },
  {
    keys: "R",
    group: "Board",
    does: "Deal a random flop",
    match: (event) => plain(event, "r"),
    run: (context) => {
      context.randomBoard();
      track("flop_dealt");
    },
  },
  {
    keys: "Backspace",
    group: "Board",
    does: "Clear the board",
    match: (event) => event.key === "Backspace" || event.key === "Delete",
    run: () => {
      chrome.boardCards = [];
      chrome.visible = 0;
      mutate((engine) => engine.setBoard(""));
      track("board_cleared");
    },
  },

  {
    keys: "↑  ↓",
    group: "Range",
    does: "Paint weight up or down by 5%",
    match: (event) => event.key === "ArrowUp" || event.key === "ArrowDown",
    run: () => {
      const step = 0.05;
      const delta = lastKey === "ArrowUp" ? step : -step;
      chrome.brush = Math.min(1, Math.max(0, Math.round((chrome.brush + delta) * 20) / 20));
      repaint();
    },
  },
  {
    keys: "S",
    group: "Range",
    does: "Switch to the other seat",
    match: (event) => plain(event, "s"),
    run: () => {
      const next = state().active === 0 ? 1 : 0;
      mutate((engine) => engine.setActive(next));
    },
  },

  {
    keys: "Tab",
    group: "Statistics",
    does: "Percentages or combinations",
    match: (event) => event.key === "Tab" && !event.ctrlKey && !event.altKey,
    run: () => {
      chrome.showCombos = !chrome.showCombos;
      repaint();
    },
  },
  {
    keys: "Space",
    group: "Statistics",
    does: "Next colour on the palette",
    match: (event) => plain(event, " "),
    run: () => {
      const colours = palette();
      const at = colours.indexOf(state().colour);
      mutate((engine) => engine.setColour(colours[(at + 1) % colours.length]));
    },
  },
  {
    keys: "1  2  3",
    group: "Statistics",
    does: "Apply or lift the flop, turn or river filter",
    match: (event) => ["1", "2", "3"].includes(meaning(event)) && !event.ctrlKey && !event.metaKey,
    run: () => {
      const street = Number(lastKey) - 1;
      if (street >= state().streetsDealt) return;
      mutate((engine) => engine.toggleStreetFilter(street));
      track("street_filter");
    },
  },
  {
    keys: "Alt+S",
    group: "Statistics",
    does: "Clear every colour and filter",
    match: (event) => event.altKey && meaning(event) === "s",
    run: () => mutate((engine) => engine.clearFilters()),
  },
  {
    keys: "T",
    group: "Statistics",
    does: "Copy the numbers on screen",
    match: (event) => plain(event, "t"),
    run: (context) => {
      const stat = hovered();
      const label = stat === undefined ? "Statistics" : statDefs[stat]?.label;
      copy(context, statisticsText(stat), `${label} table`);
    },
  },
  {
    keys: "⇧T",
    group: "Statistics",
    does: "Copy every combination they cover",
    match: (event) => shifted(event, "t"),
    run: (context) => {
      const stat = hovered();
      const combos = statisticsCombos(stat);
      const count = combos === "" ? 0 : combos.split(",").length;
      copy(context, combos, `${count} combos`);
    },
  },

  {
    keys: "[  ]",
    group: "Output",
    does: "Previous or next view",
    match: (event) => meaning(event) === "[" || meaning(event) === "]",
    run: () => {
      const tabs = ["groups", "overlap", "eq-matrix", "eq-graph", "hotness"] as const;
      const at = tabs.indexOf(chrome.output);
      const step = lastKey === "]" ? 1 : tabs.length - 1;
      chrome.output = tabs[(at + step) % tabs.length];
      repaint();
    },
  },
  {
    keys: "F",
    group: "Output",
    does: "Fold the flops panel away",
    match: (event) => plain(event, "f"),
    run: () => {
      chrome.flopsOpen = !chrome.flopsOpen;
      repaint();
    },
  },

  {
    keys: "C",
    group: "Session",
    does: "Copy the range as text",
    match: (event) => plain(event, "c"),
    run: (context) => context.actions.copyRange(),
  },
  {
    keys: "L",
    group: "Session",
    does: "Copy a link to this session",
    match: (event) => plain(event, "l"),
    run: (context) => context.actions.copyLink(),
  },
  {
    keys: "Ctrl+S",
    group: "Session",
    does: "Save to a file",
    match: (event) => control(event, "s"),
    run: (context) => context.actions.save(),
  },
  {
    keys: "Ctrl+P",
    group: "Session",
    does: "Save the matrix as a picture",
    match: (event) => control(event, "p"),
    run: (context) => context.actions.image(),
  },
  {
    keys: "Ctrl+O",
    group: "Session",
    does: "Load a file",
    match: (event) => control(event, "o"),
    run: (context) => context.actions.load(),
  },
  {
    keys: "A",
    group: "Other",
    does: "Show a girl",
    match: (event) => plain(event, "a"),
    run: (context) => context.mascot(),
  },
];

/** The key of the press being handled, so a shared binding can tell them apart. */
let lastKey = "";

/**
 * Whether the press belongs to the page rather than to the keyboard.
 *
 * Fields take everything. A focused button or link takes Space and Enter,
 * because that is how they are pressed without a mouse - stealing Space there
 * would leave anyone navigating by keyboard unable to press anything at all.
 */
function reserved(event: KeyboardEvent): boolean {
  const target = event.target;
  // A press with nothing focused arrives on the window, which is not an element
  // and has none of the methods below.
  if (!(target instanceof Element)) return false;
  const element = target as HTMLElement;
  if (element.isContentEditable) return true;
  if (/^(INPUT|TEXTAREA|SELECT)$/.test(element.tagName)) return true;
  const activatable = element.closest("button, a[href], [role='button']") !== null;
  return activatable && (event.key === " " || event.key === "Enter");
}

/** Installs the keyboard. Returns a function that removes it again. */
export function installHotkeys(context: Context): () => void {
  const onKey = (event: KeyboardEvent) => {
    // Escape works everywhere, including out of a field. Nothing else does.
    if (event.key === "Escape") {
      if (context.escape()) event.preventDefault();
      return;
    }
    if (reserved(event)) return;

    lastKey = meaning(event);
    const binding = BINDINGS.find((candidate) => candidate.match(event));
    if (!binding) return;
    // Streets and weights share the arrows; the board only answers the
    // horizontal pair and the matrix only the vertical.
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      context.stepStreet(event.key === "ArrowRight" ? 1 : -1);
      return;
    }
    event.preventDefault();
    binding.run(context);
  };
  window.addEventListener("keydown", onKey);
  return () => window.removeEventListener("keydown", onKey);
}

/** The cheat sheet, built from the same table the keyboard runs on. */
export function createHotkeySheet(): {
  element: HTMLElement;
  toggle: () => void;
  close: () => boolean;
} {
  const backdrop = document.createElement("div");
  backdrop.className = "sheet-backdrop";
  backdrop.hidden = true;

  const sheet = document.createElement("div");
  sheet.className = "sheet";
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-label", "Keyboard shortcuts");

  const head = document.createElement("div");
  head.className = "sheet-head";
  const title = document.createElement("h2");
  title.textContent = "Keyboard";
  const close = document.createElement("button");
  close.type = "button";
  close.className = "btn";
  close.textContent = "✕";
  close.title = "Close (Esc)";
  head.append(title, close);

  const body = document.createElement("div");
  body.className = "sheet-body";
  const groups = [...new Set(BINDINGS.map((binding) => binding.group))];
  for (const group of groups) {
    const section = document.createElement("div");
    section.className = "sheet-group";
    const heading = document.createElement("h3");
    heading.className = "sub-title";
    heading.textContent = group;
    section.append(heading);
    for (const binding of BINDINGS.filter((candidate) => candidate.group === group)) {
      const row = document.createElement("div");
      row.className = "sheet-row";
      const keys = document.createElement("kbd");
      keys.textContent = binding.keys;
      const does = document.createElement("span");
      does.textContent = binding.does;
      row.append(keys, does);
      section.append(row);
    }
    body.append(section);
  }

  const foot = document.createElement("p");
  foot.className = "sheet-foot";
  // Why some of these are not the obvious key: a browser keeps the obvious one
  // for itself, and a shortcut that opened a new tab would be worse than none.
  foot.textContent = "A few keys are the second choice, because a browser keeps the first.";

  sheet.append(head, body, foot);
  backdrop.append(sheet);

  const setOpen = (open: boolean) => {
    backdrop.hidden = !open;
    if (open) close.focus();
  };
  close.addEventListener("click", () => setOpen(false));
  backdrop.addEventListener("click", (event) => {
    if (event.target === backdrop) setOpen(false);
  });

  return {
    element: backdrop,
    toggle: () => setOpen(backdrop.hidden),
    close: () => {
      if (backdrop.hidden) return false;
      setOpen(false);
      return true;
    },
  };
}
