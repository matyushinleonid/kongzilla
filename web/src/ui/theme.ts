/**
 * Colour scheme.
 *
 * Three states, not two: following the system is the default, and choosing light
 * or dark pins it. The choice is remembered per browser.
 */

import { chrome, repaint } from "../store";

const STORE_KEY = "kongzilla.theme";
const ORDER: Array<"light" | "dark" | null> = [null, "light", "dark"];

/** Reads the saved choice and applies it. */
export function loadTheme(): void {
  try {
    const saved = localStorage.getItem(STORE_KEY);
    if (saved === "light" || saved === "dark") chrome.theme = saved;
  } catch {
    // Blocked storage just means the system setting wins.
  }
  applyTheme();
}

/** Stamps the current choice onto the document. */
export function applyTheme(): void {
  const root = document.documentElement;
  if (chrome.theme) {
    root.dataset.theme = chrome.theme;
  } else {
    delete root.dataset.theme;
  }
}

/** Moves to the next choice: system, light, dark. */
export function cycleTheme(): void {
  const next = ORDER[(ORDER.indexOf(chrome.theme) + 1) % ORDER.length];
  chrome.theme = next;
  try {
    if (next) {
      localStorage.setItem(STORE_KEY, next);
    } else {
      localStorage.removeItem(STORE_KEY);
    }
  } catch {
    // The choice still applies for this session.
  }
  applyTheme();
  repaint();
}

/** What the toggle should say, with a glyph so it reads as a colour scheme. */
export function themeLabel(): string {
  switch (chrome.theme) {
    case "light":
      return "☀ Light";
    case "dark":
      return "☾ Dark";
    default:
      return "◐ Auto";
  }
}
