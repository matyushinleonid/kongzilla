/**
 * The flops panel.
 *
 * How often each kind of flop comes is a property of the deck, not of the range,
 * so it belongs beside the output rather than inside it: you read it while you
 * are looking at something else. It sits under the output column and folds down
 * to a strip when the room is wanted elsewhere.
 *
 * Every row deals: reading "two-tone flops are 55% of them" and then having to
 * think one up is the slow half of the work.
 *
 * Every row also picks. A pass over all 22,100 flops says what a range does on
 * average; ticking rows says which flops to average over, so the same question
 * can be put to each range in turn - which of these two does more on a monotone
 * ace-high board. Within one heading the ticks are alternatives and across
 * headings they are conditions, which is the reading that lets a reader narrow
 * as they tick rather than widen.
 *
 * The ticks are only there before a flop is dealt. With one on the table there
 * is nothing to average over - the panel is then a reference, not a control -
 * so they go away, and the choice waits rather than being thrown out.
 */

import { track } from "../analytics";
import {
  chrome,
  clearFlopFilter,
  dealFlop,
  dealFlopFrom,
  flopBreakdown,
  repaint,
  revision,
  state,
  toggleFlopGroup,
} from "../store";
import { die } from "./cards";
import type { FlopBreakdown } from "../types";

/** The flop counts, and the session they were counted for. */
let counted: FlopBreakdown = { total: 0, kept: 0, axes: [] };
let countedAt = -1;

