/**
 * The statistics panel.
 *
 * Three independently computed blocks - made hands, draws, and the two held
 * together - so a hand appears in more than one of them on purpose. Each row is
 * a marker, a name, and a bar with the count at its left edge, which is how
 * Flopzilla draws it.
 *
 * The marker is a funnel in one of the palette's colours. Clicking a row paints
 * its hands with the held colour; if the hands inside a category disagree - the
 * usual case once anything has been edited by hand or the slider has run - the
 * funnel becomes a gear, which is the panel admitting the category no longer
 * speaks for all of them.
 *
 * Painting narrows nothing. The street buttons at the foot do that, one per
 * dealt street: press the flop's and the range going forward is the hands that
 * carried a colour when you pressed it.
 */

import {
  blockLabels,
  chrome,
  clearCut,
  compareBreakdown,
  equityBuckets,
  equitySteps,
  paintEquityBand,
  revision,
  hoverBreakdown,
  mutate,
  markColour,
  peekStats,
  palette,
  repaint,
  runPreflop,
  runPreflopIfCheap,
  preflopEquityMissing,
  preflopEquityReady,
  preflopOutstanding,
  preflopIsCheap,
  setCompareSeat,
  setCut,
  state,
  statDefs,
} from "../store";
import { track } from "../analytics";
import { pipText, seatName, seatTag } from "./cards";
import { press, touchOnly } from "./press";
import type { Block, EquityBucket, StatRow } from "../types";

/** Preflop the panel marks what counts as a hit; postflop it paints. */
let preflopMode = false;

/** What the preflop note says when a pass is over every flop there is. */
const EVERY_FLOP_NOTE =
  "Over every flop at once. Tick the hands you would call a hit, and the footer says how often the range makes one.";

/** And when the flops panel has narrowed which flops those are. */
const NARROWED_NOTE =
  "Over the flops ticked in the flops panel. Tick the hands you would call a hit, and the footer says how often the range makes one.";

/**
 * A marker being dragged across rows.
 *
 * Painting a dozen categories one click at a time is tedious, so the markers
 * work like the matrix does: press one and sweep. The colour picked up on the
 * first row is the colour every row under the pointer gets.
 */
let painting: string | null = null;

if (typeof window !== "undefined") {
  window.addEventListener("pointerup", () => {
    // The end of a brush stroke is the end of the press the band was armed
    // for - and the whole stroke gets the same part of every row it crosses,
    // rather than the first row partial and the rest whole.
    if (painting !== null) disarmBand();
    painting = null;
  });
}

const STREETS = ["Flop", "Turn", "River"];

/** How long the bigger slider waits before going away. */
const SLICE_LINGER_MS = 320;

