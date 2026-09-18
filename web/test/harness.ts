/** Boots the compiled engine for tests, outside a browser. */

import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import init, { Engine } from "../wasm/kongzilla_wasm.js";
import type { View } from "../src/types";

let ready: Promise<void> | null = null;

/**
 * The compiled module, read from disk.
 *
 * Resolved against the working directory rather than `import.meta.url`, because
 * under the jsdom environment that URL is an http one and `readFile` refuses it.
 */
export function wasmBytes(): Promise<Buffer> {
  return readFile(resolve(process.cwd(), "wasm/kongzilla_wasm_bg.wasm"));
}

/** Loads the WebAssembly module once per process. */
export function loadEngine(): Promise<void> {
  ready ??= (async () => {
    await init({ module_or_path: await wasmBytes() });
  })();
  return ready;
}

/** The raw WebAssembly module, for tests that drive it directly. */
export async function newEngine(): Promise<Engine> {
  await loadEngine();
  return new Engine();
}

/** The view model an engine currently reports. */
export function view(engine: Engine): View {
  return JSON.parse(engine.view()) as View;
}

/** A statistic's registry index, by key. */
export function statIndex(key: string): number {
  const defs = JSON.parse(Engine.statDefinitions()) as Array<{ key: string; index: number }>;
  const found = defs.find((def) => def.key === key);
  if (!found) throw new Error(`no statistic called ${key}`);
  return found.index;
}
