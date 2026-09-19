/**
 * The three-column workspace.
 *
 * Starting hands, the board and the statistics sit side by side with a draggable
 * gutter after each one. A gutter always resizes the panel to its left, so the
 * rule is the same wherever you grab. Widths persist per browser, because how much
 * room the matrix deserves is a matter of screen and taste rather than something
 * the app should keep deciding.
 */

import { chrome, repaint } from "../store";

export interface Column {
  key: keyof typeof chrome.columns;
  element: HTMLElement;
  label: string;
  min: number;
}

const STORE_KEY = "kongzilla.columns";

/**
 * The smallest each column may be squeezed to when the whole thing is fitted to
 * a window. Below these the panel stops being usable rather than just tight.
 */
const FLOORS: typeof chrome.columns = { range: 360, board: 180, stats: 260, output: 230 };

/**
 * Below this the stylesheet stacks the columns into one, so there is nothing to
 * fit. It has to match the media query in `styles.css`, and the floors above
 * have to add up to less than it - otherwise the narrowest four-column layout
 * would not fit the narrowest window that still uses four columns.
 */
const STACKS_BELOW = 1080;

/** What the four gutters and the padding cost, outside the columns themselves. */
const CHROME = 4 * 7 + 16;

/**
 * Reads saved column widths, or fits the defaults to the window.
 *
 * The defaults are what the layout wants, not what everyone has. On a narrower
 * screen the last column used to hang off the right-hand edge, which is the
 * one thing a first load must not do - so with nothing saved, the columns are
 * scaled down together until they fit, each stopping at its own floor.
 */
export function loadColumns(): void {
  let restored = false;
  try {
    const saved = localStorage.getItem(STORE_KEY);
    if (saved) {
      const parsed = JSON.parse(saved) as Partial<typeof chrome.columns>;
      for (const key of Object.keys(chrome.columns) as Array<keyof typeof chrome.columns>) {
        const value = parsed[key];
        if (typeof value === "number" && Number.isFinite(value)) {
          chrome.columns[key] = value;
          restored = true;
        }
      }
    }
  } catch {
    // Private windows and blocked storage are fine; the defaults still apply.
  }
  if (!restored) fitColumns();
}

/** Scales the columns down together until they fit the window. */
export function fitColumns(): void {
  if (typeof window === "undefined") return;
  if (window.innerWidth <= STACKS_BELOW) return;
  const room = window.innerWidth - CHROME;
  const keys = Object.keys(chrome.columns) as Array<keyof typeof chrome.columns>;
  const wanted = keys.reduce((sum, key) => sum + chrome.columns[key], 0);
  if (!Number.isFinite(room) || room <= 0 || wanted <= room) return;

  // Take the shortfall out of the columns in proportion to how much slack each
  // has above its floor, so the matrix gives up the most and the board the least.
  const slack = keys.reduce((sum, key) => sum + (chrome.columns[key] - FLOORS[key]), 0);
  const over = wanted - room;
  if (slack <= 0) {
    for (const key of keys) chrome.columns[key] = FLOORS[key];
    return;
  }
  const share = Math.min(1, over / slack);
  for (const key of keys) {
    const give = (chrome.columns[key] - FLOORS[key]) * share;
    chrome.columns[key] = Math.floor(chrome.columns[key] - give);
  }
  // Rounding each column down can still leave a pixel or two over; the matrix
  // is the one with room to spare, so it pays.
  const left = keys.reduce((sum, key) => sum + chrome.columns[key], 0) - room;
  if (left > 0) {
    chrome.columns.range = Math.max(FLOORS.range, chrome.columns.range - left);
  }
}

function saveColumns(): void {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(chrome.columns));
  } catch {
    // Not being able to remember the layout is not worth an error.
  }
}