export function createStatsPanel(): { element: HTMLElement; render: () => void } {
  const panel = document.createElement("section");
  panel.className = "panel panel-stats";

  const head = document.createElement("div");
  head.className = "panel-head";
  const title = document.createElement("h2");
  title.className = "panel-title";
  title.textContent = "Statistics";
  const modeButton = button("absolute", "Switch between this hand and this hand or better");
  const unitButton = button("%", "Switch between percentages and combinations (Tab)");
  // One button rather than a dropdown: with a handful of seats, pressing it
  // again to get to the next one is fewer actions than opening a menu, and it
  // says what it is showing without being opened.
  const versusButton = button("vs —", "Show a second column for another range");
  versusButton.classList.add("versus-button");
  const clearButton = button("Clear", "Unpaint everything and lift every street filter (Alt+S)");
  clearButton.classList.add("clear-filters");
  head.append(title, modeButton, unitButton, versusButton, clearButton);

  versusButton.addEventListener("click", () => {
    const view = state();
    // Round the other seats in order, then back to no comparison at all.
    const others = view.players.map((_, index) => index).filter((index) => index !== view.active);
    if (others.length === 0) return;
    const current = view.compareSeat;
    const at = current === null ? -1 : others.indexOf(current);
    setCompareSeat(at + 1 < others.length ? others[at + 1] : null);
  });

  modeButton.addEventListener("click", () => {
    const next = state().mode === "absolute" ? "cumulative" : "absolute";
    mutate((engine) => engine.setMode(next));
  });
  unitButton.addEventListener("click", () => {
    chrome.showCombos = !chrome.showCombos;
    repaint();
  });
  clearButton.addEventListener("click", () => mutate((engine) => engine.clearFilters()));

  // The palette: the colour every paint uses until another is picked.
  const paletteRow = document.createElement("div");
  paletteRow.className = "row palette";
  const swatches: HTMLButtonElement[] = [];
  for (const colour of palette()) {
    const swatch = document.createElement("button");
    swatch.type = "button";
    swatch.className = `swatch colour-${colour}`;
    markColour(swatch, colour);
    swatch.textContent = "▼";
    swatch.title = `Paint with ${colour}`;
    swatch.setAttribute("aria-label", swatch.title);
    swatch.addEventListener("click", () => mutate((engine) => engine.setColour(colour)));
    swatches.push(swatch);
    paletteRow.append(swatch);
  }
  const eraser = document.createElement("button");
  eraser.type = "button";
  eraser.className = "swatch colour-none";
  eraser.dataset.colour = "none";
  eraser.textContent = "✕";
  eraser.title = "Unpaint what you click on";
  eraser.addEventListener("click", () => mutate((engine) => engine.setColour("none")));
  swatches.push(eraser);
  paletteRow.append(eraser);

  // How often a colour continues, which is what turns painting into a mixed
  // strategy: blue always, green a third of the time. Flopzilla puts a weight
  // on the filter itself; here it belongs to the colour, because the colour is
  // already the thing that says what happens to a group of hands.
  const passRow = document.createElement("div");
  passRow.className = "row pass-row";
  const passLabel = document.createElement("span");
  passLabel.className = "field-label";
  const passSlider = document.createElement("input");
  passSlider.type = "range";
  passSlider.min = "0";
  passSlider.max = "100";
  passSlider.step = "5";
  passSlider.className = "slider pass-slider";
  passSlider.setAttribute("aria-label", "How often this colour continues");
  const passValue = document.createElement("span");
  passValue.className = "pass-value num";
  passRow.append(passLabel, passSlider, passValue);

  passSlider.addEventListener("input", () => {
    const colour = state().colour;
    if (colour === "none") return;
    mutate((engine) => engine.setColourShare(colour, Number(passSlider.value) / 100));
  });

  const invertRows = button(
    "Inv rows",
    "Turn every marker over: what is marked goes bare, what is bare gets the colour. " +
      "Categories overlap, so a hand in a marked and an unmarked one stays painted either way.",
  );
  invertRows.classList.add("palette-action");
  invertRows.addEventListener("click", () => mutate((engine) => engine.invertCategories()));
  const invert = button(
    "Inv hands",
    "Select exactly the hands that are not selected. The two halves add up to the range, " +
      "and a category the split runs through gets a gear to say so.",
  );
  invert.classList.add("palette-action");
  invert.addEventListener("click", () => mutate((engine) => engine.invertGroups()));
  const reset = button("↺", "Back to top pair or better, flushdraws and open-enders");
  reset.classList.add("palette-action");
  reset.addEventListener("click", () => mutate((engine) => engine.resetGroups()));
  paletteRow.append(invertRows, invert, reset);

  // Preflop the panel is a different tool - what the range does against an
  // unknown flop rather than against this one - and the ticks down the side
  // mean something different too. Said here, where the ticking happens, and
  // not only in the footer at the bottom of a long list.
  const preflopNote = document.createElement("p");
  preflopNote.className = "hint preflop-note";
  preflopNote.textContent = EVERY_FLOP_NOTE;

  const body = document.createElement("div");
  body.className = "stats-body";

  const rows = new Map<number, RowElements>();
  const blocks: Block[] = ["made", "draw", "combination"];
  for (const block of blocks) {
    const section = document.createElement("div");
    section.className = `stat-block block-${block}`;
    const heading = document.createElement("h3");
    heading.className = "sub-title";
    heading.textContent = blockLabels.get(block) ?? block;
    section.append(heading);
    for (const def of statDefs.filter((candidate) => candidate.block === block)) {
      const row = createRow(def.index, def.label);
      rows.set(def.index, row);
      section.append(row.element);
    }
    body.append(section);
  }

  /*
   * The same range read the other way: not what a hand is, but what it is
   * worth.
   *
   * A ladder rung says what a hand *made*, and two hands on one rung can be a
   * long way apart - second pair with a flushdraw and top pair with nothing
   * are rungs apart and about the same hand to play. Four bands rather than
   * ten, because this is for taking in a shape; the equity graph two panels
   * over is for looking a number up.
   *
   * Empty where there is nothing to measure against, and the heading goes with
   * it - a block reading nought four times over is worse than no block.
   */
  const bandBlock = document.createElement("div");
  bandBlock.className = "stat-block block-bands";
  const bandHeading = document.createElement("h3");
  bandHeading.className = "sub-title";
  bandHeading.textContent = "By equity";
  bandBlock.append(bandHeading);
  const bandRows = new Map<string, RowElements>();
  for (const key of ["best", "good", "weak", "trash"]) {
    const row = createRow(-1, key, key);
    row.element.dataset.band = key;
    row.element.removeAttribute("data-index");
    // A band is not a statistic, so it cannot be hovered as one - the row
    // factory would put a made-up index into `hovered`, and the matrix would
    // look up a statistic that does not exist and light nothing. It says which
    // band it is instead, and the matrix lights the hands worth that much.
    row.element.addEventListener("pointerenter", () => {
      chrome.hovered = null;
      chrome.hoveredBand = key;
      repaint();
    });
    row.element.addEventListener("pointerleave", () => {
      if (chrome.hoveredBand !== key) return;
      chrome.hoveredBand = null;
      repaint();
    });
    // And held, or shift-clicked, it opens its hands under the matrix - the
    // same thing a rung of the ladder does, for the same reason: a band is a
    // hundred hands and sometimes only a few of them are meant.
    press(row.element, {
      hold: () => {
        chrome.editing = null;
        chrome.editingBand = chrome.editingBand === key ? null : key;
        repaint();
      },
    });
    bandRows.set(key, row);
    bandBlock.append(row.element);
  }
  body.append(bandBlock);

  /*
   * Which part of a row a press paints.
   *
   * A rung says what a hand made, and two hands on one rung can be a long way
   * apart in what they are worth - so pressing one and getting all of it is
   * often not what was meant. Two handles rather than one, because a reader
   * wants the best of the rubbish about as often as the worst of the good.
   *
   * Five per cent a step: the point is to take a slice, not to tune one, and
   * a slider that stopped at every per cent would be a slider nobody could
   * land on twice.
   */
  const bandRow = document.createElement("div");
  bandRow.className = "row band-row";
  const bandLabel = document.createElement("span");
  bandLabel.className = "field-label";
  bandLabel.textContent = "Press a row for";
  const bandFrom = document.createElement("input");
  const bandTo = document.createElement("input");
  for (const [handle, which] of [
    [bandFrom, "where the slice starts"],
    [bandTo, "where it ends"],
  ] as const) {
    handle.type = "range";
    handle.min = "0";
    handle.max = "100";
    handle.step = "5";
    handle.className = "slider band-slider";
    handle.setAttribute("aria-label", `Part of a row a press paints, ${which}`);
  }
  bandFrom.value = "0";
  bandTo.value = "100";
  const bandValue = document.createElement("span");
  bandValue.className = "band-value num";
  const bandClear = button("✕", "Put it back to the whole of a row");
  bandClear.classList.add("band-clear");
  const bandTrack = document.createElement("span");
  bandTrack.className = "slider-track band-track";
  bandTrack.append(bandFrom, bandTo);
  bandRow.append(bandLabel, bandTrack, bandValue, bandClear);

  const bandMoved = () => {
    // Clamp rather than swap, so a handle stops at its neighbour.
    const low = Math.min(Number(bandFrom.value), Number(bandTo.value));
    const high = Math.max(Number(bandFrom.value), Number(bandTo.value));
    chrome.rowBand = { low, high };
    bandFrom.value = String(low);
    bandTo.value = String(high);
    repaint();
  };
  bandFrom.addEventListener("input", bandMoved);
  bandTo.addEventListener("input", bandMoved);
  bandClear.addEventListener("click", () => {
    chrome.rowBand = { low: 0, high: 100 };
    bandFrom.value = "0";
    bandTo.value = "100";
    repaint();
  });

  /*
   * A slice of the range by equity, taken from anywhere in it.
   *
   * By equity on this board, not by the ladder: a rung is a poor proxy for
   * strength, since a nut flushdraw and a four-high one are one statistic and
   * nowhere near each other. This paints with the held colour, so it moves the
   * markers rather than the matrix, and the street buttons stay the only thing
   * that narrows anything.
   *
   * Two handles, because the top was never the only part worth looking at: the
   * hands that are neither good enough to raise nor bad enough to fold are in
   * the middle, and one handle could not reach them.
   *
   * The row itself is small, and a slider this size is a poor thing to aim at.
   * So it reads as an indicator and hands the aiming to a larger one, which
   * opens under the pointer and says which hand the far handle has come to
   * rest on and what that hand is worth - the number the slice is really
   * about.
   */
  const shareRow = document.createElement("div");
  shareRow.className = "row share-row";
  const shareLabel = document.createElement("span");
  shareLabel.className = "field-label";
  shareLabel.textContent = "Paint slice";
  const shareValue = document.createElement("span");
  shareValue.className = "share-value num";
  const cutClear = button("✕", "Undo, putting back the painting from before");
  cutClear.classList.add("cut-clear");

  /** One handle of the slice, at whichever size. */
  const sliceHandle = (which: string, what: string) => {
    const handle = document.createElement("input");
    handle.type = "range";
    handle.min = "0";
    handle.max = "100";
    // Any position, and then snapped to the nearest step below: equity across
    // a range is a staircase, so most positions on a smooth slider paint
    // exactly what the one beside them paints. The reader still drags
    // anywhere; what they let go of is a step.
    handle.step = "any";
    handle.className = `slider share-slider ${which}`;
    handle.setAttribute("aria-label", `Slice of the range to paint, ${what}`);
    return handle;
  };
  const shareFrom = sliceHandle("slice-from", "where it starts");
  const shareTo = sliceHandle("slice-to", "where it ends");
  const shareTrack = document.createElement("span");
  shareTrack.className = "slider-track slice-track";
  shareTrack.append(shareFrom, shareTo);
  shareRow.append(shareLabel, shareTrack, shareValue, cutClear);

  // The larger one, which is where the aiming happens.
  const slicePopup = document.createElement("div");
  slicePopup.className = "slice-popup";
  slicePopup.hidden = true;
  const sliceTitle = document.createElement("span");
  sliceTitle.className = "field-label";
  sliceTitle.textContent = "Paint slice";
  const bigFrom = sliceHandle("slice-from", "where it starts");
  const bigTo = sliceHandle("slice-to", "where it ends");
  const bigTrack = document.createElement("span");
  bigTrack.className = "slider-track slice-track big";
  bigTrack.append(bigFrom, bigTo);
  const sliceReadout = document.createElement("span");
  sliceReadout.className = "slice-readout num";
  slicePopup.append(sliceTitle, bigTrack, sliceReadout);
  shareRow.append(slicePopup);

  const sliceMoved = (from: HTMLInputElement, to: HTMLInputElement) => {
    const low = snapToStep(Math.min(Number(from.value), Number(to.value)) / 100);
    const high = snapToStep(Math.max(Number(from.value), Number(to.value)) / 100);
    chrome.slice = { from: low, to: high };
    chrome.continueShare = high - low;
    if (low <= 0 && high >= 1) {
      clearCut();
      repaint();
      return;
    }
    setCut(low, high);
    track("paint_top", true);
  };
  for (const [from, to] of [
    [shareFrom, shareTo],
    [bigFrom, bigTo],
  ] as const) {
    from.addEventListener("input", () => sliceMoved(from, to));
    to.addEventListener("input", () => sliceMoved(from, to));
  }
  cutClear.addEventListener("click", () => {
    chrome.slice = { from: 0, to: 1 };
    chrome.continueShare = 1;
    clearCut();
    repaint();
  });
  /*
   * Under the pointer, and gone a moment after it leaves.
   *
   * A hover rather than a press: the small one is not a control to open
   * something with, it is the same control at a size nobody can aim at.
   *
   * The moment matters. Closing the instant the pointer leaves the row made
   * the big slider almost unreachable - it sits above the row with a gap under
   * it, and crossing that gap is leaving the row, so only a flick fast enough
   * to skip the gap between two mouse readings ever got there. The gap is
   * bridged in the stylesheet, and this forgives the rest: a breath before it
   * goes, cancelled if the pointer comes back, and never while a handle is
   * being dragged.
   */
  let closing = 0;
  let dragging = false;
  const keepOpen = () => {
    if (closing) window.clearTimeout(closing);
    closing = 0;
    slicePopup.hidden = false;
  };
  const letGo = () => {
    if (closing) window.clearTimeout(closing);
    closing = window.setTimeout(() => {
      closing = 0;
      if (!dragging) slicePopup.hidden = true;
    }, SLICE_LINGER_MS);
  };
  shareRow.addEventListener("pointerenter", keepOpen);
  shareRow.addEventListener("pointermove", keepOpen);
  shareRow.addEventListener("pointerleave", letGo);
  for (const handle of [bigFrom, bigTo, shareFrom, shareTo]) {
    handle.addEventListener("pointerdown", () => {
      dragging = true;
      keepOpen();
    });
  }
  window.addEventListener("pointerup", () => {
    if (!dragging) return;
    dragging = false;
    // The hand may have finished the drag somewhere else entirely, so it goes
    // on the same terms as any other leaving.
    if (!shareRow.matches(":hover")) letGo();
  });

  const footer = document.createElement("div");
  footer.className = "stats-footer";
  const streetRow = document.createElement("div");
  streetRow.className = "row streets";
  const streetButtons = STREETS.map((_label, street) => {
    const control = document.createElement("button");
    control.type = "button";
    control.className = "btn street-filter";
    control.addEventListener("click", () => {
      mutate((engine) => engine.toggleStreetFilter(street));
      track("street_filter");
    });
    streetRow.append(control);
    return control;
  });
  const preflopButton = document.createElement("button");
  preflopButton.type = "button";
  preflopButton.className = "btn filter-toggle";
  preflopButton.addEventListener("click", () => {
    runPreflop();
    track("preflop_run");
  });
  const effective = document.createElement("p");
  effective.className = "effective";
  footer.append(streetRow, preflopButton, effective);

  panel.append(head, paletteRow, passRow, bandRow, preflopNote, body, shareRow, footer);

  /**
   * Marks which edges have more list behind them.
   *
   * Reading `scrollHeight` is a question about layout, and asking it straight
   * after changing the rows makes the browser lay the page out there and then
   * to answer. Doing that on every repaint cost a third of the time a hover
   * took - so it is asked when the list can actually have changed height: when
   * the session moved, when the reader scrolled, or when the window resized.
   */
  const markEdges = () => {
    const hidden = body.scrollHeight - body.clientHeight;
    body.classList.toggle("more-above", body.scrollTop > 1);
    body.classList.toggle("more-below", hidden - body.scrollTop > 1);
  };
  body.addEventListener("scroll", markEdges, { passive: true });
  window.addEventListener("resize", markEdges);

  /** The session the edges were last measured for. */
  let edgesAt = -1;

  /** The bands, and the session they were worked out for. */
  let bands: EquityBucket[] = [];
  let bandsAt = "";

  const render = () => {
    const view = state();
    // What the hand under the pointer is about, wherever the pointer is: the
    // matrix, the suit breakdown, or one of the equity views. Pointing at a
    // hand and pointing at a row are the same question asked from the two
    // ends, and the panel answers both.
    const peek = peekStats();
    preflopMode = view.board === "";
    const source = preflopMode
      ? { rows: chrome.preflop?.rows ?? [] }
      : (hoverBreakdown() ?? view.breakdown);
    const shown = new Map(source.rows.map((row) => [row.index, row]));
    panel.classList.toggle("preflop-mode", preflopMode);

    for (const swatch of swatches) {
      swatch.classList.toggle("active", swatch.dataset.colour === view.colour);
      // A colour that does not continue every time says so on the swatch, so
      // the palette carries the whole strategy rather than only the one being
      // edited below it.
      const at = palette().indexOf(swatch.dataset.colour ?? "") + 1;
      const share = at > 0 ? (view.colourShares[at] ?? 1) : 1;
      swatch.dataset.share = share < 1 ? `${Math.round(share * 100)}` : "";
      swatch.classList.toggle("partial", share < 1);
    }
    paletteRow.hidden = preflopMode;
    preflopNote.hidden = !preflopMode;
    preflopNote.textContent = view.flopGroups.length > 0 ? NARROWED_NOTE : EVERY_FLOP_NOTE;

    // The eraser has no share: unpainted hands never continue.
    const held = view.colour;
    passRow.hidden = preflopMode || held === "none";
    if (!passRow.hidden) {
      const at = palette().indexOf(held) + 1;
      const share = view.colourShares[at] ?? 1;
      passLabel.textContent = `${held} continues`;
      passLabel.style.setProperty("--mark", `var(--group-${at})`);
      passLabel.className = "field-label pass-label";
      if (document.activeElement !== passSlider) {
        passSlider.value = String(Math.round(share * 100));
      }
      passSlider.style.setProperty("--mark", `var(--group-${at})`);
      passValue.textContent = `${Math.round(share * 100)}%`;
    }

    // Preflop the per-combo equities are sampled, and far too noisy to cut on.
    shareRow.hidden = preflopMode;
    // Worked out when the session changes and not when the pointer moves. It
    // sorts the whole range, and repaints are mostly hovers: doing it on every
    // one of those put a third of a second between the pointer arriving on a
    // hand and the panel lighting up.
    if (stepsAt !== revision() || preflopMode) {
      stepsAt = revision();
      steps = preflopMode ? new Float32Array() : equitySteps();
      for (const track of [shareTrack, bigTrack]) {
        track.style.setProperty("--ticks", ticks());
      }
    }
    for (const [from, to] of [
      [shareFrom, shareTo],
      [bigFrom, bigTo],
    ] as const) {
      // Exactly where they stand, not rounded to a whole percent: they stand on
      // a step of the staircase, and rounding moved them off the step they had
      // just been snapped to - so the thumb and the number under it disagreed.
      if (document.activeElement !== from) from.value = String(chrome.slice.from * 100);
      if (document.activeElement !== to) to.value = String(chrome.slice.to * 100);
    }

    // Equity is equity against somebody, and which somebody is chosen over in
    // the output panel. The slider names it, because a control that quietly
    // depends on a setting two panels away is a control nobody can trust.
    const against = view.versusSeat === null ? null : view.players[view.versusSeat];
    const facing = against ? seatName(against) : null;
    const sliceName = against ? `Paint slice vs ${seatTag(against)}` : "Paint slice";
    shareLabel.textContent = sliceName;
    sliceTitle.textContent = sliceName;
    shareLabel.title = facing
      ? `A run of the range by equity against ${facing}, from the top down.`
      : "A run of the range by equity against every other range at once.";

    // Report the equity the cut landed on, not the percentage that was asked
    // for: "everything above 61%" is the thing worth knowing.
    const cut = chrome.cut;
    shareValue.textContent = cut
      ? `${sliceText(cut.from, cut.from + cut.covered)} · ${(cut.threshold * 100).toFixed(0)}%+ eq`
      : "all";
    shareValue.title = cut
      ? `${cut.combos.toFixed(0)} combos, down to ${(cut.threshold * 100).toFixed(1)}% equity on ${pipText(cut.board)}`
      : "Nothing painted by equity.";
    // The hand the far handle has come to rest on, which is the one that says
    // what the slice really reaches: a percentage is a guess at a range, a hand
    // is the range itself.
    sliceReadout.textContent = cut
      ? `${sliceText(cut.from, cut.from + cut.covered)} · down to ${cut.hand ?? "—"} at ${(cut.threshold * 100).toFixed(1)}%`
      : "All of it";
    shareRow.classList.toggle("stale-cut", cut !== null && cut.board !== view.board);
    cutClear.hidden = cut === null;

    modeButton.textContent = view.mode;
    modeButton.classList.toggle("active", view.mode === "cumulative");
    unitButton.textContent = chrome.showCombos ? "combos" : "%";

    // Preflop there is no board for two ranges to be compared on.
    versusButton.hidden = preflopMode;
    const compared = view.compareSeat === null ? null : view.players[view.compareSeat];
    const versusName = compared ? seatName(compared) : null;
    versusButton.textContent = compared ? `vs ${seatTag(compared)}` : "vs —";
    versusButton.classList.toggle("active", versusName !== null);
    versusButton.style.setProperty("--seat", `var(--seat-${view.compareSeat ?? 0})`);
    versusButton.title = versusName
      ? `Comparing against ${versusName}. Press again for the next range.`
      : "Show a second column for another range";

    // The other seat's column, when one is being compared against. Preflop the
    // rows come from the flop pass rather than from a board, and there is no
    // second column to be had.
    const other = preflopMode ? null : compareBreakdown();
    const versus = other === null ? null : new Map(other.rows.map((row) => [row.index, row]));
    const versusSeat = view.compareSeat ?? 0;

    // What this range holds, before a hover narrows it. Preflop that is the
    // pass over the flops, and until one has been run it is nothing at all -
    // so every rung shows, because there has to be a ladder to tick.
    const holds: Map<number, number> | null = preflopMode
      ? chrome.preflop
        ? new Map(chrome.preflop.rows.map((row) => [row.index, row.combos]))
        : null
      : new Map(view.breakdown.rows.map((row) => [row.index, row.combos]));

    for (const [index, row] of rows) {
      const data = shown.get(index);
      // Preflop the rows stay visible before the pass has run, so there is a
      // ladder to put checkmarks on.
      //
      // Otherwise a rung with nothing on it goes. There is no reading to be
      // had from "quads 0%" on a board with no pair, and the panel is long
      // enough without the rungs that never come up - which is also what lets
      // it name third and fourth pair without growing.
      // A rung with nothing on it goes. There is no reading to be had from
      // "quads 0%" on a board with no pair, and the panel is long enough
      // without the rungs that never come up - which is also what lets it name
      // third and fourth pair without growing.
      //
      // Held, not shown: hovering a statistic re-filters the panel and takes
      // most rows to nothing, and a ladder that reshuffled itself under the
      // pointer could not be read. So this asks what the range holds as the
      // street filters leave it, which is what the panel is about anyway.
      //
      // And what the reader has acted on stays whatever it holds: hiding a
      // row that carries a colour or a checkmark would hide the only way back
      // to it.
      const held = holds === null ? 1 : (holds.get(index) ?? 0);
      const painted = (view.marks[index] ?? "empty") !== "empty";
      const ticked = view.checkmarks[index] ?? false;
      row.element.hidden =
        (!preflopMode && data === undefined) || (held <= 0 && !painted && !ticked);
      const mark = preflopMode
        ? view.checkmarks[index]
          ? "check"
          : "empty"
        : (view.marks[index] ?? "empty");
      paintRow(row, index, data, mark, index === chrome.hovered, versus, versusSeat);
      // A share rather than a yes: a cell is up to sixteen hands and only some
      // of them may make this, and half a cell making top pair is worth
      // telling apart from all of it.
      const share = peek?.[index] ?? 0;
      row.element.classList.toggle("makes", share > 0);
      row.element.classList.toggle("makes-some", share > 0 && share < 0.999);
      if (share > 0) row.element.style.setProperty("--makes", share.toFixed(3));
      else row.element.style.removeProperty("--makes");
    }

    // The bands, which need something to measure against: without that there
    // is no per-hand equity and nothing to sort by.
    //
    // Two things move them, not one. The range moving is the obvious one; the
    // other is a pass over the flops finishing, which is what works out
    // per-hand equity before there is a board - and that changes nothing about
    // the session, so the revision does not move with it. Watching the
    // revision alone left the block empty until something else happened to
    // bump it, which meant the reader saw it fill in only when they changed
    // seats and came back.
    const bandsKey = `${revision()}/${preflopEquityReady()}`;
    if (bandsAt !== bandsKey) {
      bandsAt = bandsKey;
      bands = equityBuckets();
    }
    bandBlock.hidden = bands.length === 0;
    for (const band of bands) {
      const row = bandRows.get(band.key);
      if (!row) continue;
      row.element.classList.toggle("empty-band", band.fraction <= 0);
      row.label.textContent = band.label;
      row.value.textContent = `${(band.fraction * 100).toFixed(1)}%`;
      row.bar.style.width = `${(band.fraction * 100).toFixed(1)}%`;
      row.element.title =
        `${band.label}: equity ${band.low} to ${band.high} per cent, ` +
        `${formatCombos(band.combos)} combos. Press to paint them.`;
      row.mark.className = "filter-mark mark-none";
      row.mark.textContent = "▼";
      row.element.classList.toggle("editing", chrome.editingBand === band.key);
    }

    // What the band is asking for, said in words rather than in two numbers:
    // "0-100" is a thing a reader has to decode and "all of it" is not.
    const { low, high } = chrome.rowBand;
    // Only where there is an opponent to be strong against. The part it takes
    // is the top of a row *by equity*, so with nothing to measure against
    // there is no top and no bottom - the control would be a control that
    // silently does nothing, which is worse than one that is not there.
    bandRow.hidden = preflopMode || bands.length === 0;
    bandRow.title =
      "Pressing a row paints this much of it, strongest first, by equity against the other range. " +
      "All of it is the plain thing a press has always done; anything less leaves the row half " +
      "painted, which is what a gear beside it means.";
    bandFrom.value = String(low);
    bandTo.value = String(high);
    bandValue.textContent =
      low <= 0 && high >= 100
        ? "all of it"
        : low <= 0
          ? `top ${high}%`
          : high >= 100
            ? `bottom ${100 - low}%`
            : `${low}\u2013${high}%`;
    bandRow.classList.toggle("narrowed", !(low <= 0 && high >= 100));
    bandClear.hidden = low <= 0 && high >= 100;

    streetRow.hidden = preflopMode;
    streetButtons.forEach((control, street) => {
      const applied = view.streetsOn[street] ?? false;
      control.hidden = street >= view.streetsDealt;
      // Cumulative, as Flopzilla has it: each button says what is left once the
      // filters down to it have run, so the numbers read as a chain and the
      // last one on matches the footer. Off, it says what pressing would leave.
      const combos = view.streetCounts[street] ?? 0;
      control.replaceChildren(
        light(applied),
        document.createTextNode(`${STREETS[street]} · ${formatCombos(combos)}`),
      );
      control.classList.toggle("active", applied);
      control.title = applied
        ? `${formatCombos(combos)} combos left after the ${STREETS[street].toLowerCase()} filter. Click to lift it (${street + 1})`
        : `${formatCombos(combos)} combos would be left. Press to keep only the painted hands from the ${STREETS[street].toLowerCase()} on (${street + 1})`;
    });

    preflopButton.hidden = !preflopMode;
    if (preflopMode) {
      // Which flops, said on the button that runs the pass: narrowing happens
      // in another panel, and a reader who has forgotten they narrowed would
      // otherwise read the answer as being about every flop.
      const narrowed = view.flopGroups.length > 0;
      const cheap = preflopIsCheap();
      const count = view.filteredFlops.toLocaleString();
      // Gone once there is a pass. It offered to run it again, and running it
      // again gave the same answer instantly - the pass is kept under what it
      // was a pass over, so asking twice is the same question. A button whose
      // press changes nothing is worse than no button.
      //
      // It comes back the moment the pass stops applying: change the range,
      // the dead cards or the ticked flops and there is something to ask for
      // again, and the caption says what. The per-hand equity rides along with
      // a pass but goes stale on its own terms - it is about this range
      // against another one - so filling the second seat after a pass leaves
      // the breakdown standing and the equity never worked out. That is
      // something to ask for too, and without this the button was not there to
      // ask it with.
      // Every seat, not only the one being looked at: a reader who has just
      // filled the second range and gone back to the first was being shown
      // nothing to press, and the way to get the answer was to go to the other
      // seat, press there, and come back.
      const waiting = preflopOutstanding();
      const outstanding = waiting > 0 || chrome.preflop === null || preflopEquityMissing();
      preflopButton.hidden = !outstanding && !chrome.preflopRunning;
      // The label does not get a state for "about to run itself": that lasts a
      // quarter of a second, and a button captioned with something it is not
      // going to be asked to do reads as a broken one.
      // Where only the equity is outstanding the caption says so, or a reader
      // looking at a breakdown that is already on screen would read the button
      // as one that does nothing.
      const onlyEquity = chrome.preflop !== null && preflopEquityMissing() && waiting <= 1;
      // How many ranges it is about, where that is more than one: a press that
      // is going to take four seconds rather than one should say so before it
      // takes them.
      const ranges = waiting > 1 ? ` · ${waiting} ranges` : "";
      preflopButton.textContent = chrome.preflopRunning
        ? `Working through ${count} flops…`
        : onlyEquity
          ? `Add equity over ${narrowed ? "the" : "all"} ${count} flops`
          : narrowed
            ? `Calculate over the ${count} flops picked${ranges}`
            : `Calculate over all ${count} flops${ranges}`;
      preflopButton.disabled = chrome.preflopRunning;
      preflopButton.title = cheap
        ? "This one is quick, so it runs itself once the range stops moving."
        : narrowed
          ? "The flops panel is narrowing this to the groups ticked there. Tick more groups, or a narrower range, and it stops needing to be asked."
          : "Every flop the dead cards allow, which is the slow one. Tick groups in the flops panel to narrow it.";
      // Cheap enough to be nobody's decision: run it and show the numbers. Only
      // the passes that would freeze the page for a second or more are left for
      // the reader to ask for.
      runPreflopIfCheap();
      // Three states, and each says what to do next. A tick with nothing to
      // tick against is what made the whole thing look broken: the mark went
      // on and not one number moved.
      const ticked = view.checkmarks.some(Boolean);
      const over = narrowed
        ? `the ${chrome.preflop?.flops.toLocaleString() ?? count} flops picked`
        : "every flop";
      effective.textContent = !chrome.preflop
        ? `How the range hits an unknown flop, averaged over ${narrowed ? over : "every one of them"}.`
        : ticked
          ? `Hits ${(chrome.preflop.hit * 100).toFixed(2)}% of the time — the share of ${over} where the range makes one of the ticked hands.`
          : "Tick the statistics that count as hitting, and this says how often the range hits at all.";
    } else {
      // The three numbers Flopzilla puts here, and nothing else. A dump of the
      // notation was a wall of text in a corner nobody reads it from - the
      // range is on screen in the matrix, and ⇧T copies it if it is wanted.
      const live = view.liveCombos;
      const passing = live * view.passFraction;
      const lines = [
        `Total number of combos: ${formatCombos(live)}`,
        view.filtersEnabled
          ? `Combos that pass the filters: ${formatCombos(passing)} (${(view.passFraction * 100).toFixed(2)}%)`
          : `Painted: ${formatCombos(view.wouldPass)} (${live > 0 ? ((view.wouldPass / live) * 100).toFixed(2) : "0.00"}%)`,
        `The filters are ${view.filtersEnabled ? "ON" : "OFF"}`,
      ];
      // A street filter freezes combos, not a rule, so it carries to the next
      // card unchanged - say where the painting came from when it has moved on.
      if (cut && cut.board !== view.board) lines.push(`Painted on ${pipText(cut.board)}`);
      effective.replaceChildren(
        ...lines.map((line) => {
          const row = document.createElement("span");
          row.textContent = line;
          return row;
        }),
      );
    }

    if (edgesAt !== revision()) {
      edgesAt = revision();
      markEdges();
    }
  };

  return { element: panel, render };
}

