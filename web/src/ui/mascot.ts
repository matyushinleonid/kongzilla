/**
 * The mascot, which nobody is told about.
 *
 * Every tool this size has one thing in it that is there for no reason, and
 * this is that thing: press A and she turns up. She is in the keyboard sheet
 * with everything else, under a heading of her own - a key that does something
 * and is written down nowhere is not a joke, it is a bug somebody will report.
 *
 * The picture is imported rather than fetched by hand so the build owns it: the
 * hosted site gets a fingerprinted file that is only downloaded the first time
 * anybody presses the key, and the single-file build carries it inside itself
 * like everything else it carries. WebP rather than PNG because it is a drawing
 * with soft shading: a quarter of the bytes for a picture nobody can tell from
 * the original.
 */

import mascot from "../assets/anime.webp";

/** How long she takes to arrive and to leave. */
const FADE_MS = 220;

export interface Mascot {
  element: HTMLElement;
  /** Shows her, or sends her away again if she is already here. */
  toggle: () => void;
  /** Sends her away. `true` if there was anything to send away. */
  close: () => boolean;
}

export function createMascot(): Mascot {
  const backdrop = document.createElement("div");
  backdrop.className = "mascot-backdrop";
  backdrop.hidden = true;

  // Built on the first press rather than at start-up: everyone pays for the
  // markup, and nobody should pay for the picture until they ask for it.
  let image: HTMLImageElement | null = null;
  let leaving = 0;

  const close = (): boolean => {
    if (backdrop.hidden) return false;
    backdrop.classList.remove("open");
    // Kept in the tree through the fade, then taken out of the way of clicks.
    leaving = window.setTimeout(() => {
      backdrop.hidden = true;
      leaving = 0;
    }, FADE_MS);
    return true;
  };

  const open = () => {
    if (leaving) {
      window.clearTimeout(leaving);
      leaving = 0;
    }
    if (!image) {
      image = document.createElement("img");
      image.className = "mascot";
      image.src = mascot;
      image.alt = "";
      backdrop.append(image);
    }
    backdrop.hidden = false;
    // A frame between being in the tree and being told to fade in, or the
    // transition has nothing to transition from.
    requestAnimationFrame(() => backdrop.classList.add("open"));
  };

  backdrop.addEventListener("pointerdown", close);

  return {
    element: backdrop,
    toggle: () => {
      if (backdrop.hidden || !backdrop.classList.contains("open")) open();
      else close();
    },
    close,
  };
}
