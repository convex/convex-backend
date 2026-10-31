/** Size of the compiled wasm and its biggest sections. */
export const WASM_SIZE_WARN_BYTES = 20 * 1024 * 1024;

export interface WasmSizes {
  total: number;
  code: number;
  data: number;
  names: number;
  other: number;
}

function readLeb(bytes: Uint8Array, pos: number): [number, number] {
  let result = 0;
  let shift = 0;
  while (pos < bytes.length) {
    const b = bytes[pos++];
    result += (b & 0x7f) * 2 ** shift;
    if ((b & 0x80) === 0) return [result, pos];
    shift += 7;
  }
  return [result, pos]; // truncated input
}

/** Section sizes (payload bytes) from the binary format; no instantiation. */
export function wasmSectionSizes(wasm: Uint8Array): WasmSizes {
  const sizes: WasmSizes = {
    total: wasm.length,
    code: 0,
    data: 0,
    names: 0,
    other: 0,
  };
  let pos = 8;
  const decoder = new TextDecoder();
  while (pos < wasm.length) {
    const id = wasm[pos++];
    let size: number;
    [size, pos] = readLeb(wasm, pos);
    const end = pos + size;
    if (id === 10) sizes.code += size;
    else if (id === 11) sizes.data += size;
    else if (id === 0) {
      const [nameLen, namePos] = readLeb(wasm, pos);
      const name = decoder.decode(wasm.subarray(namePos, namePos + nameLen));
      if (name === "name") sizes.names += size;
      else sizes.other += size;
    } else sizes.other += size;
    pos = end;
  }
  return sizes;
}

export function formatBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${n} B`;
}

export function describeWasmSize(
  wasm: Uint8Array,
  warnBytes: number = WASM_SIZE_WARN_BYTES,
): { message: string; warning?: string } {
  const s = wasmSectionSizes(wasm);
  const message =
    `Rust wasm: ${formatBytes(s.total)} ` +
    `(code ${formatBytes(s.code)}, data ${formatBytes(s.data)}, ` +
    `names ${formatBytes(s.names)}, other ${formatBytes(s.other)})`;
  if (s.total <= warnBytes) return { message };
  return {
    message,
    warning:
      `The Rust wasm is ${formatBytes(s.total)}, above the ${formatBytes(warnBytes)} threshold. ` +
      `Largest section: ${s.code >= s.data ? "code" : "data"}. ` +
      `Consider fewer/lighter dependencies, \`opt-level = "z"\`, \`lto = true\`, or installing \`wasm-opt\` (binaryen).`,
  };
}