interface RowElements {
  element: HTMLElement;
  mark: HTMLButtonElement;
  label: HTMLElement;
  bar: HTMLElement;
  value: HTMLElement;
  versus: HTMLElement;
}

/** Applies the drag in progress to one row. */
function applyPainted(index: number, band?: string): void {
  if (painting === null) return;
  const wanted = painting;
  // A band is not a rung of the ladder: there is no statistic behind it, so
  // what it paints is worked out from equity rather than looked up.
  if (band !== undefined) {
    paintEquityBand(band, wanted);
    return;
  }
  mutate((engine) => {
    if (preflopMode) {
      const on = state().checkmarks[index];
      if (on !== (wanted !== "none")) engine.toggleCheckmark(index);
      return;
    }
    // The same part of the row the marker beside it would paint: pressing the
    // triangle and pressing the row are the same act, and answering them
    // differently was the surest way to make the band look broken.
    if (wanted === "none" || whole()) {
      engine.paintStat(index, wanted);
      return;
    }
    engine.paintStatPart(index, chrome.rowBand.low, chrome.rowBand.high, wanted);
  });
}

/**
 * Puts the band back to the whole of a row, once the painting is done.
 *
 * Armed, used, disarmed: a part-of-a-row press is a thing a reader wants for
 * one press, and leaving it set meant every press after it was quietly partial
 * too - which reads as the panel painting the wrong hands rather than as a
 * setting still being on.
 */
