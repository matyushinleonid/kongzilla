/**
 * The engine bytes, when the page was built as a single file.
 *
 * A standalone build has nothing to fetch, so it carries the WebAssembly module
 * inline and hands the bytes to `boot()`. A hosted build returns nothing here
 * and the loader fetches the `.wasm` next to the script, as usual.
 */
export function embeddedWasm(): Uint8Array | undefined {
  const encoded = (globalThis as { __KONGZILLA_WASM__?: string }).__KONGZILLA_WASM__;
  if (!encoded) return undefined;
  const binary = atob(encoded);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}
