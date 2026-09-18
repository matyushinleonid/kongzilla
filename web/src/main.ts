/** Application entry point. */

import "./styles.css";
import { startAnalytics } from "./analytics";
import { embeddedWasm } from "./standalone";
import { boot, onError, repaint, subscribe } from "./store";
import { dismissOne, installDismiss } from "./ui/dismiss";
import { createMenuBar } from "./ui/menubar";
import { createTopStrip } from "./ui/topStrip";
import { createRangePanel } from "./ui/rangePanel";
import { createBoardPanel } from "./ui/boardPanel";
import { createStatsPanel } from "./ui/statsPanel";
import { createOutputPanel } from "./ui/outputPanel";
import { createFlopsPanel } from "./ui/flopsPanel";
import { loadTheme } from "./ui/theme";
import { createHotkeySheet, installHotkeys } from "./ui/hotkeys";
import { createWorkspace, loadColumns } from "./ui/workspace";

async function main(): Promise<void> {
  const root = document.getElementById("app");
  if (!root) throw new Error("the page is missing its #app element");

  try {
    await boot(embeddedWasm());
  } catch (error) {
    root.className = "fatal";
    root.textContent = `The analysis engine did not load: ${String(error)}`;
    return;
  }

  loadColumns();
  loadTheme();

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

  const toast = document.createElement("div");
  toast.className = "toast";
  toast.hidden = true;
  onError((message) => {
    toast.textContent = message;
    toast.hidden = false;
    window.setTimeout(() => {
      toast.hidden = true;
    }, 3000);
  });

  root.className = "app";
  root.replaceChildren(menubar.element, topStrip.element, workspace.element, toast, sheet.element);

  subscribe(() => {
    workspace.render();
    menubar.render();
    topStrip.render();
    range.render();
    board.render();
    stats.render();
    output.render();
    flops.render();
  });

  installHotkeys({
    randomBoard: board.randomBoard,
    stepStreet: board.stepStreet,
    toggleSheet: sheet.toggle,
    actions: menubar.actions,
    escape: () => dismissOne(sheet.close),
    say: (message) => {
      toast.textContent = message;
      toast.hidden = false;
      window.setTimeout(() => {
        toast.hidden = true;
      }, 2000);
    },
  });

  installDismiss();

  repaint();
  startAnalytics();
}

void main();