function disarmBand(): void {
  if (whole()) return;
  chrome.rowBand = { low: 0, high: 100 };
  repaint();
}

/** Whether the band is the whole of a row, which is what a press used to do. */
function whole(): boolean {
  return chrome.rowBand.low <= 0 && chrome.rowBand.high >= 100;
}

function createRow(index: number, label: string, band?: string): RowElements {
  const element = document.createElement("div");
  element.className = "stat-row";
  element.dataset.index = String(index);
  element.tabIndex = 0;
  element.setAttribute("role", "button");

  const mark = document.createElement("button");
  mark.type = "button";
  mark.className = "filter-mark";
  mark.tabIndex = -1;

  const name = document.createElement("span");
  name.className = "stat-label";
  name.textContent = label;

  const barTrack = document.createElement("span");
  barTrack.className = "bar-track";
  const bar = document.createElement("span");
  bar.className = "bar";
  const value = document.createElement("span");
  value.className = "stat-value num";
  barTrack.append(bar, value);

  // The other seat's number for the same statistic, hidden until one is being
  // compared against. It sits outside the bar because the bar is about this
  // range: two bars in one track would read as one range split in two.
  const versus = document.createElement("span");
  versus.className = "stat-versus num";
  versus.hidden = true;

  // A lone arrow glyph is a riddle: it says a key is involved and not which
  // one or what it does. So the offer is made in words, in a small box that
  // appears under the pointer the way the suit breakdown does.
  const shift = document.createElement("i");
  shift.className = "shift-hint";
  shift.textContent = touchOnly() ? "hold for combos" : "⇧-click for combos";
  shift.setAttribute("aria-hidden", "true");
  element.append(mark, name, barTrack, versus, shift);

  // Clicking paints with the held colour; clicking a row already wholly in that
  // colour unpaints it, so one control both sets and clears.
  const apply = () => {
    if (band !== undefined) {
      // Pressing a band paints it, in whichever part the band row asks for:
      // press "trash hands" with the slider on its best fifth and you have the
      // best of the rubbish, which is what goes in a betting range.
      const colour = state().colour;
      if (colour !== "none") paintEquityBand(band, colour);
      return;
    }
    mutate((engine) => {
      if (preflopMode) {
        engine.toggleCheckmark(index);
        return;
      }
      const view = state();
      const wanted = view.marks[index] === view.colour ? "none" : view.colour;
      // Which part of the row, by equity. The whole of it is still painted as
      // a category, so nothing about pressing a row has changed until the
      // reader moves the band.
      if (wanted === "none" || whole()) {
        engine.paintStat(index, wanted);
        return;
      }
      engine.paintStatPart(index, chrome.rowBand.low, chrome.rowBand.high, wanted);
    });
  };
  // Preflop there is no colour to pick up, so the sweep takes its direction from
  // the row it started on: press a ticked row and it unticks, and so does every
  // row the pointer then crosses.
  const startPainting = (event: Event) => {
    event.stopPropagation();
    const view = state();
    if (band !== undefined) {
      if (view.colour === "none") return;
      painting = view.colour;
      applyPainted(index, band);
      return;
    }
    painting = preflopMode
      ? view.checkmarks[index]
        ? "none"
        : "paint"
      : view.marks[index] === view.colour
        ? "none"
        : view.colour;
    applyPainted(index);
  };
  press(mark, { act: startPainting });
  mark.addEventListener("pointerenter", () => applyPainted(index, band));

  element.addEventListener("click", (event) => {
    if ((event.target as HTMLElement).closest(".filter-mark")) return;
    if ((event as MouseEvent).shiftKey) return;
    apply();
    disarmBand();
    // Preflop a tick is a question about a pass that may not have been run.
    // Asking the question is as good a reason to run it as pressing the button,
    // and it is the only reading under which the tick does anything at all.
    if (preflopMode && !chrome.preflop && !chrome.preflopRunning) {
      runPreflop();
      track("preflop_run");
    }
  });
  element.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      apply();
    }
  });
  element.addEventListener("pointerenter", () => {
    chrome.hovered = index;
    repaint();
  });
  element.addEventListener("pointerleave", () => {
    if (chrome.hovered === index) {
      chrome.hovered = null;
      repaint();
    }
  });
  // Shift-click - or, on a screen with no shift key, a held finger - opens the
  // category over the matrix, where its hands can be painted one at a time,
  // which is what turns the funnel into a gear.
  press(element, {
    when: () => !preflopMode,
    hold: (event) => {
      if ((event.target as HTMLElement).closest(".filter-mark")) return;
      chrome.editing = chrome.editing === index ? null : index;
      repaint();
    },
  });

  return { element, mark, label: name, bar, value, versus };
}

