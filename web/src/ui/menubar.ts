import { track } from "../analytics";
import { chrome, mutate, restore, runPreflop, snapshot, state } from "../store";
import { shareLink } from "../share";
import { imageName, rangeImage } from "./rangeImage";
import { saveAs } from "./saveAs";
import { cycleTheme, themeLabel } from "./theme";

export function createMenuBar(): {
  element: HTMLElement;
  render: () => void;
  actions: Record<string, () => void>;
} {
  const bar = document.createElement("header");
  bar.className = "menubar";

  const logo = document.createElement("img");
  logo.className = "brand-logo";
  // Served from public/ rather than imported: TypeScript resolves a relative
  // import to the real file and then refuses its extension.
  logo.src = "./logo.png";
  logo.alt = "";
  logo.width = 24;
  logo.height = 24;

  const brand = document.createElement("span");
  brand.className = "brand";
  brand.textContent = "Kongzilla";
  const tagline = document.createElement("span");
  tagline.className = "tagline";
  tagline.textContent = "Hold'em range analysis";

  // Beside the name rather than among the actions on the right: it is not
  // something to press while working, it is who made the thing. It goes with
  // the tagline, and disappears with it when the window is too narrow for
  // anything but the controls.
  const byline = document.createElement("a");
  byline.className = "tagline byline";
  byline.href = "https://leonid.sh";
  byline.textContent = "by Leonid Matyushin";
  byline.title = "The rest of what I build";

  const spacer = document.createElement("span");
  spacer.className = "spacer";

  const bdfd = document.createElement("label");
  bdfd.className = "check";
  const bdfdInput = document.createElement("input");
  bdfdInput.type = "checkbox";
  bdfd.append(bdfdInput, document.createTextNode("1-card backdoor FD"));
  bdfd.title = "Report one-card backdoor flushdraws on two-flush flops";
  bdfdInput.addEventListener("change", () => {
    // A pass over the flops was classified with this setting as it stood, so
    // changing it retires the pass - every row of it would otherwise be a
    // number worked out under the other rule.
    //
    // Retiring it and stopping there emptied the whole panel and put the
    // button back, which reads as the checkbox having broken something. The
    // reader had already asked for this pass; changing how its hands are
    // classified is the same question again, so it is asked again for them.
    const had = chrome.preflop !== null;
    mutate((engine) => engine.setOneCardBackdoorFlushdraw(bdfdInput.checked));
    if (had && chrome.preflop === null && !chrome.preflopRunning) runPreflop();
  });

  const theme = document.createElement("button");
  theme.type = "button";
  theme.className = "btn";
  theme.className = "btn theme-toggle";
  theme.title = "Colour scheme: follow the system, or pin light or dark";
  theme.setAttribute("aria-label", "Colour scheme");
  theme.addEventListener("click", cycleTheme);

  const copyLink = action("Copy link", async () => {
    await navigator.clipboard.writeText(shareLink(snapshot()));
    track("copy_link");
  });
  const copyRange = action("Copy range", async () => {
    await navigator.clipboard.writeText(state().players[state().active].notation);
    track("copy_range");
  });
  const save = action("Save", async () => {
    const name = `kongzilla-${new Date().toISOString().slice(0, 10)}.json`;
    const saved = await saveAs(snapshot(), name, {
      description: "Kongzilla session",
      mime: "application/json",
      extension: ".json",
    });
    if (saved) track("session_saved");
  });
  // A link carries the session for anyone who has the app; a picture is for
  // everyone else - a forum post, a chat, a hand history.
  const image = action("Image", async () => {
    const saved = await saveAs(await rangeImage(), imageName(), {
      description: "PNG image",
      mime: "image/png",
      extension: ".png",
    });
    if (saved) track("image_saved");
  });
  const load = action("Load", () => picker.click());

  const picker = document.createElement("input");
  picker.type = "file";
  picker.accept = "application/json";
  picker.hidden = true;
  picker.addEventListener("change", async () => {
    const file = picker.files?.[0];
    if (file) restore(await file.text());
    picker.value = "";
  });

  /*
   * Full screen, which is the browser's own rather than a mode of ours.
   *
   * The app already comes down to whatever height it is given - the matrix has
   * a ceiling the window sets - so this is not how it is made to fit. It is
   * how a reader gets the browser's chrome and the operating system's bar back
   * as working room, which on a laptop is a hundred and fifty pixels and a row
   * of statistics.
   */
  const full = document.createElement("button");
  full.type = "button";
  full.className = "btn";
  full.addEventListener("click", async () => {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await document.documentElement.requestFullscreen();
    } catch {
      // Refused, or not on offer. The caption is put right by the change
      // event, which does not fire, so it is put right here instead.
      renderFull();
    }
  });
  const renderFull = () => {
    // Truthiness rather than a comparison with null: a browser that does not
    // do this at all leaves the property undefined, and `undefined !== null`
    // had the button offering a way out of something nobody was in.
    const on = Boolean(document.fullscreenElement);
    full.textContent = on ? "Exit full screen" : "Full screen";
    full.title = on ? "Give the browser its chrome back" : "Use the whole screen";
    full.setAttribute("aria-pressed", String(on));
  };
  renderFull();
  document.addEventListener("fullscreenchange", renderFull);

  const keys = document.createElement("button");
  keys.type = "button";
  keys.className = "btn";
  keys.textContent = "Hotkeys";
  keys.title = "Keyboard shortcuts (?)";
  keys.setAttribute("aria-label", keys.title);
  keys.addEventListener("click", () => {
    // The same press the keyboard handles, so there is one path to the sheet.
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "?", bubbles: true }));
  });

  // The written pages are static HTML and carry the search terms the app
  // cannot; linking them from here is what makes them findable at all.
  //
  // The label is deliberately dull. They are written for search engines rather
  // than for somebody mid-hand, and a label that promised help would send
  // readers out of the app to find none. It still has to read as an ordinary
  // link to a crawler, so it says what is there - pages of prose - rather than
  // what they are for.
  const guide = document.createElement("a");
  guide.className = "btn source-link";
  guide.href = "/guide/";
  guide.textContent = "Notes";
  guide.title = "Written pages about ranges and board texture";

  const source = document.createElement("a");
  source.className = "btn source-link";
  source.href = "https://github.com/matyushinleonid/kongzilla";
  source.target = "_blank";
  source.rel = "noopener noreferrer";
  source.title = "matyushinleonid/kongzilla - star it on GitHub";
  source.innerHTML =
    '<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">' +
    '<path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38' +
    " 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53" +
    " .63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95" +
    " 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.42 7.42 0 0 1 2-.27c.68 0 1.36.09" +
    " 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95" +
    '.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z"/>' +
    "</svg><span>Star</span>";

  bar.append(
    logo,
    brand,
    tagline,
    byline,
    spacer,
    bdfd,
    theme,
    copyRange,
    copyLink,
    save,
    load,
    image,
    full,
    keys,
    guide,
    source,
    picker,
  );

  const render = () => {
    bdfdInput.checked = state().options.oneCardBackdoorFlushdraw;
    theme.textContent = themeLabel();
  };

  return {
    element: bar,
    render,
    // The keyboard presses these rather than repeating what they do, so a key
    // and its button can never end up meaning different things.
    actions: {
      save: () => save.click(),
      load: () => load.click(),
      copyRange: () => copyRange.click(),
      copyLink: () => copyLink.click(),
      image: () => image.click(),
      keys: () => keys.click(),
    },
  };
}

/** Which key presses each title-bar button, for its tooltip. */
const KEYS: Record<string, string> = {
  "Copy range": "C",
  "Copy link": "L",
  Save: "Ctrl+S",
  Load: "Ctrl+O",
  Image: "Ctrl+P",
};

function action(label: string, run: () => void | Promise<void>): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "btn";
  button.textContent = label;
  if (KEYS[label]) button.title = `${label} (${KEYS[label]})`;
  button.addEventListener("click", async () => {
    const original = button.textContent;
    try {
      await run();
      button.textContent = "Done";
    } catch (error) {
      // Dismissing a file dialog is not a failure.
      if ((error as DOMException)?.name === "AbortError") return;
      button.textContent = "Failed";
    }
    window.setTimeout(() => {
      button.textContent = original;
    }, 1200);
  });
  return button;
}
