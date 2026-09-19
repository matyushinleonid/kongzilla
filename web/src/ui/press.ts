/**
 * Telling a press apart from a scroll, and holding apart from both.
 *
 * A mouse says what it means the moment it goes down: it cannot scroll the page
 * by pressing, so pressing is pressing. A finger cannot say - the same touch
 * begins a tap and begins a swipe, and which one it was is only known once it
 * moves or lets go. The app used to answer on the way down and take the gesture
 * for itself, which is why a finger landing on the matrix could not scroll the
 * page: the whole grid was a wall.
 *
 * So the browser decides. `touch-action` in the stylesheet says which gestures
 * it may keep, and when it takes one it sends `pointercancel` - the app's cue
 * that the touch was never a press. What is left, a touch that goes down and
 * comes up without the browser claiming it, is a tap.
 *
 * Holding is the third thing, and it stands in for the shift key: there is no
 * shift on a touchscreen, and the things it reaches - the suit breakdown, the
 * combinations behind a row - are exactly the ones a reader wants to look into
 * rather than change. Hold, and you get what shift-clicking gives.
 */

/** How long a finger has to stay put before it means "look into this". */
const HOLD_MS = 450;

/** How far it may drift in that time and still count as staying put. */
const HOLD_SLOP = 10;

export interface PressHandlers {
  /** The ordinary press: a click, or a tap the browser did not take. */
  act?: (event: PointerEvent) => void;
  /** Shift-click, or a finger held still. Left out where there is nothing under it. */
  hold?: (event: PointerEvent) => void;
  /** Whether there is anything to do at all, asked at press time. */
  when?: () => boolean;
}

/**
 * Binds the three gestures to one element.
 *
 * Mouse and pen act on the way down, because that is what they have always done
 * and because carrying on into a drag is how a brush works. Touch acts on the
 * way up, because until then it might have been a scroll.
 */
export function press(element: HTMLElement, handlers: PressHandlers): void {
  let waiting: number | null = null;
  let timer = 0;
  let from = { x: 0, y: 0 };
  let held = false;

  const forget = () => {
    if (timer) window.clearTimeout(timer);
    timer = 0;
    waiting = null;
  };

  element.addEventListener("pointerdown", (event) => {
    if (handlers.when && !handlers.when()) return;
    if (event.pointerType !== "touch") {
      // Nothing to work out: a mouse that is down is pressing something. The
      // default is only taken away when there is something here to take it for
      // - an element whose only gesture is a hold still has to be focusable by
      // clicking it.
      const doing = event.shiftKey && handlers.hold ? handlers.hold : handlers.act;
      if (!doing) return;
      event.preventDefault();
      doing(event);
      return;
    }
    // No preventDefault here, on purpose: it would stop the browser starting a
    // scroll, which is the whole complaint this exists to answer.
    held = false;
    waiting = event.pointerId;
    from = { x: event.clientX, y: event.clientY };
    if (!handlers.hold) return;
    timer = window.setTimeout(() => {
      timer = 0;
      if (waiting !== event.pointerId) return;
      held = true;
      // A hold that has fired is done. Letting go must not then also press, so
      // the tap is spent here.
      waiting = null;
      handlers.hold?.(event);
    }, HOLD_MS);
  });

  element.addEventListener("pointermove", (event) => {
    if (waiting !== event.pointerId || timer === 0) return;
    // A finger on its way somewhere is not being held still. The browser may be
    // about to claim the gesture; either way this is no longer a hold.
    const moved =
      Math.abs(event.clientX - from.x) > HOLD_SLOP || Math.abs(event.clientY - from.y) > HOLD_SLOP;
    if (moved) {
      window.clearTimeout(timer);
      timer = 0;
    }
  });

  element.addEventListener("pointerup", (event) => {
    if (waiting !== event.pointerId) return;
    forget();
    handlers.act?.(event);
  });

  // The browser took the gesture for a scroll. Nothing was pressed.
  element.addEventListener("pointercancel", (event) => {
    if (waiting === event.pointerId) forget();
  });

  // A tap ends in a click as well, and after a hold that click would act on top
  // of what the hold already did.
  element.addEventListener(
    "click",
    (event) => {
      if (!held) return;
      held = false;
      event.preventDefault();
      event.stopPropagation();
    },
    true,
  );

  // The long press a browser answers with its own menu has already been
  // answered here.
  element.addEventListener("contextmenu", (event) => {
    if (held || timer !== 0) event.preventDefault();
  });
}

/** Whether this reader is working with a touchscreen rather than a pointer. */
export function touchOnly(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;
  return window.matchMedia("(hover: none) and (pointer: coarse)").matches;
}
