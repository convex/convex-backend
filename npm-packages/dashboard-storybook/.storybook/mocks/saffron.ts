// Storybook-only mock for `saffron` (WebAssembly).
// The real module bundles a `.wasm` that Vite can't currently handle in our Storybook build.
export class WasmCron {
  static parseAndDescribe(_expr: string): [WasmCron, string] {
    // Keep behavior deterministic for stories/tests without pulling in WASM.
    return [new WasmCron(), "Cron schedule"];
  }

  free(): void {
    // no-op
  }
}
