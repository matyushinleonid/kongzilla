/**
 * Closing the windows that open over the matrix.
 *
 * There are two of them - the suit breakdown and the combo editor - and they are
 * the same kind of thing: a panel opened on one row or one cell, over the top of
 * everything else. A reader who learns one way out of the first should not have
 * to learn a second way out of the other, so both close on the cross, on Escape,
 * and on a press anywhere that is not them.
 */

import { chrome, repaint } from "../store";

/** What counts as "still inside" for each window. */
const INSIDE = {
  suits: ".suit-popup, .matrix",
  editing: ".edit-strip, .stat-row",
};

/**
 * Closes whichever window the press landed outside of.
 *
 * The cell and the row that open these windows count as inside them: pressing a
 * different matrix cell should move the breakdown, not close it and leave the
 * reader pressing twice for what used to take once.
 */
export function dismissOnPress(target: EventTarget | null): void {
  if (!(target instanceof Element)) return;
  let changed = false;
  if (chrome.suitCell !== null && !target.closest(INSIDE.suits)) {
    chrome.suitCell = null;
    chrome.suitPeek = null;
    changed = true;
  }
  if (chrome.editing !== null && !target.closest(INSIDE.editing)) {
    chrome.editing = null;
    changed = true;
  }
  if (changed) repaint();
}

/**
 * Closes the innermost open window, and says whether there was one.
 *
 * The order is the order they sit in: the sheet is over everything, then the
 * combo editor, then the breakdown.
 */
export function dismissOne(closeSheet: () => boolean): boolean {
  if (closeSheet()) return true;
  // Dealing is a mode rather than a window, but it is the same promise: the
  // key that gets you out of things gets you out of this too.
  if (chrome.dealing !== null) {
    chrome.dealing = null;
    repaint();
    return true;
  }

  if (chrome.editing !== null) {
    chrome.editing = null;
    repaint();
    return true;
  }
  if (chrome.suitCell !== null) {
    chrome.suitCell = null;
    chrome.suitPeek = null;
    repaint();
    return true;
  }
  return false;
}

/** Listens for presses outside the open windows. Returns a way to stop. */
export function installDismiss(): () => void {
  const onPress = (event: PointerEvent) => dismissOnPress(event.target);
  // Capture, and before the panels see it: a press on a matrix cell has to be
  // read as "open that one" rather than as "somewhere outside this one".
  window.addEventListener("pointerdown", onPress, true);
  return () => window.removeEventListener("pointerdown", onPress, true);
}
