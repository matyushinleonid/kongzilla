/**
 * The whole interface, assembled.
 *
 * Everything that has to agree about how the app is put together lives here
 * rather than in the entry point: which panels there are, what order they are
 * drawn in, where an error is shown, which keys are bound to what. The entry
 * point starts the engine and hands the page over; this builds what goes on it.
 *
 * It is one function because the assembly is the thing being got right, and a
 * second copy of it is a second thing to get right. There used to be one: the
 * end-to-end tests built their own version of this, and it drifted - a new
 * piece of chrome nobody added to the copy leaked between tests twice. Now the
 * tests drive the same assembly the reader gets, and what is left to them is
 * what a reader does rather than how the page is wired.
 */

import { onError, repaint, subscribe } from "./store";
import { createBoardPanel } from "./ui/boardPanel";
import { dismissOne, installDismiss } from "./ui/dismiss";
import { createFlopsPanel } from "./ui/flopsPanel";
import { createHotkeySheet, installHotkeys } from "./ui/hotkeys";
import { createMascot } from "./ui/mascot";
import { createMenuBar } from "./ui/menubar";
import { createOutputPanel } from "./ui/outputPanel";
import { createRangePanel } from "./ui/rangePanel";
import { createStatsPanel } from "./ui/statsPanel";
import { createTopStrip } from "./ui/topStrip";
import { createWorkspace } from "./ui/workspace";

/** How long a message stays up: an error longer, because it is read. */
const SAID_MS = 2000;
const WRONG_MS = 3000;

export interface App {
  /** One pass over every panel, which is what a change asks for. */
  render: () => void;
  menubar: HTMLElement;
  strip: HTMLElement;
  range: HTMLElement;
  board: HTMLElement;
  stats: HTMLElement;
  output: HTMLElement;
  flops: HTMLElement;
  sheet: HTMLElement;
  mascot: HTMLElement;
  /** Where a message - an error, or a key saying what it did - is shown. */
  toast: HTMLElement;
  /** Takes it all down again: listeners, timers, and the page itself. */
  teardown: () => void;
}

/** Builds the interface into `root` and starts drawing it. */
export function createApp(root: HTMLElement): App {
  const menubar = createMenuBar();
  const topStrip = createTopStrip();
  const range = createRangePanel();
  const board = createBoardPanel();
  const stats = createStatsPanel();
  const output = createOutputPanel();
  const flops = createFlopsPanel();

  // Flops is about the deck rather than about the range, so it belongs under the
  // board - the column the deck is already in - and not under the output, where
  // it was competing for height with the equity views.
  const boardColumn = document.createElement("div");
  boardColumn.className = "stacked-column";
  boardColumn.append(board.element, flops.element);

  const workspace = createWorkspace([
    { key: "range", element: range.element, label: "the starting-hand panel", min: 340 },
    { key: "board", element: boardColumn, label: "the board panel", min: 190 },
    { key: "stats", element: stats.element, label: "the statistics panel", min: 260 },
    { key: "output", element: output.element, label: "the output panel", min: 280 },
  ]);

  const sheet = createHotkeySheet();
  const mascot = createMascot();

  const toast = document.createElement("div");
  toast.className = "toast";
  toast.hidden = true;
  let saying = 0;
  const say = (message: string, holding: number) => {
    toast.textContent = message;
    toast.hidden = false;
    if (saying) window.clearTimeout(saying);
    saying = window.setTimeout(() => {
      toast.hidden = true;
      saying = 0;
    }, holding);
  };
  onError((message) => say(message, WRONG_MS));

  root.className = "app";
  root.replaceChildren(
    menubar.element,
    topStrip.element,
    workspace.element,
    toast,
    sheet.element,
    mascot.element,
  );

  // The workspace goes last, because what it does is measure: how wide the
  // matrix may be and how much height the panels have left. Measuring first
  // meant measuring the panels as they were before this pass filled them, so
  // the fit was always one render behind whatever had just changed.
  const render = () => {
    menubar.render();
    topStrip.render();
    range.render();
    board.render();
    stats.render();
    output.render();
    flops.render();
    workspace.render();
  };
  const unsubscribe = subscribe(render);

  const uninstallKeys = installHotkeys({
    randomBoard: board.randomBoard,
    stepStreet: board.stepStreet,
    toggleSheet: sheet.toggle,
    actions: menubar.actions,
    mascot: mascot.toggle,
    escape: () => dismissOne(sheet.close) || mascot.close(),
    say: (message) => say(message, SAID_MS),
  });
  const uninstallDismiss = installDismiss();

  repaint();

  return {
    render,
    menubar: menubar.element,
    strip: topStrip.element,
    range: range.element,
    board: board.element,
    stats: stats.element,
    output: output.element,
    flops: flops.element,
    sheet: sheet.element,
    mascot: mascot.element,
    toast,
    teardown: () => {
      unsubscribe();
      uninstallKeys();
      uninstallDismiss();
      // Nothing is registered for errors any more, and a message still on its
      // way out has nowhere to land.
      onError(() => {});
      if (saying) window.clearTimeout(saying);
      root.replaceChildren();
    },
  };
}