export function createFlopsPanel(): { element: HTMLElement; render: () => void } {
  const panel = document.createElement("section");
  panel.className = "panel panel-flops";

  const head = document.createElement("div");
  head.className = "panel-head";
  const fold = document.createElement("button");
  fold.type = "button";
  fold.className = "btn fold";
  fold.addEventListener("click", () => {
    chrome.flopsOpen = !chrome.flopsOpen;
    repaint();
  });
  const title = document.createElement("h2");
  title.className = "panel-title";
  title.textContent = "Flops";
  const summary = document.createElement("span");
  summary.className = "flops-summary num";

  // Dealing lives here now rather than under the board, because here is where
  // the groups are: press it and you get one of the flops you have ticked,
  // which is the whole loop - narrow, deal, look, deal again.
  const deal = document.createElement("button");
  deal.type = "button";
  deal.className = "btn deal-any";
  deal.append(die());
  deal.addEventListener("click", () => {
    dealFlop();
    track("flop_dealt");
  });

  head.append(fold, title, summary, deal);

  const body = document.createElement("div");
  body.className = "flops-body";

  // Said above the rows rather than on each one: what a tick does is a property
  // of the whole list, and sixteen copies of it would be noise.
  const pickNote = document.createElement("div");
  pickNote.className = "flop-pick-note";
  const pickText = document.createElement("span");
  const pickClear = document.createElement("button");
  pickClear.type = "button";
  pickClear.className = "btn pick-clear";
  // The same word the other panels use for the same act, rather than a
  // sentence of its own: putting something back to nothing is one idea.
  pickClear.textContent = "Clear";
  pickClear.title = "Put every flop back into the pass";
  pickClear.addEventListener("click", () => {
    clearFlopFilter();
    track("flop_group_picked");
  });
  pickNote.append(pickText, pickClear);

  const render = () => {
    panel.classList.toggle("folded", !chrome.flopsOpen);
    fold.textContent = chrome.flopsOpen ? "▾" : "▸";
    fold.title = chrome.flopsOpen ? "Fold the flops panel away (F)" : "Open the flops panel (F)";
    fold.setAttribute("aria-expanded", String(chrome.flopsOpen));

    // Counted when the session changes, not when the pointer moves: it walks
    // all 22,100 flops and buckets each one four ways, and a repaint is far
    // more often a hover than a change.
    if (countedAt !== revision()) {
      countedAt = revision();
      counted = flopBreakdown();
    }
    const result = counted;
    const view = state();
    // Ticking is about what a pass averages over, and a dealt flop is not an
    // average of anything.
    const picking = view.board === "";
    const picked = new Set(view.flopGroups);

    summary.textContent = picked.size
      ? `${view.filteredFlops.toLocaleString()} of ${result.total.toLocaleString()} flops`
      : `${result.total.toLocaleString()} flops`;
    summary.title = picked.size
      ? `The ticked groups leave ${view.filteredFlops.toLocaleString()} of the ${result.total.toLocaleString()} flops the dead cards allow.`
      : "After the dead cards. Put your own two cards in the dead-card slots to see how they block.";

    deal.title = picked.size
      ? `Deal one of the ${view.filteredFlops.toLocaleString()} flops ticked (R)`
      : "Deal a random flop (R)";
    deal.setAttribute("aria-label", deal.title);
    // Nothing is both of the things ticked, so there is nothing to deal.
    deal.disabled = view.filteredFlops === 0;
    if (!chrome.flopsOpen) {
      body.replaceChildren();
      return;
    }

    pickNote.hidden = !picking;
    body.classList.toggle("picking", picking);
    if (picking) {
      pickText.textContent = picked.size
        ? `A pass runs over the ${view.filteredFlops.toLocaleString()} flops ticked. `
        : "Tick rows to run the pass over only those flops, on any range in turn.";
      pickClear.hidden = picked.size === 0;
    }

    const nodes: Node[] = [];
    const board = view.board;
    for (const axis of result.axes) {
      const heading = document.createElement("h3");
      heading.className = "sub-title";
      heading.textContent = axis.label;
      nodes.push(heading);
      for (const group of axis.groups) {
        const row = document.createElement("div");
        row.className = "flop-row";

        const key = `${axis.key}/${group.key}`;
        const on = picked.has(key);
        // Nothing of this group survives the ticks on the other headings, so
        // ticking it would select no flops at all. A box that can only ever
        // mean "none" is not a choice, so it does not offer itself - but one
        // already ticked stays pressable, or there would be no way back.
        const empty = group.kept === 0 && !on;
        const pick = document.createElement("button");
        pick.type = "button";
        pick.className = "btn flop-pick";
        pick.hidden = !picking;
        pick.disabled = empty;
        pick.textContent = on ? "☑" : "☐";
        pick.classList.toggle("on", on);
        pick.setAttribute("aria-pressed", String(on));
        pick.title = empty
          ? `No flop is both ${group.label} and what is ticked above`
          : on
            ? `Stop narrowing the pass to ${group.label} flops`
            : `Run the pass over ${group.label} flops only`;
        pick.setAttribute("aria-label", pick.title);
        pick.addEventListener("click", () => {
          toggleFlopGroup(axis.key, group.key);
          track("flop_group_picked");
        });
        row.classList.toggle("picked", on);
        row.classList.toggle("shut-out", empty && picked.size > 0);

        const deal = document.createElement("button");
        deal.type = "button";
        deal.className = "btn deal-flop";
        // A die, because that is what the button does: it rolls one.
        deal.append(die());
        deal.title = `Deal a random ${group.label} flop`;
        deal.setAttribute("aria-label", deal.title);
        deal.disabled = group.flops === 0;
        deal.addEventListener("click", () => {
          dealFlopFrom(axis.key, group.key);
          track("flop_dealt");
        });

        const label = document.createElement("span");
        label.className = "flop-label";
        label.textContent = group.label;
        // The bar is always the group's share of every flop, so the rows can
        // still be read against each other. What the ticks on the other
        // headings took out of it is drawn in the same bar, faded: the reader
        // sees both how common the group is and how much of it their other
        // conditions leave.
        const barTrack = document.createElement("span");
        barTrack.className = "bar-track";
        const bar = document.createElement("span");
        bar.className = "bar";
        bar.style.width = `${(group.keptFraction * 100).toFixed(2)}%`;
        const lost = document.createElement("span");
        lost.className = "bar-lost";
        lost.style.left = `${(group.keptFraction * 100).toFixed(2)}%`;
        lost.style.width = `${((group.fraction - group.keptFraction) * 100).toFixed(2)}%`;
        const value = document.createElement("span");
        value.className = "stat-value num";
        // The number goes with the solid part of the bar: under a narrowing it
        // is what the reader is actually looking at.
        value.textContent = `${(group.keptFraction * 100).toFixed(1)}%`;
        barTrack.append(bar, lost, value);

        row.append(pick, deal, label, barTrack);
        // What the ticks on the *other* headings leave. The groups of a heading
        // divide those flops between them, so their kept counts add up to it -
        // which is the only denominator this row's number is a share of. The
        // whole filter's count is not: it has this heading's own ticks in it,
        // and tick monotone as well and it would be counting paired monotone
        // flops, of which there are none.
        const elsewhere = axis.groups.reduce((sum, other) => sum + other.kept, 0);
        row.title =
          group.kept === group.flops
            ? `${group.flops.toLocaleString()} of ${result.total.toLocaleString()} flops`
            : // Both denominators spelled out. With one sentence and two
              // numbers in it, a reader pairs the percentage with whichever
              // count is nearer, and it is a share of neither by itself.
              `${group.flops.toLocaleString()} ${group.label} flops in all. ` +
              `${group.kept.toLocaleString()} of them also match what is ticked on the other headings, ` +
              `which is ${elsewhere > 0 ? ((group.kept / elsewhere) * 100).toFixed(1) : "0"}% ` +
              `of the ${elsewhere.toLocaleString()} flops those ticks leave.`;
        // The flop on the table is one of these; saying which one saves a squint.
        row.classList.toggle("current", board !== "" && dealtFrom(board, axis.key, group.key));
        nodes.push(row);
      }
    }
    body.replaceChildren(...nodes);
  };

  panel.append(head, pickNote, body);
  return { element: panel, render };
}

/** Whether the board on the table sits in this bucket. */
function dealtFrom(board: string, axis: string, group: string): boolean {
  const current = chrome.dealtBucket;
  return (
    current !== null && current.board === board && current.axis === axis && current.group === group
  );
}