export function createWorkspace(columns: Column[]): { element: HTMLElement; render: () => void } {
  const workspace = document.createElement("main");
  workspace.className = "workspace";

  for (const column of columns) {
    workspace.append(column.element, gutter(column));
  }

  const fitHeight = () => capPanels(workspace);
  window.addEventListener("resize", fitHeight);

  const render = () => {
    for (const column of columns) {
      workspace.style.setProperty(`--w-${column.key}`, `${chrome.columns[column.key]}px`);
    }
    fitHeight();
  };

  return { element: workspace, render };
}

/**
 * Below this there is no point fitting anything to the height.
 *
 * A phone is scrolled, and a window three hundred pixels tall has no room to
 * give a list either way. Fitting is worth doing where the whole tool can
 * plausibly be seen at once and a panel growing past the edge is a surprise;
 * on anything smaller the page simply scrolls, which is what a reader expects
 * there anyway.
 */
const FITS_ABOVE = 640;

/**
 * What cannot be made shorter, whatever the screen.
 *
 * The matrix is thirteen rows of cells and the board is a row of cards: neither
 * has a list to give up, so neither takes the limit. If one of them is already
 * past the bottom of the window then the page is going to scroll no matter what
 * the statistics panel does - and capping the panel then is the worst of both,
 * a list that scrolls inside a page that also scrolls.
 */
const UNCAPPED = ".panel-range, .panel-output";

/**
 * Tells the panels how much height they may take before they have to scroll
 * inside themselves.
 *
 * The statistics panel grows with what the range can make: on an empty board it
 * is one height and after a flop another, and on a laptop the second one hangs
 * off the bottom of the screen. Rather than hide rows or shrink them, the panel
 * is told where the screen ends and lets its own list scroll - so the palette,
 * the filters and the totals stay put and only the ladder moves.
 *
 * Measured rather than guessed at: what is above the workspace is a strip whose
 * height depends on how many seats there are and how the dead cards wrap.
 */
function capPanels(workspace: HTMLElement): void {
  const stacked = window.innerWidth < STACKS_BELOW;
  if (stacked || window.innerHeight < FITS_ABOVE) {
    workspace.style.removeProperty("--panel-max");
    return;
  }
  const top = workspace.getBoundingClientRect().top + window.scrollY;
  // The workspace's own bottom padding sits below the panels, so it comes out
  // of the room they have. Read rather than repeated: a number copied from the
  // stylesheet is a number that goes stale the first time the stylesheet moves.
  const below = Number.parseFloat(getComputedStyle(workspace).paddingBottom) || 0;
  const room = window.innerHeight - top - below;
  // These heights do not depend on the limit, so reading them here cannot set
  // the limit chasing itself.
  const fixed = Array.from(workspace.querySelectorAll(UNCAPPED)).reduce(
    (tallest, panel) => Math.max(tallest, panel.getBoundingClientRect().height),
    0,
  );
  if (room < fixed) {
    workspace.style.removeProperty("--panel-max");
    return;
  }
  workspace.style.setProperty("--panel-max", `${Math.max(320, Math.floor(room))}px`);
}

function gutter(column: Column): HTMLElement {
  const handle = document.createElement("div");
  handle.className = "gutter";
  handle.dataset.column = column.key;
  handle.setAttribute("role", "separator");
  handle.setAttribute("aria-orientation", "vertical");
  handle.setAttribute("aria-label", `Resize ${column.label}`);
  handle.tabIndex = 0;

  let startX = 0;
  let startWidth = 0;

  const resize = (width: number) => {
    chrome.columns[column.key] = Math.max(column.min, Math.round(width));
    repaint();
  };

  const onMove = (event: MouseEvent) => {
    resize(startWidth + (event.clientX - startX));
  };

  const onUp = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    document.body.classList.remove("resizing");
    saveColumns();
  };

  handle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    startX = (event as MouseEvent).clientX;
    startWidth = chrome.columns[column.key];
    document.body.classList.add("resizing");
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  });

  handle.addEventListener("keydown", (event) => {
    const step = event.shiftKey ? 40 : 10;
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      resize(chrome.columns[column.key] - step);
      saveColumns();
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      resize(chrome.columns[column.key] + step);
      saveColumns();
    }
  });

  return handle;
}
