/// <reference types="vitest" />
import { defineConfig } from "vite";

import type { Plugin } from "vite";

import { singleFile } from "./standalone";

/**
 * Serves `/guide/` the way the real server does.
 *
 * The written pages live in `public/` as `guide/index.html`, and nginx resolves
 * a trailing slash to the index inside. The dev server does not, so every one of
 * those pages 404s locally while working in production - which is the worst way
 * round for a link to be broken.
 */
function directoryIndexes(): Plugin {
  return {
    name: "kongzilla-directory-indexes",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use((request, _response, next) => {
        const [path, query] = (request.url ?? "/").split("?");
        if (path !== "/" && path.endsWith("/")) {
          request.url = `${path}index.html${query ? `?${query}` : ""}`;
        }
        next();
      });
    },
  };
}

export default defineConfig(({ mode }) => {
  // `vite build --mode standalone` produces one HTML file that runs from disk.
  const standalone = mode === "standalone";
  return {
    base: "./",
    // The beacon is part of the hosted site and no part of the file people keep.
    // Folding the constant at build time is what removes it, rather than a runtime
    // check that would still ship the endpoint.
    define: { __ANALYTICS__: JSON.stringify(!standalone) },
    // The single file carries its own icons as data URIs, so nothing is copied.
    publicDir: standalone ? false : "public",
    plugins: standalone ? [singleFile(import.meta.dirname)] : [directoryIndexes()],
    build: {
      target: "es2022",
      outDir: standalone ? "dist-standalone" : "dist",
      emptyOutDir: true,
      assetsInlineLimit: standalone ? Number.MAX_SAFE_INTEGER : 4096,
      cssCodeSplit: !standalone,
      modulePreload: standalone ? false : undefined,
      rollupOptions: standalone ? { output: { inlineDynamicImports: true } } : {},
    },
    server: {
      host: true,
      port: 5173,
      watch: { usePolling: true },
    },
    test: {
      include: ["test/**/*.test.ts"],
      environment: "node",
      restoreMocks: true,
    },
  };
});