function paintRow(
  row: RowElements,
  index: number,
  data: StatRow | undefined,
  mark: string,
  hovered: boolean,
  compare: Map<number, StatRow> | null,
  compareSeat: number,
): void {
  row.element.classList.toggle("hovered", hovered);
  row.element.classList.toggle("editing", chrome.editing === index);
  row.mark.className = `filter-mark mark-${mark}`;
  markColour(row.mark, mark);
  // A gear says the category's own hands disagree, which is what happens as
  // soon as anything inside it is painted by hand or by the slider.
  row.mark.textContent = mark === "mixed" ? "⚙" : mark === "check" ? "✓" : "▼";
  row.mark.title =
    mark === "mixed"
      ? "Some of these hands are painted and some are not. Shift-click the row to see which."
      : mark === "empty"
        ? "The range holds none of these"
        : mark === "none"
          ? "Unpainted"
          : `Painted ${mark}`;

  const other = compare?.get(index);
  row.versus.hidden = compare === null;
  if (compare !== null) {
    row.versus.textContent = other
      ? chrome.showCombos
        ? formatCombos(other.combos)
        : `${(other.fraction * 100).toFixed(1)}%`
      : "—";
    row.versus.style.setProperty("--seat", `var(--seat-${compareSeat})`);
    // Which way the difference goes is the thing being read, so it is said in
    // weight rather than left to the reader to subtract two numbers.
    row.versus.classList.toggle("ahead", !!(other && data && other.fraction > data.fraction));
  }

  if (!data) {
    row.element.classList.add("empty");
    row.bar.style.width = "0%";
    row.value.textContent = "";
    return;
  }
  // A statistic the range cannot make is not a choice worth offering, so its
  // name recedes rather than sitting at full strength next to the live ones.
  row.element.classList.toggle("empty", data.combos === 0);
  row.bar.style.width = `${Math.min(100, data.fraction * 100).toFixed(2)}%`;
  row.value.textContent = chrome.showCombos
    ? formatCombos(data.combos)
    : `${(data.fraction * 100).toFixed(1)}%`;
  row.label.title = `${data.label}: ${formatCombos(data.combos)} combos`;
}

