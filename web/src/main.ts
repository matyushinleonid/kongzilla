/** Application entry point. */

import "./styles.css";
import { startAnalytics } from "./analytics";
import { createApp } from "./app";
import { embeddedWasm } from "./standalone";
import { boot } from "./store";
import { loadTheme } from "./ui/theme";
import { loadColumns } from "./ui/workspace";

async function main(): Promise<void> {
  const root = document.getElementById("app");
  if (!root) throw new Error("the page is missing its #app element");

  // Before the engine rather than after it. The theme is the reader's own
  // choice and needs nothing loaded to honour it, while the engine is the best
  // part of a megabyte of WebAssembly - so applying it afterwards meant a
  // reader who had pinned a scheme their system disagrees with watched the
  // wrong one for as long as that took, and the message below, if it came to
  // that, in the wrong one entirely.
  loadTheme();

  try {
    await boot(embeddedWasm());
  } catch (error) {
    root.className = "fatal";
    root.textContent = `The analysis engine did not load: ${String(error)}`;
    return;
  }

  loadColumns();
  createApp(root);
  startAnalytics();
}

void main();
