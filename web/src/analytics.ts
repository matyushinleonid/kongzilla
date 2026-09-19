/**
 * Telling the beacon that something happened.
 *
 * The app is a static page with no back end, so the only thing that can say
 * whether anyone is using it is the page itself. This reports a handful of named
 * events to a first-party endpoint; the counting, and the forgetting, happen
 * there. No identifier is stored here and nothing is read back.
 *
 * `__ANALYTICS__` is a build-time constant. The standalone build sets it false,
 * which folds every function below to an empty body and takes the endpoint, the
 * queue and the event names out of the bundle - a file somebody keeps on their
 * desktop has no business calling home, and a guard that could be edited back in
 * is not the same as code that is not there.
 */

declare const __ANALYTICS__: boolean;

/**
 * The written pages report too, so the Russian ones can be told apart from the
 * English. The beacon keeps the same list; a path missing from it is counted,
 * but as `other`.
 */

/** Where reports go. Same origin, so no adblocker and no third party. */
const ENDPOINT = "/api/event";

/** Names the beacon knows. Anything else is refused there, so keep them in step. */
export type Event =
  | "pageview"
  | "flop_dealt"
  | "chart_loaded"
  | "street_filter"
  | "paint_top"
  | "board_cleared"
  | "copy_link"
  | "copy_range"
  | "session_saved"
  | "image_saved"
  | "preflop_run"
  | "flop_group_picked";

/** Events already sent this session, for the ones only worth hearing once. */
const sent = new Set<string>();

/** Whether this page is one that should be reporting at all. */
function live(): boolean {
  if (!__ANALYTICS__) return false;
  if (typeof window === "undefined") return false;
  // A page opened from a file, or served from a dev machine, is not a visit.
  return window.location.protocol === "https:" || window.location.hostname === "localhost";
}

/** Reports one event. Never throws, never waits, never blocks a click. */
export function track(name: Event, once = false): void {
  if (!__ANALYTICS__) return;
  if (!live()) return;
  if (once) {
    if (sent.has(name)) return;
    sent.add(name);
  }
  const body = JSON.stringify(
    name === "pageview"
      ? { name, path: window.location.pathname, referrer: document.referrer }
      : { name },
  );
  try {
    // sendBeacon survives the page being closed, which a fetch does not.
    if (navigator.sendBeacon?.(ENDPOINT, new Blob([body], { type: "application/json" }))) {
      return;
    }
    void fetch(ENDPOINT, {
      method: "POST",
      body,
      headers: { "Content-Type": "application/json" },
      keepalive: true,
    }).catch(() => {});
  } catch {
    // Statistics are never worth an error in front of the reader.
  }
}

/** Reports the visit itself. */
export function startAnalytics(): void {
  if (!__ANALYTICS__) return;
  track("pageview");
}
