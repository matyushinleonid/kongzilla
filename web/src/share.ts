/**
 * Shareable links.
 *
 * The whole session travels in the URL fragment, so a spot can be pasted into a
 * forum post or a chat the way Flopzilla's forum text blocks were, with no server
 * and no account.
 */

const PREFIX = "#s=";

function toBase64Url(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  bytes.forEach((byte) => {
    binary += String.fromCharCode(byte);
  });
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(text: string): string {
  const padded = text.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(padded + "=".repeat((4 - (padded.length % 4)) % 4));
  const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

/** Reads a session out of the current URL, if there is one. */
export function readHash(): string | null {
  if (!location.hash.startsWith(PREFIX)) return null;
  try {
    return fromBase64Url(location.hash.slice(PREFIX.length));
  } catch {
    return null;
  }
}

/** Writes a session into the URL without adding a history entry. */
export function writeHash(json: string): void {
  const hash = PREFIX + toBase64Url(json);
  if (location.hash !== hash) {
    history.replaceState(null, "", hash);
  }
}

/** The full shareable link for the current session. */
export function shareLink(json: string): string {
  return `${location.origin}${location.pathname}${PREFIX}${toBase64Url(json)}`;
}
