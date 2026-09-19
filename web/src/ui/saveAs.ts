/**
 * Saving a file under a name the reader chose.
 *
 * Two browsers, two answers. Chrome and Edge have a real save dialog behind
 * `showSaveFilePicker`, and that is the right thing: it is the operating
 * system's own window, it remembers the last folder, and the name is already
 * filled in. Firefox and Safari have no such call - all they offer is a
 * download, which lands in the downloads folder under whatever name the page
 * asked for and never asks the reader anything. (Firefox can be told to ask,
 * under "Always ask you where to save files", but that is a setting on their
 * side and most people have never touched it.)
 *
 * So where there is no dialog the app puts up its own: one field, the suggested
 * name in it, and the extension left alone while the reader types over the part
 * that is theirs to choose. It is not the operating system's window, but it
 * answers the same question, which is the point.
 */

interface FilePicker extends Window {
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

export interface FileKind {
  /** What the file is, for the dialog's file-type row. */
  description: string;
  /** Its media type, for the blob and for the picker's filter. */
  mime: string;
  /** Its extension, with the dot. Kept out of the reader's way while they type. */
  extension: string;
}

/**
 * Writes `data` to a file the reader names.
 *
 * Resolves to `false` if they backed out, so the caller can leave its counters
 * and its status line alone.
 */
export async function saveAs(
  data: string | Blob,
  suggestedName: string,
  kind: FileKind,
): Promise<boolean> {
  const picker = (window as FilePicker).showSaveFilePicker;
  if (picker) {
    let handle;
    try {
      handle = await picker.call(window, {
        suggestedName,
        types: [{ description: kind.description, accept: { [kind.mime]: [kind.extension] } }],
      });
    } catch {
      // The only thing that throws here in practice is the reader pressing
      // cancel, and cancelling is not an error worth reporting back to them.
      return false;
    }
    const writable = await handle.createWritable();
    await writable.write(data);
    await writable.close();
    return true;
  }

  const name = await askForName(suggestedName, kind);
  if (name === null) return false;
  download(data instanceof Blob ? data : new Blob([data], { type: kind.mime }), name);
  return true;
}

/** Hands the blob to the browser's downloader under `name`. */
function download(blob: Blob, name: string): void {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  URL.revokeObjectURL(url);
}

/** The stand-in dialog. Resolves to the name, or `null` if they backed out. */
function askForName(suggested: string, kind: FileKind): Promise<string | null> {
  return new Promise((resolve) => {
    const backdrop = document.createElement("div");
    backdrop.className = "sheet-backdrop";

    const sheet = document.createElement("form");
    sheet.className = "sheet save-sheet";
    sheet.setAttribute("role", "dialog");
    sheet.setAttribute("aria-label", `Save ${kind.description}`);

    const head = document.createElement("div");
    head.className = "sheet-head";
    const title = document.createElement("h2");
    title.textContent = `Save ${kind.description.toLowerCase()}`;
    head.append(title);

    const field = document.createElement("input");
    field.type = "text";
    field.className = "save-name";
    field.value = suggested;
    field.setAttribute("aria-label", "File name");
    field.spellcheck = false;

    const foot = document.createElement("div");
    foot.className = "save-actions";
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "btn";
    cancel.textContent = "Cancel";
    const confirm = document.createElement("button");
    confirm.type = "submit";
    confirm.className = "btn quick";
    confirm.textContent = "Save";
    foot.append(cancel, confirm);

    sheet.append(head, field, foot);
    backdrop.append(sheet);
    document.body.append(backdrop);

    const done = (name: string | null) => {
      backdrop.remove();
      resolve(name);
    };

    sheet.addEventListener("submit", (event) => {
      event.preventDefault();
      const typed = field.value.trim();
      if (typed === "") return;
      // A name without the extension is still the name they meant.
      done(typed.toLowerCase().endsWith(kind.extension) ? typed : typed + kind.extension);
    });
    cancel.addEventListener("click", () => done(null));
    backdrop.addEventListener("pointerdown", (event) => {
      if (event.target === backdrop) done(null);
    });
    // Escape closes this and nothing else: the app's own Escape works from
    // inside a field, and without this it would shut something behind us.
    sheet.addEventListener("keydown", (event) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      done(null);
    });

    field.focus();
    // Everything but the extension, so typing replaces the name and keeps the
    // ".png" the reader was never asked to think about.
    const stem = suggested.toLowerCase().endsWith(kind.extension)
      ? suggested.length - kind.extension.length
      : suggested.length;
    field.setSelectionRange(0, stem);
  });
}