/** The lamp on a street's button: green when the filter is on, red when it is not. */
function light(on: boolean): HTMLElement {
  const lamp = document.createElement("i");
  lamp.className = on ? "lamp on" : "lamp";
  lamp.setAttribute("aria-hidden", "true");
  return lamp;
}

function formatCombos(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

/** The steps of the staircase, as the panel last read them, and from when. */
let steps: Float32Array = new Float32Array();
let stepsAt = -1;

/**
 * The step at or below a share.
 *
 * Below the first step there is nothing to paint, so it stays where it is put
 * and the cut comes out empty; that is the reader saying "none of it".
 */
function snapToStep(share: number): number {
  if (steps.length === 0) return share;
  let best = share;
  for (const step of steps) {
    if (step <= share + 1e-6) best = step;
    else break;
  }
  return share < steps[0] ? share : best;
}

/** What a slice from `low` to `high` is called, as shares of the range. */
function sliceText(low: number, high: number): string {
  const from = Math.round(low * 100);
  const to = Math.round(high * 100);
  if (from <= 0 && to >= 100) return "all";
  if (from <= 0) return `top ${to}%`;
  if (to >= 100) return `bottom ${100 - from}%`;
  return `${from}\u2013${to}%`;
}

/** The staircase drawn on the track, one hairline per step. */
function ticks(): string {
  if (steps.length === 0 || steps.length > 120) return "none";
  const marks: string[] = [];
  for (const step of steps) {
    const at = step * 100;
    if (at >= 99.9) continue;
    marks.push(
      `transparent ${(at - 0.35).toFixed(2)}%, var(--line) ${(at - 0.35).toFixed(2)}%, var(--line) ${(at + 0.35).toFixed(2)}%, transparent ${(at + 0.35).toFixed(2)}%`,
    );
  }
  return marks.length > 0 ? `linear-gradient(to right, ${marks.join(", ")})` : "none";
}

function button(label: string, title: string): HTMLButtonElement {
  const element = document.createElement("button");
  element.type = "button";
  element.className = "btn";
  element.textContent = label;
  element.title = title;
  return element;
}
