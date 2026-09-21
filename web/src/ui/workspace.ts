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

  const fitHeight = () => {
    ceiling = fitMatrix(workspace);
    workspace.style.setProperty("--w-range", `${Math.min(chrome.columns.range, ceiling)}px`);
    capPanels(workspace);
  };
  window.addEventListener("resize", fitHeight);
  // The first render measures a page the browser has not laid out yet, so what
  // it works out is about the page as it was a moment ago - and on a laptop
  // that is the difference between fitting and not. One more pass once there
  // is something to measure settles it, and settles in one because the answer
  // does not depend on whether it is already being worn.
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(fitHeight);

  const render = () => {
    for (const column of columns) {
      const wanted = chrome.columns[column.key];
      // The starting-hands column has a ceiling the window sets as well as the
      // one the reader sets, and the smaller of the two wins. See `fitMatrix`.
      const width = column.key === "range" ? Math.min(wanted, ceiling) : wanted;
      workspace.style.setProperty(`--w-${column.key}`, `${width}px`);
    }
    fitHeight();
  };

  return { element: workspace, render };
}

/**
 * The widest the starting-hands column may be before its matrix stops fitting.
 *
 * Kept between renders because it is worked out from a measurement, and the
 * measurement is of a panel that is already wearing the last answer.
 */
let ceiling = Number.POSITIVE_INFINITY;

/**
 * Brings the matrix down to what the window has room for.
 *
 * The matrix is a square of thirteen cells: its height follows its width, so
 * the column being wide is the same thing as the panel being tall. On a laptop
 * that panel is what decides whether the page scrolls - nothing else in it is
 * big enough to matter - and a page that scrolls pushes off whatever happens
 * to be last rather than whatever matters least.
 *
 * So the column gets a second width, the one the height allows, and wears
 * whichever of the two is smaller. The reader's own choice is not overwritten:
 * widen the window and the matrix goes back to the size they asked for.
 *
 * Measured rather than worked out from the stylesheet. How much of the panel
 * is not matrix depends on what is in the library and how the chips wrap, and
 * a number copied from the stylesheet is a number that goes stale.
 */
function fitMatrix(workspace: HTMLElement): number {
  const panel = workspace.querySelector<HTMLElement>(".panel-range");
  const matrix = workspace.querySelector<HTMLElement>(".panel-range .matrix");
  if (!panel || !matrix || window.innerWidth < STACKS_BELOW || window.innerHeight < FITS_ABOVE) {
    return Number.POSITIVE_INFINITY;
  }
  const grid = matrix.getBoundingClientRect();
  const whole = panel.getBoundingClientRect();
  if (grid.height <= 0 || grid.width <= 0) return Number.POSITIVE_INFINITY;

  // Everything measured here is what the answer does *not* change, so that
  // asking twice gives the same answer. Cap the column and the matrix shrinks
  // both ways at once - but its shape stays, the rest of the panel keeps its
  // height, and the weight column beside it keeps its width. Work from those
  // and the ceiling comes out the same whether or not it is already on, which
  // is what stops it chasing itself: shrink, fit, un-shrink, repeat.
  const shape = grid.width / grid.height;
  const rest = whole.height - grid.height;
  const beside = whole.width - grid.width;
  return Math.max(FLOORS.range, Math.floor((roomBelow(workspace) - rest) * shape + beside));
}

/** How much height the workspace has between where it starts and the bottom. */
function roomBelow(workspace: HTMLElement): number {
  const top = workspace.getBoundingClientRect().top + window.scrollY;
  // The workspace's own bottom padding sits below the panels, so it comes out
  // of the room they have. Read rather than repeated: a number copied from the
  // stylesheet is a number that goes stale the first time the stylesheet moves.
  const below = Number.parseFloat(getComputedStyle(workspace).paddingBottom) || 0;
  return window.innerHeight - top - below;
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
    uncap(workspace);
    return;
  }
  const room = roomBelow(workspace);
  // These heights do not depend on the limit, so reading them here cannot set
  // the limit chasing itself.
  const fixed = Array.from(workspace.querySelectorAll(UNCAPPED)).reduce(
    (tallest, panel) => Math.max(tallest, panel.getBoundingClientRect().height),
    0,
  );
  if (room < fixed) {
    uncap(workspace);
    return;
  }
  workspace.style.setProperty("--panel-max", `${Math.max(320, Math.floor(room))}px`);
  workspace.classList.add("capped");
}

/**
 * Takes the ceiling off, and says so.
 *
 * The class matters as much as the property: what scrolls inside a panel is
 * only allowed to scroll while there is a ceiling to scroll under. A box that
 * is a scroll container with nothing to scroll swallows the gesture that would
 * have scrolled the page.
 */
function uncap(workspace: HTMLElement): void {
  workspace.style.removeProperty("--panel-max");
  workspace.classList.remove("capped");
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
