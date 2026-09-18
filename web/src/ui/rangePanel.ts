/** The starting-hand panel: the matrix, the sliders, the quick buttons, the text. */

import {
  chrome,
  library,
  markColour,
  mutate,
  presets,
  rankings,
  repaint,
  state,
  statCombos,
  statDefs,
} from "../store";
import { track } from "../analytics";
import { SUIT_GLYPH } from "./cards";
import { createMatrix, renderMatrix } from "./matrix";

const BRUSHES: Array<[string, number]> = [
  ["100%", 1],
  ["75%", 0.75],
  ["50%", 0.5],
  ["25%", 0.25],
];

export function createRangePanel(): { element: HTMLElement; render: () => void } {
  const panel = document.createElement("section");
  panel.className = "panel panel-range";

  const head = document.createElement("div");
  head.className = "panel-head";
  const rankingSelect = document.createElement("select");
  rankingSelect.className = "select";
  rankingSelect.setAttribute("aria-label", "Hand ranking");
  rankingSelect.addEventListener("change", () =>
    mutate((engine) => engine.setRanking(rankingSelect.value)),
  );
  const title = document.createElement("h2");
  title.className = "panel-title";
  title.textContent = "Starting hand";
  head.append(rankingSelect, title);

  // The weight control: a vertical slider beside the matrix, at full weight by
  // default, with the presets Flopzilla keeps as numbered buttons.
  const weightColumn = document.createElement("div");
  weightColumn.className = "weight-column";
  const weightValue = document.createElement("span");
  weightValue.className = "weight-value num";
  const weightSlider = document.createElement("input");
  weightSlider.type = "range";
  weightSlider.min = "0";
  weightSlider.max = "100";
  weightSlider.step = "5";
  weightSlider.value = "100";
  weightSlider.className = "weight-slider";
  weightSlider.setAttribute("aria-label", "Weight painted onto the matrix");
  weightSlider.addEventListener("input", () => {
    chrome.brush = Number(weightSlider.value) / 100;
    repaint();
  });
  const brushButtons: HTMLButtonElement[] = [];
  const brushRow = document.createElement("div");
  brushRow.className = "brushes";
  for (const [label, weight] of BRUSHES) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "btn brush";
    button.textContent = label;
    button.title = `Paint at ${label} weight`;
    button.addEventListener("click", () => {
      chrome.brush = weight;
      repaint();
    });
    brushButtons.push(button);
    brushRow.append(button);
  }
  weightColumn.append(weightValue, weightSlider, brushRow);

  const matrixRow = document.createElement("div");
  matrixRow.className = "matrix-row";
  const matrix = createMatrix();
  matrixRow.append(matrix, weightColumn);

  // Shift-clicking a statistic opens its hands here, one button per combo, so
  // the ones that continue can be picked out of a category that mostly does not.
  const editStrip = document.createElement("div");
  editStrip.className = "edit-strip";
  editStrip.hidden = true;

  // One slider, two handles: the red one is where the band starts and the blue
  // one where it ends. They span the whole ranking to begin with, and neither can
  // overtake the other.
  const sliders = document.createElement("div");
  sliders.className = "sliders";
  const barTrack = document.createElement("div");
  barTrack.className = "slider-track";
  const span = document.createElement("span");
  span.className = "slider-span";
  const cut = handle("slider-cut", "Where the band starts");
  const top = handle("slider-top", "Where the band ends");
  cut.value = String(chrome.window.low);
  top.value = String(chrome.window.high);
  let handlesSynced = false;
  barTrack.append(span, cut, top);
  const percent = document.createElement("input");
  percent.type = "number";
  percent.min = "0";
  percent.max = "100";
  percent.step = "0.5";
  percent.className = "percent";
  percent.setAttribute("aria-label", "Percent of all hands");
  sliders.append(barTrack, percent);

  const applyWindow = () => {
    mutate((engine) => engine.setWindow(chrome.window.low, chrome.window.high));
  };
  cut.addEventListener("input", () => {
    // Clamp rather than swap, so a handle stops at its neighbour.
    chrome.window.low = Math.min(Number(cut.value), chrome.window.high);
    cut.value = String(chrome.window.low);
    applyWindow();
  });
  top.addEventListener("input", () => {
    chrome.window.high = Math.max(Number(top.value), chrome.window.low);
    top.value = String(chrome.window.high);
    applyWindow();
  });
  // Two handles on one value are two handles in one place, and a native range
  // input gives the press to whichever is on top - here the blue one, which
  // cannot go left because the end of the band may not pass its start. So the
  // gesture did nothing at all. While the two coincide the track takes the
  // gesture itself and gives it to the handle the first movement asks for:
  // leftward to the red one that starts the band, rightward to the blue one
  // that ends it. Once they are apart again the native behaviour is right.
  const valueAt = (clientX: number): number => {
    const rect = barTrack.getBoundingClientRect();
    // A range input insets its travel by half a thumb at each end.
    const thumb = 11;
    const travel = Math.max(1, rect.width - thumb);
    const share = (clientX - rect.left - thumb / 2) / travel;
    return Math.min(100, Math.max(0, Math.round(share * 200) / 2));
  };
  const drive = (which: HTMLInputElement, value: number) => {
    which.value = String(value);
    which.dispatchEvent(new Event("input"));
  };
  let gesture = false;
  let dragging: HTMLInputElement | null = null;
  const capture = (event: PointerEvent, take: boolean) => {
    // Keeps the drag with the track when the pointer leaves it. Not every
    // environment implements it, and the gesture works without it.
    try {
      if (take) barTrack.setPointerCapture(event.pointerId);
      else barTrack.releasePointerCapture(event.pointerId);
    } catch {
      /* no pointer capture here */
    }
  };
  barTrack.addEventListener(
    "pointerdown",
    (event) => {
      if (chrome.window.low !== chrome.window.high) return;
      event.preventDefault();
      gesture = true;
      dragging = null;
      capture(event, true);
    },
    true,
  );
  barTrack.addEventListener("pointermove", (event) => {
    if (!gesture) return;
    const value = valueAt(event.clientX);
    // The first move off the shared value picks the handle; after that the
    // gesture belongs to it, so dragging back does not hand it over again.
    dragging ??= value < chrome.window.low ? cut : value > chrome.window.high ? top : null;
    if (dragging) drive(dragging, value);
  });
  const endDrag = (event: Event) => {
    if (!gesture) return;
    gesture = false;
    dragging = null;
    capture(event as PointerEvent, false);
  };
  barTrack.addEventListener("pointercancel", endDrag);
  // On window, so letting go anywhere ends the drag rather than leaving it live.
  window.addEventListener("pointerup", endDrag);

  percent.addEventListener("change", () => {
    const size = Math.max(0, Math.min(100, Number(percent.value) || 0));
    chrome.window.high = Math.min(100, chrome.window.low + size);
    top.value = String(chrome.window.high);
    applyWindow();
  });

  const summary = document.createElement("p");
  summary.className = "summary";

  // Two rows: the four structural presets with Clear, and under them the value
  // ranges, which are the other kind of thing a quick button can be - a shape
  // of the matrix against a range somebody actually plays.
  const quick = document.createElement("div");
  quick.className = "row quick-row";
  const values = document.createElement("div");
  values.className = "row quick-row value-row";
  const preset = (key: string, label: string) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "btn quick";
    button.dataset.preset = key;
    button.textContent = label;
    button.title = `Add ${label} to the range`;
    button.addEventListener("click", () => mutate((engine) => engine.addPreset(key)));
    return button;
  };
  for (const [key, label, isValueRange] of presets) {
    (isValueRange ? values : quick).append(preset(key, label));
  }
  const clear = document.createElement("button");
  clear.type = "button";
  clear.className = "btn quick";
  clear.textContent = "Clear";
  clear.addEventListener("click", () => {
    chrome.window = { low: 0, high: 0 };
    cut.value = "0";
    top.value = "0";
    mutate((engine) => engine.clearRange());
  });
  quick.append(clear);

  // The preflop library, one block per game. Flopzilla ships an empty predef
  // tree; somewhere to start is worth more than somewhere to store. Cash and
  // tournaments are separate blocks rather than chips in one row, because they
  // are not comparable - a raked NL25 range and a chip-EV range at the same
  // depth are different answers to different questions. Within a block the
  // chips say which format, and their tooltips say what the difference is.
  const GAMES: Array<[string, string]> = [
    ["mtt", "MTT chip-EV"],
    ["cash", "6-max cash"],
  ];
  const libraries = document.createElement("div");
  libraries.className = "libraries";
  const games = GAMES.map(([game, label]) => {
    const block = document.createElement("div");
    block.className = `library library-${game}`;
    const heading = document.createElement("span");
    heading.className = "field-label library-label";
    heading.textContent = label;
    const stackRow = document.createElement("div");
    stackRow.className = "chips stacks";
    block.append(heading, stackRow);
    const rows = new Map<string, { label: HTMLElement; chips: HTMLElement }>();
    libraries.append(block);
    return { game, block, heading, stackRow, rows };
  });

  const notation = document.createElement("textarea");
  notation.className = "notation";
  notation.rows = 2;
  notation.spellcheck = false;
  notation.setAttribute("aria-label", "Range in text notation");
  notation.placeholder = "AKs+, 77-99, QJo";
  let editing = false;
  notation.addEventListener("focus", () => {
    editing = true;
  });
  notation.addEventListener("blur", () => {
    editing = false;
    mutate((engine) => engine.setRangeText(notation.value));
  });
  notation.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) notation.blur();
  });

  panel.append(head, matrixRow, editStrip, summary, sliders, quick, values, libraries, notation);

  // Loading a chart remembers which one, so the chip can stay lit - and
  // remembers what it put in the range, so the light goes out the moment the
  // range stops being that chart.
  const loadChart = (id: string) => {
    mutate((engine) => engine.loadLibrary(id));
    track("chart_loaded");
    chrome.libraryChart = { id, notation: state().players[state().active].notation };
    repaint();
  };

  const render = () => {
    const view = state();
    // A hand has been dealt: there is nothing to type into, paint over or
    // narrow. The panel greys out rather than leaving controls that quietly
    // refuse, which is the difference between "you cannot" and "it is broken".
    panel.classList.toggle("dealt-hand", !view.editable);
    const player = view.players[view.active];
    if (chrome.libraryChart && chrome.libraryChart.notation !== player.notation) {
      chrome.libraryChart = null;
    }

    for (const block of games) {
      const stacks = library.stacks.filter(([, , game]) => game === block.game);
      block.block.hidden = stacks.length === 0;
      if (stacks.length === 0) continue;

      if (block.stackRow.children.length !== stacks.length) {
        block.stackRow.replaceChildren(
          ...stacks.map(([stack, what]) => {
            const chip = document.createElement("button");
            chip.type = "button";
            chip.className = "btn chip stack-chip";
            chip.dataset.stack = stack;
            chip.textContent = stack;
            chip.title = what;
            chip.addEventListener("click", () => {
              chrome.libraryStack = stack;
              repaint();
            });
            return chip;
          }),
        );
        // One row of chips per kind of spot: what a seat opens with, and what
        // the big blind continues with against it.
        for (const [row, label] of library.rows) {
          const name = document.createElement("span");
          name.className = "field-label action-label";
          name.textContent = label;
          const chips = document.createElement("div");
          chips.className = `chips seats action-${row}`;
          block.rows.set(row, { label: name, chips });
          block.block.append(name, chips);
        }
      }
      // Only one game is open at a time; a block nobody has picked in shows its
      // depths without the seats under them.
      const showing = stacks.some(([stack]) => stack === chrome.libraryStack);
      block.block.classList.toggle("open", showing);

      for (const [row, entry] of block.rows) {
        const charts = showing
          ? library.entries.filter(
              (chart) => chart.stack === chrome.libraryStack && chart.row === row,
            )
          : [];
        // Twenty blinds has no limp to isolate, so a row with nothing in it goes.
        entry.label.hidden = charts.length === 0;
        entry.chips.hidden = charts.length === 0;
        if (entry.chips.children.length !== charts.length) {
          entry.chips.replaceChildren(
            ...charts.map(() => {
              const chip = document.createElement("button");
              chip.type = "button";
              chip.className = "btn chip seat-chip";
              // The chart is read off the chip when it is clicked, not captured
              // when it is made: the same chips serve every depth, and a handler
              // that closed over one would keep loading it after the labels had
              // moved on.
              chip.addEventListener("click", () => {
                const id = chip.dataset.chart;
                if (id) loadChart(id);
              });
              return chip;
            }),
          );
        }
        Array.from(entry.chips.children).forEach((chip, index) => {
          const chart = charts[index];
          const button = chip as HTMLButtonElement;
          button.dataset.chart = chart.id;
          button.textContent = chart.label;
          button.title = `${chart.description} — ${chart.percent.toFixed(1)}% of hands`;
          button.classList.toggle("active", chrome.libraryChart?.id === chart.id);
        });
      }

      for (const chip of Array.from(block.stackRow.children) as HTMLButtonElement[]) {
        chip.classList.toggle("active", chip.dataset.stack === chrome.libraryStack);
      }
    }

    if (rankingSelect.options.length !== rankings.length) {
      rankingSelect.replaceChildren(
        ...rankings.map(([key, label]) => {
          const option = document.createElement("option");
          option.value = key;
          option.textContent = label;
          return option;
        }),
      );
    }
    rankingSelect.value = view.ranking;
    rankingSelect.hidden = rankings.length < 2;

    weightValue.textContent = `${(chrome.brush * 100).toFixed(0)}%`;
    if (document.activeElement !== weightSlider) {
      weightSlider.value = String(Math.round(chrome.brush * 100));
    }
    if (!handlesSynced) {
      // The restored window is only known once the engine has booted.
      cut.value = chrome.window.low.toFixed(1);
      top.value = chrome.window.high.toFixed(1);
      handlesSynced = true;
    }
    span.style.left = `${chrome.window.low}%`;
    span.style.right = `${100 - chrome.window.high}%`;

    if (document.activeElement !== percent) percent.value = player.percent.toFixed(1);
    if (!editing) notation.value = player.notation;

    brushButtons.forEach((button, index) => {
      button.classList.toggle("active", BRUSHES[index][1] === chrome.brush);
    });

    const held = player.combos.toFixed(0);
    summary.textContent = `${held} ${held === "1" ? "combo" : "combos"} in range · ${player.percent.toFixed(2)}%`;
    renderEditStrip(editStrip);

    renderMatrix(matrix);
  };

  return { element: panel, render };
}

