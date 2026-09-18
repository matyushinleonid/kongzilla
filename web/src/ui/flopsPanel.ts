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
 */

import { track } from "../analytics";
import { chrome, dealFlopFrom, flopBreakdown, repaint, state } from "../store";
import { die } from "./cards";

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
  head.append(fold, title, summary);

  const body = document.createElement("div");
  body.className = "flops-body";

  const render = () => {
    panel.classList.toggle("folded", !chrome.flopsOpen);
    fold.textContent = chrome.flopsOpen ? "▾" : "▸";
    fold.title = chrome.flopsOpen ? "Fold the flops panel away (F)" : "Open the flops panel (F)";
    fold.setAttribute("aria-expanded", String(chrome.flopsOpen));

    const result = flopBreakdown();
    summary.textContent = `${result.total.toLocaleString()} flops`;
    summary.title =
      "After the dead cards. Put your own two cards in the dead-card slots to see how they block.";
    if (!chrome.flopsOpen) {
      body.replaceChildren();
      return;
    }

    const nodes: Node[] = [];
    const board = state().board;
    for (const axis of result.axes) {
      const heading = document.createElement("h3");
      heading.className = "sub-title";
      heading.textContent = axis.label;
      nodes.push(heading);
      for (const group of axis.groups) {
        const row = document.createElement("div");
        row.className = "flop-row";

        const deal = document.createElement("button");
        deal.type = "button";
        deal.className = "btn deal-flop";
        // A die, because that is what the button does: it rolls one.
        deal.append(die());
        deal.title = `Deal a random ${group.label.toLowerCase()} flop`;
        deal.setAttribute("aria-label", deal.title);
        deal.disabled = group.flops === 0;
        deal.addEventListener("click", () => {
          dealFlopFrom(axis.key, group.key);
          track("flop_dealt");
        });

        const label = document.createElement("span");
        label.className = "flop-label";
        label.textContent = group.label;
        const barTrack = document.createElement("span");
        barTrack.className = "bar-track";
        const bar = document.createElement("span");
        bar.className = "bar";
        bar.style.width = `${(group.fraction * 100).toFixed(2)}%`;
        const value = document.createElement("span");
        value.className = "stat-value num";
        value.textContent = `${(group.fraction * 100).toFixed(1)}%`;
        barTrack.append(bar, value);

        row.append(deal, label, barTrack);
        row.title = `${group.flops.toLocaleString()} of ${result.total.toLocaleString()} flops`;
        // The flop on the table is one of these; saying which one saves a squint.
        row.classList.toggle("current", board !== "" && dealtFrom(board, axis.key, group.key));
        nodes.push(row);
      }
    }
    body.replaceChildren(...nodes);
  };

  panel.append(head, body);
  return { element: panel, render };
}

/** Whether the board on the table sits in this bucket. */
function dealtFrom(board: string, axis: string, group: string): boolean {
  const current = chrome.dealtBucket;
  return (
    current !== null && current.board === board && current.axis === axis && current.group === group
  );
}
