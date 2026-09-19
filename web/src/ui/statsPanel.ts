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
  equitySteps,
  revision,
  hoverBreakdown,
  mutate,
  markColour,
  peekStats,
  palette,
  repaint,
  runPreflop,
  runPreflopIfCheap,
  preflopIsCheap,
  setCompareSeat,
  setCut,
  state,
  statDefs,
} from "../store";
import { track } from "../analytics";
import { pipText, seatName, seatTag } from "./cards";
import { press, touchOnly } from "./press";
import type { Block, StatRow } from "../types";

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
    painting = null;
  });
}

const STREETS = ["Flop", "Turn", "River"];

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

  // "Paint the top 20%" - by equity on this board, not by the ladder. A class
  // is a poor proxy for strength: a nut flushdraw and a four-high one are one
  // statistic and nowhere near each other. This paints with the held colour, so
  // it moves the markers rather than the matrix, and the street buttons stay
  // the only thing that narrows anything.
  const shareRow = document.createElement("div");
  shareRow.className = "row share-row";
  const shareLabel = document.createElement("span");
  shareLabel.className = "field-label";
  shareLabel.textContent = "Paint top";
  const shareSlider = document.createElement("input");
  shareSlider.type = "range";
  shareSlider.min = "0";
  shareSlider.max = "100";
  // Any position, and then snapped to the nearest step below: equity across a
  // range is a staircase, so most of the positions on a smooth slider paint
  // exactly what the one beside them paints. The reader still drags anywhere;
  // what they let go of is a step.
  shareSlider.step = "any";
  shareSlider.className = "slider share-slider";
  shareSlider.setAttribute("aria-label", "Share of the range to paint, by equity");
  const shareValue = document.createElement("span");
  shareValue.className = "share-value num";
  const cutClear = button("✕", "Undo, putting back the painting from before");
  cutClear.classList.add("cut-clear");
  shareRow.append(shareLabel, shareSlider, shareValue, cutClear);

  shareSlider.addEventListener("input", () => {
    chrome.continueShare = snapToStep(Number(shareSlider.value) / 100);
    shareSlider.value = String(chrome.continueShare * 100);
    if (chrome.continueShare >= 1) {
      clearCut();
      return;
    }
    setCut(chrome.continueShare);
    track("paint_top", true);
  });
  cutClear.addEventListener("click", () => {
    chrome.continueShare = 1;
    shareSlider.value = "100";
    clearCut();
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

  panel.append(head, paletteRow, passRow, preflopNote, body, shareRow, footer);

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
      shareSlider.style.setProperty("--ticks", ticks());
    }
    if (document.activeElement !== shareSlider) {
      // Exactly where it stands, not rounded to a whole percent: it stands on
      // a step of the staircase, and rounding moved it off the step it had
      // just been snapped to - so the thumb and the number under it disagreed.
      shareSlider.value = String(chrome.continueShare * 100);
    }
    // Equity is equity against somebody, and which somebody is chosen over in
    // the output panel. The slider names it, because a control that quietly
    // depends on a setting two panels away is a control nobody can trust.
    const against = view.versusSeat === null ? null : view.players[view.versusSeat];
    const facing = against ? seatName(against) : null;
    shareLabel.textContent = against ? `Paint top vs ${seatTag(against)}` : "Paint top";
    shareLabel.title = facing
      ? `The strongest share of the range by equity against ${facing}.`
      : "The strongest share of the range by equity against every other range at once.";

    // Report the equity the cut landed on, not the percentage that was asked
    // for: "everything above 61%" is the thing worth knowing.
    const cut = chrome.cut;
    shareValue.textContent = cut
      ? `${(cut.covered * 100).toFixed(0)}% · ${(cut.threshold * 100).toFixed(0)}%+ eq`
      : "all";
    shareValue.title = cut
      ? `${cut.combos.toFixed(0)} combos, every one with at least ${(cut.threshold * 100).toFixed(1)}% equity on ${pipText(cut.board)}`
      : "Nothing painted by equity.";
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

    for (const [index, row] of rows) {
      const data = shown.get(index);
      // Preflop the rows stay visible before the pass has run, so there is a
      // ladder to put checkmarks on.
      row.element.hidden = data === undefined && !preflopMode;
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
      // again, and the caption says what.
      preflopButton.hidden = chrome.preflop !== null && !chrome.preflopRunning;
      // The label does not get a state for "about to run itself": that lasts a
      // quarter of a second, and a button captioned with something it is not
      // going to be asked to do reads as a broken one.
      preflopButton.textContent = chrome.preflopRunning
        ? `Working through ${count} flops…`
        : narrowed
          ? `Calculate over the ${count} flops picked`
          : `Calculate over all ${count} flops`;
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
function applyPainted(index: number): void {
  if (painting === null) return;
  const wanted = painting;
  mutate((engine) => {
    if (preflopMode) {
      const on = state().checkmarks[index];
      if (on !== (wanted !== "none")) engine.toggleCheckmark(index);
      return;
    }
    engine.paintStat(index, wanted);
  });
}

function createRow(index: number, label: string): RowElements {
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
    mutate((engine) => {
      if (preflopMode) {
        engine.toggleCheckmark(index);
        return;
      }
      const view = state();
      engine.paintStat(index, view.marks[index] === view.colour ? "none" : view.colour);
    });
  };
  // Preflop there is no colour to pick up, so the sweep takes its direction from
  // the row it started on: press a ticked row and it unticks, and so does every
  // row the pointer then crosses.
  const startPainting = (event: Event) => {
    event.stopPropagation();
    const view = state();
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
  mark.addEventListener("pointerenter", () => applyPainted(index));

  element.addEventListener("click", (event) => {
    if ((event.target as HTMLElement).closest(".filter-mark")) return;
    if ((event as MouseEvent).shiftKey) return;
    apply();
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
