/**
 * The range as a picture.
 *
 * A range lives in a matrix, and a matrix is the one thing you cannot paste
 * into a forum post or a hand history. A link carries the whole session, which
 * is the right thing when the reader has the app; a picture is the right thing
 * when they have a chat window. So this draws what the matrix is showing -
 * weight from the bottom, what the filters left from the left, the colours
 * along the foot - onto a canvas, at a size worth looking at.
 *
 * The colours are read out of the live stylesheet rather than repeated here, so
 * the picture is in whichever theme the reader is looking at and cannot drift
 * from what is on their screen.
 */

import { chrome, classLabels, palette, state } from "../store";

/** How big one cell is drawn, before the device's pixel ratio. */
const CELL = 46;
const GAP = 2;
const PAD = 18;
const HEADER = 46;
const FOOT = 20;

/** Reads one CSS custom property off the document. */
function token(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value === "" ? fallback : value;
}

/** Draws the active range's matrix and returns it as a PNG blob. */
export async function rangeImage(scale = 2): Promise<Blob> {
  const view = state();
  const width = PAD * 2 + 13 * CELL + 12 * GAP;
  const height = PAD * 2 + HEADER + 13 * CELL + 12 * GAP + FOOT;

  const canvas = document.createElement("canvas");
  canvas.width = width * scale;
  canvas.height = height * scale;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("this browser will not draw a canvas");
  context.scale(scale, scale);

  const ink = token("--ink", "#1c1f23");
  const soft = token("--ink-soft", "#6b7280");
  const panel = token("--panel", "#ffffff");
  const empty = token("--cell-empty", "#fbfbfc");
  const filtered = token("--cell-filtered", "#d8dade");
  const edge = token("--cell-edge", "#e3e5e9");
  const paints = [
    token("--paint-pair", "#bcd6f0"),
    token("--paint-suited", "#dcc98a"),
    token("--paint-offsuit", "#e0b3ae"),
  ];
  // Unpainted, then one per palette colour: the picture has exactly the
  // colours the panel has, however many that is.
  const groups = Array.from({ length: palette().length + 1 }, (_, at) =>
    token(`--group-${at}`, "#c9c9c9"),
  );

  context.fillStyle = panel;
  context.fillRect(0, 0, width, height);

  // The header says which range on which board, because a matrix on its own
  // does not: the same picture means different things on different flops.
  const player = view.players[view.active];
  context.fillStyle = ink;
  context.font = "600 15px system-ui, sans-serif";
  const board = view.boardCards.slice(0, chrome.visible).join(" ");
  context.fillText(
    board === "" ? `${player.name} — preflop` : `${player.name} — ${board}`,
    PAD,
    PAD + 15,
  );
  context.fillStyle = soft;
  context.font = "12px system-ui, sans-serif";
  context.fillText(
    `${player.combos.toFixed(0)} combos · ${player.percent.toFixed(2)}%${
      view.filtersEnabled ? ` · filters on, ${view.passFraction.toFixed(2)} through` : ""
    }`,
    PAD,
    PAD + 33,
  );

  const top = PAD + HEADER;
  for (let index = 0; index < 169; index += 1) {
    const row = Math.floor(index / 13);
    const column = index % 13;
    const x = PAD + column * (CELL + GAP);
    const y = top + row * (CELL + GAP);

    const weight = view.classWeights[index] ?? 0;
    const passing = view.classPassing[index] ?? 0;
    const paint = paints[row === column ? 0 : row < column ? 1 : 2];

    // The ground, then the weight rising from the bottom, then the part the
    // filters took repainted grey from the left: the same three axes the cell
    // on screen uses, so the picture reads the way the screen does.
    context.fillStyle = empty;
    context.fillRect(x, y, CELL, CELL);
    if (weight > 0) {
      const filled = CELL * Math.min(1, weight);
      context.fillStyle = paint;
      context.fillRect(x, y + CELL - filled, CELL, filled);
      if (view.filtersEnabled && passing < 1) {
        const kept = CELL * Math.max(0, Math.min(1, passing));
        context.fillStyle = filtered;
        context.fillRect(x + kept, y + CELL - filled, CELL - kept, filled);
      }
    }

    // The colours the cell holds, as a strip along its foot.
    const shares = view.classColours[index] ?? [];
    if (view.coloursUsed > 1 && weight > 0) {
      let walked = 0;
      for (let colour = 1; colour < shares.length; colour += 1) {
        const share = shares[colour] ?? 0;
        if (share <= 0) continue;
        context.fillStyle = groups[colour] ?? groups[0];
        context.fillRect(x + walked * CELL, y + CELL - 4, share * CELL, 4);
        walked += share;
      }
    }

    context.strokeStyle = edge;
    context.lineWidth = 1;
    context.strokeRect(x + 0.5, y + 0.5, CELL - 1, CELL - 1);

    context.fillStyle = weight > 0 ? ink : soft;
    context.font = `${weight > 0 ? "600" : "400"} 13px system-ui, sans-serif`;
    context.textAlign = "center";
    context.fillText(classLabels[index] ?? "", x + CELL / 2, y + CELL / 2 + 5);
    context.textAlign = "left";
  }

  context.fillStyle = soft;
  context.font = "11px system-ui, sans-serif";
  context.fillText("kongzilla.leonid.sh", PAD, height - PAD + 6);

  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob);
      else reject(new Error("the canvas would not turn into a PNG"));
    }, "image/png");
  });
}

/** A file name that says what the picture is of. */
export function imageName(): string {
  const view = state();
  const board = view.boardCards.slice(0, chrome.visible).join("");
  return `kongzilla-${board === "" ? "preflop" : board.toLowerCase()}.png`;
}
