/** The title bar: the name, the option toggles and the sharing actions. */

/** The save dialog, where the browser has one. */
interface WindowWithFilePicker extends Window {
  showSaveFilePicker?: (options: {
    suggestedName?: string;
    types?: Array<{ description: string; accept: Record<string, string[]> }>;
  }) => Promise<{
    createWritable: () => Promise<{
      write: (data: string | Blob) => Promise<void>;
      close: () => Promise<void>;
    }>;
  }>;
}

import { track } from "../analytics";
import { mutate, restore, snapshot, state } from "../store";
import { shareLink } from "../share";
import { imageName, rangeImage } from "./rangeImage";
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

  const spacer = document.createElement("span");
  spacer.className = "spacer";

  const bdfd = document.createElement("label");
  bdfd.className = "check";
  const bdfdInput = document.createElement("input");
  bdfdInput.type = "checkbox";
  bdfd.append(bdfdInput, document.createTextNode("1-card backdoor FD"));
  bdfd.title = "Report one-card backdoor flushdraws on two-flush flops";
  bdfdInput.addEventListener("change", () => {
    mutate((engine) => engine.setOneCardBackdoorFlushdraw(bdfdInput.checked));
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
    const json = snapshot();
    const suggestedName = `kongzilla-${new Date().toISOString().slice(0, 10)}.json`;
    // Where browsers offer a real save dialog, use it: naming the file is part of
    // saving. Everywhere else, fall back to a download.
    const picker = (window as WindowWithFilePicker).showSaveFilePicker;
    if (picker) {
      const handle = await picker.call(window, {
        suggestedName,
        types: [{ description: "Kongzilla session", accept: { "application/json": [".json"] } }],
      });
      const writable = await handle.createWritable();
      await writable.write(json);
      await writable.close();
      track("session_saved");
      return;
    }
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = suggestedName;
    link.click();
    URL.revokeObjectURL(url);
    track("session_saved");
  });
  // A link carries the session for anyone who has the app; a picture is for
  // everyone else - a forum post, a chat, a hand history.
  const image = action("Image", async () => {
    const blob = await rangeImage();
    const suggestedName = imageName();
    const filePicker = (window as WindowWithFilePicker).showSaveFilePicker;
    if (filePicker) {
      const handle = await filePicker.call(window, {
        suggestedName,
        types: [{ description: "PNG image", accept: { "image/png": [".png"] } }],
      });
      const writable = await handle.createWritable();
      await writable.write(blob);
      await writable.close();
      track("image_saved");
      return;
    }
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = suggestedName;
    link.click();
    URL.revokeObjectURL(url);
    track("image_saved");
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

  // The written pages are static HTML and carry the search terms the app
  // cannot; linking them from here is what makes them findable at all.
  const keys = document.createElement("button");
  keys.type = "button";
  keys.className = "btn";
  keys.textContent = "?";
  keys.title = "Keyboard shortcuts (?)";
  keys.setAttribute("aria-label", keys.title);
  keys.addEventListener("click", () => {
    // The same press the keyboard handles, so there is one path to the sheet.
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "?", bubbles: true }));
  });

  const guide = document.createElement("a");
  guide.className = "btn source-link";
  guide.href = "/guide/";
  guide.textContent = "Guide";
  guide.title = "How to use Kongzilla";

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
    spacer,
    bdfd,
    theme,
    copyRange,
    copyLink,
    save,
    load,
    image,
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
