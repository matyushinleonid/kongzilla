/**
 * The single-file build.
 *
 * The app is entirely client-side, but a browser will not load an ES module or
 * a `.wasm` over `file://` - the origin is opaque and the fetch is refused. So
 * a double-clicked `dist/index.html` shows nothing. This plugin folds the whole
 * app into one HTML file that has nothing left to fetch: the script and the
 * stylesheet go inline, the icons and the logo become `data:` URIs, and the
 * engine rides along as base64 for `boot()` to instantiate from bytes.
 *
 * The result is a file you can email to someone who has never installed
 * anything, and it works offline.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { Plugin } from "vite";

/** The global the built page reads its engine out of. See src/standalone.ts. */
export const WASM_GLOBAL = "__KONGZILLA_WASM__";

/** Where a page on someone's desktop points when it needs the site. */
const SITE = "https://kongzilla.leonid.sh";

const MIME: Record<string, string> = {
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
};

function dataUri(path: string): string {
  const extension = path.slice(path.lastIndexOf("."));
  const type = MIME[extension] ?? "application/octet-stream";
  return `data:${type};base64,${readFileSync(path).toString("base64")}`;
}

export function singleFile(root: string): Plugin {
  return {
    name: "kongzilla-single-file",
    enforce: "post",
    apply: "build",

    // Public assets are copied, never inlined, so the references to them have to
    // be rewritten by hand wherever they appear - in the HTML and in the script.
    transform(code, id) {
      if (!/\.(ts|js)$/.test(id)) return null;
      let updated = code.replace(
        '"./logo.png"',
        JSON.stringify(dataUri(resolve(root, "public/logo.png"))),
      );
      // The written pages sit next to the app on the site; a file on someone's
      // desktop has no siblings, so its links point at the site itself.
      updated = updated.replace(
        /"(\/(?:guide|flopzilla-alternative|what-is-flopzilla)\/)"/g,
        (_match, path: string) => JSON.stringify(`${SITE}${path}`),
      );
      return updated === code ? null : { code: updated, map: null };
    },

    generateBundle(_options, bundle) {
      const html = Object.values(bundle).find(
        (file) => file.type === "asset" && file.fileName.endsWith(".html"),
      );
      if (!html || html.type !== "asset") {
        this.error("the single-file build produced no HTML");
      }

      let source = String(html.source);

      for (const file of Object.values(bundle)) {
        if (file.type === "chunk" && file.isEntry) {
          const script = `<script type="module">\n${file.code}\n</script>`;
          source = source.replace(
            new RegExp(`<script[^>]*src="[^"]*${file.fileName}"[^>]*></script>`),
            () => script,
          );
        }
        if (file.type === "asset" && file.fileName.endsWith(".css")) {
          source = source.replace(
            new RegExp(`<link[^>]*href="[^"]*${file.fileName}"[^>]*>`),
            () => `<style>\n${String(file.source)}\n</style>`,
          );
        }
      }

      // Anything still pointing at a file on disk would be a blank tab or a
      // missing icon, so fail the build rather than ship a half-working page.
      source = source.replace(/(?:\.\/)?(favicon|apple-touch-icon|logo)\.png/g, (_match, name) =>
        dataUri(resolve(root, `public/${name}.png`)),
      );
      const engine = readFileSync(resolve(root, "wasm/kongzilla_wasm_bg.wasm")).toString("base64");
      source = source.replace(
        "</head>",
        `  <script>window.${WASM_GLOBAL} = "${engine}";</script>\n  </head>`,
      );
      // Links out to the web are fine - those are the user's own choice to
      // click. A reference to a file on disk is not: it would be a blank tab or
      // a missing icon once the HTML is on its own.
      const onDisk = source.match(/(?:src|href)="(?!data:|https?:|mailto:|#)[^"]+"/g);
      if (onDisk) {
        this.error(`the single-file build left ${onDisk.length} local reference(s): ${onDisk[0]}`);
      }
      // The file people keep on their desktop must not be able to call home, so
      // check the beacon really was folded out rather than trusting that it was.
      for (const trace of ["/api/event", "sendBeacon"]) {
        if (source.includes(trace)) {
          this.error(`the single-file build still contains ${trace}`);
        }
      }

      html.source = source;
      // Everything the page used is inside it now, so nothing else ships.
      for (const name of Object.keys(bundle)) {
        if (name !== html.fileName) delete bundle[name];
      }
    },
  };
}
