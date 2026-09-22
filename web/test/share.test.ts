// @vitest-environment jsdom
/**
 * The link somebody sends you.
 *
 * A session travels in the fragment of a URL, which is the only part of an
 * address a browser does not send anywhere - so a spot can be pasted into a
 * forum post or a chat with no server and no account behind it. That makes the
 * encoder load-bearing in a way nothing else here is: the session round-trips
 * through base64 and back, and everything about the app that the reader wanted
 * to show somebody is inside it.
 *
 * The journeys next door check `snapshot` and `restore`, which is the middle of
 * that path. These check the ends: what goes into the address, what comes back
 * out of it, and what happens when what comes out is rubbish.
 */

import { afterEach, beforeAll, beforeEach, describe, expect, test } from "vitest";

import { readHash, shareLink, writeHash } from "../src/share";
import { boot, chrome, mutate, resetChrome, snapshot, state } from "../src/store";
import { wasmBytes } from "./harness";

let bytes: Buffer;

beforeAll(async () => {
  bytes = await wasmBytes();
});

beforeEach(() => {
  window.location.hash = "";
  resetChrome();
});

afterEach(() => {
  window.location.hash = "";
});

/** Starts the app as a fresh visit does, with whatever is in the address. */
async function visit(hash = ""): Promise<void> {
  window.location.hash = hash;
  await boot(bytes);
}

describe("a session in the address", () => {
  test("goes out and comes back the same", () => {
    const session = '{"version":1,"board":"Kh 7h 2c","players":[{"notation":"22+, AJs+"}]}';
    writeHash(session);
    expect(location.hash.startsWith("#s=")).toBe(true);
    // URL-safe, because a `+` in a fragment is a space to somebody's chat
    // client and a `/` is a path to somebody's forum. The payload, not the
    // whole fragment: the `#s=` in front of it has an equals sign of its own.
    expect(location.hash.slice("#s=".length)).not.toMatch(/[+\/=]/);
    expect(readHash()).toBe(session);
  });

  test("survives what base64 finds awkward", () => {
    // Padding is what a hand-rolled decoder gets wrong: the encoder drops it and
    // the decoder has to work out how much to put back, and it is wrong for
    // exactly two lengths in three.
    for (let length = 1; length <= 12; length += 1) {
      const session = "x".repeat(length);
      writeHash(session);
      expect(readHash(), `a session of ${length} characters`).toBe(session);
    }
    // And a range can be named in any language the reader types in.
    const named = '{"name":"Диапазон А — 2♦️"}';
    writeHash(named);
    expect(readHash()).toBe(named);
  });

  test("a link is the address plus the session", () => {
    const session = '{"version":1,"board":""}';
    const link = shareLink(session);
    expect(link.startsWith(`${location.origin}${location.pathname}#s=`)).toBe(true);
    // The link somebody pastes is the address somebody else lands on.
    window.location.hash = new URL(link).hash;
    expect(readHash()).toBe(session);
  });

  test("editing does not fill the back button", () => {
    // `replaceState`, not `pushState`: the session is written into the address
    // on every change, and a reader who painted twenty hands would otherwise
    // have to press back twenty times to leave the page.
    const before = history.length;
    for (const board of ["Kh 7h 2c", "Kh 7h 2d", "Kh 7h 2s"]) {
      writeHash(JSON.stringify({ version: 1, board }));
    }
    expect(history.length).toBe(before);
  });

  test("nothing in the address is not an error", () => {
    window.location.hash = "";
    expect(readHash()).toBeNull();
    window.location.hash = "#something-else";
    expect(readHash()).toBeNull();
    // Something that is meant to be a session and is not decodes to nothing
    // rather than throwing, because a link is a thing strangers hand you.
    window.location.hash = "#s=not%20base64!!";
    expect(readHash()).toBeNull();
  });
});

describe("opening someone else's link", () => {
  test("lands on their board, their range and their painting", async () => {
    // Build a session the way a reader does, then read what the address holds.
    await visit();
    mutate((engine) => {
      engine.setBoard("Kh 7h 2c");
      engine.setRangeText("22+, AJs+");
    });
    const sent = shareLink(snapshot());
    const board = state().board;
    const notation = state().players[state().active].notation;
    expect(location.hash).toBe(new URL(sent).hash);

    // Somebody else, opening it cold: a new engine, nothing saved, and the
    // address is all they have.
    await visit(new URL(sent).hash);
    expect(state().board).toBe(board);
    expect(state().players[state().active].notation).toBe(notation);
    // The board is on screen as cards, not just in the engine: the chrome that
    // draws them is filled in from the restored session too.
    expect(chrome.boardCards).toEqual(["Kh", "7h", "2c"]);
    expect(chrome.visible).toBe(3);
  });

  test("a link that has been mangled starts the app rather than stopping it", async () => {
    await visit("#s=Y29tcGxldGUgbm9uc2Vuc2U");
    // A fresh session, not a broken one: the opening range and no board.
    expect(state().board).toBe("");
    expect(state().players[state().active].notation.length).toBeGreaterThan(0);

    // Truncated in the middle of a word, which is what a chat client that wraps
    // a long line hands back.
    await visit();
    mutate((engine) => engine.setRangeText("22+"));
    const whole = new URL(shareLink(snapshot())).hash;
    await visit(whole.slice(0, whole.length - 12));
    expect(state().players[state().active].notation.length).toBeGreaterThan(0);
  });
});