/**
 * The hands of one statistic, so a category can be painted hand by hand.
 *
 * This is what a gear means: the category no longer speaks for all of its hands,
 * because the nut flushdraw and the four-high one went different ways.
 */
function renderEditStrip(strip: HTMLElement): void {
  const index = chrome.editing;
  if (index === null) {
    strip.hidden = true;
    strip.replaceChildren();
    return;
  }
  const def = statDefs.find((candidate) => candidate.index === index);
  const hands = statCombos(index);
  strip.hidden = false;

  const label = document.createElement("span");
  label.className = "field-label";
  const colour = state().colour;
  label.textContent = `${def?.label ?? ""} — ${hands.length} combos, painting ${colour}`;

  const close = document.createElement("button");
  close.type = "button";
  close.className = "btn suit-close";
  close.textContent = "✕";
  close.title = "Close (shift-click the row again)";
  close.addEventListener("click", () => {
    chrome.editing = null;
    repaint();
  });

  const buttons = hands.map(([combo, name, painted]) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `btn combo-chip paint-${painted}`;
    button.dataset.combo = String(combo);
    markColour(button, painted);
    button.innerHTML = `${name[0]}<i>${SUIT_GLYPH[name[1]]}</i>${name[2]}<i>${SUIT_GLYPH[name[3]]}</i>`;
    button.title = `${name} — ${painted === "none" ? "unpainted" : painted}`;
    button.addEventListener("click", () =>
      mutate((engine) => engine.paintCombo(combo, painted === colour ? "none" : colour)),
    );
    return button;
  });

  strip.replaceChildren(label, ...buttons, close);
}

function handle(className: string, label: string): HTMLInputElement {
  const input = document.createElement("input");
  input.type = "range";
  input.min = "0";
  input.max = "100";
  input.step = "0.5";
  input.className = `slider ${className}`;
  input.setAttribute("aria-label", label);
  return input;
}
