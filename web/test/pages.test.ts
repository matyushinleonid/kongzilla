/**
 * The written pages.
 *
 * These are plain HTML with no script, so nothing else checks them. The part
 * worth checking is the language graph: `hreflang` is ignored outright unless
 * every page names itself and its counterpart, and the counterpart agrees. That
 * is one typo away from two versions competing for the same result, and it
 * fails silently.
 */

import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, test } from "vitest";

const SITE = "https://kongzilla.leonid.sh";
const publicDir = resolve(import.meta.dirname, "../public");

/** The written pages, and which language each is in. */
const PAGES: Array<{ path: string; lang: "en" | "ru"; alternate: string }> = [
  { path: "/flopzilla-alternative/", lang: "en", alternate: "/ru/analog-flopzilla/" },
  { path: "/ru/analog-flopzilla/", lang: "ru", alternate: "/flopzilla-alternative/" },
  { path: "/what-is-flopzilla/", lang: "en", alternate: "/ru/razbor-diapazonov/" },
  { path: "/ru/razbor-diapazonov/", lang: "ru", alternate: "/what-is-flopzilla/" },
  { path: "/guide/", lang: "en", alternate: "/ru/rukovodstvo/" },
  { path: "/ru/rukovodstvo/", lang: "ru", alternate: "/guide/" },
];

function page(path: string): string {
  return readFileSync(resolve(publicDir, `.${path}index.html`), "utf8");
}

describe("the written pages", () => {
  test.each(PAGES)("$path declares itself and its counterpart", ({ path, lang, alternate }) => {
    const html = page(path);
    const other = lang === "en" ? "ru" : "en";

    expect(html).toContain(`<html lang="${lang}">`);
    expect(html).toContain(`<link rel="canonical" href="${SITE}${path}" />`);
    expect(html).toContain(`hreflang="${lang}" href="${SITE}${path}"`);
    expect(html).toContain(`hreflang="${other}" href="${SITE}${alternate}"`);
    // English is what an unmatched searcher gets, so it is always x-default.
    const fallback = lang === "en" ? path : alternate;
    expect(html).toContain(`hreflang="x-default" href="${SITE}${fallback}"`);
    // And a link a reader can actually click, not only one a crawler can read.
    expect(html).toContain(`href="${alternate}"`);
  });

  test("every page is reachable from the sitemap, with its alternates", () => {
    const sitemap = readFileSync(resolve(publicDir, "sitemap.xml"), "utf8");
    for (const { path, lang, alternate } of PAGES) {
      expect(sitemap).toContain(`<loc>${SITE}${path}</loc>`);
      expect(sitemap).toContain(`hreflang="${lang}" href="${SITE}${path}"`);
      expect(sitemap).toContain(
        `hreflang="${lang === "en" ? "ru" : "en"}" href="${SITE}${alternate}"`,
      );
    }
    expect(sitemap).toContain(`<loc>${SITE}/</loc>`);
  });

  test("every written page reports its own visit", () => {
    // These are the pages search lands on, so the referrer is worth having; the
    // app cannot report for them because they carry none of its code.
    for (const { path } of PAGES) {
      expect(page(path)).toContain('<script src="/pages.js" defer></script>');
    }
    const beacon = readFileSync(resolve(publicDir, "pages.js"), "utf8");
    expect(beacon).toContain("/api/event");
    // Same guard as the app: a page opened from disk does not call home.
    expect(beacon).toContain('location.protocol !== "https:"');
  });

  test("no page is left out of the language graph", () => {
    // A page added to public/ without a counterpart is the failure this catches.
    const found: string[] = [];
    const walk = (directory: string, prefix: string) => {
      for (const entry of readdirSync(resolve(publicDir, directory), { withFileTypes: true })) {
        if (entry.isDirectory()) walk(`${directory}/${entry.name}`, `${prefix}${entry.name}/`);
        else if (entry.name === "index.html") found.push(prefix);
      }
    };
    walk(".", "/");
    expect(found.sort()).toEqual(PAGES.map((entry) => entry.path).sort());
  });

  test("the Russian pages say things the English ones do not", () => {
    // The point of writing them rather than translating them: they answer a
    // different audience. If this ever becomes a rendering of the English, these
    // are the first things that would disappear.
    const ru = PAGES.filter((entry) => entry.lang === "ru")
      .map((entry) => page(entry.path))
      .join("\n");
    const en = PAGES.filter((entry) => entry.lang === "en")
      .map((entry) => page(entry.path))
      .join("\n");

    // The comparison a Russian-speaking player actually wants: not Flopzilla
    // alone, but the tools they already know by name.
    for (const rival of ["Equilab", "Power-Equilab", "PokerRanger", "oRanges"]) {
      expect(ru).toContain(rival);
      expect(en).not.toContain(rival);
    }
    // And headings that are not a one-to-one rendering of the English ones.
    const headings = (html: string) => html.match(/<h2>([^<]+)<\/h2>/g) ?? [];
    expect(headings(ru).length).not.toBe(headings(en).length);
  });

  test("the Russian pages use the terms Russian players actually use", () => {
    const all = PAGES.filter((entry) => entry.lang === "ru")
      .map((entry) => page(entry.path))
      .join("\n");
    // The vernacular, not a dictionary rendering of the English.
    for (const term of [
      "диапазон",
      "борд",
      "эквити",
      "флеш-дро",
      "гатшот",
      "топ-пара",
      "опен-рейз",
      "постфлоп",
      "комбо",
      "солвер",
    ]) {
      expect(all).toContain(term);
    }
    // Positions stay in the Latin the tables are written in.
    for (const position of ["UTG", "CO", "BTN", "SB", "BB"]) {
      expect(all).toContain(position);
    }
  });
});
