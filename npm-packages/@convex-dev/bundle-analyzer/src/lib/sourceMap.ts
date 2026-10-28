import path from "path";
import { fileURLToPath } from "url";
import fs from "fs";

export type SourceMapBytes = {
  /** The map's `sources`, as written. */
  sources: string[];
  /** Bytes of the map attributable to each entry of `sources`. */
  bytesBySource: number[];
  /**
   * Everything else: `names`, other fields, JSON punctuation, and mappings
   * segments that don't point at a source.
   */
  unattributedBytes: number;
};

const BASE64 =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_DIGITS = new Int8Array(128).fill(-1);
for (let i = 0; i < BASE64.length; i++) {
  BASE64_DIGITS[BASE64.charCodeAt(i)] = i;
}

/**
 * Splits a source map's UTF-8 bytes between its sources. Source i gets the
 * bytes of `sources[i]` and `sourcesContent[i]` as JSON (quotes and escapes
 * included) and, for each `mappings` segment whose source index is i, the
 * segment's characters plus the separator that follows it, where the source
 * index is the running sum of the second field of every segment with at least
 * four fields. The byte counts sum to the map's length. A map that isn't a
 * JSON object is entirely unattributed.
 *
 * Elements are measured by re-encoding them, so the counts are exact only when
 * the map escapes strings the way esbuild does: an all-ASCII map is assumed to
 * write every non-ASCII character as `\uXXXX`, as esbuild does by default, and
 * any other map to write them unescaped.
 */
export function sourceMapBytes(map: string): SourceMapBytes {
  const totalBytes = Buffer.byteLength(map);
  let parsed: unknown;
  try {
    parsed = JSON.parse(map);
  } catch {
    parsed = null;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return { sources: [], bytesBySource: [], unattributedBytes: totalBytes };
  }
  const { sources, sourcesContent, mappings } = parsed as Record<
    string,
    unknown
  >;
  const sourceList: unknown[] = Array.isArray(sources) ? sources : [];
  const jsonBytes = /[\u0080-\uffff]/.test(map)
    ? utf8JsonBytes
    : asciiJsonBytes;
  const bytesBySource = sourceList.map(jsonBytes);
  if (Array.isArray(sourcesContent)) {
    sourcesContent.forEach((content, i) => {
      if (i < bytesBySource.length) {
        bytesBySource[i] += jsonBytes(content);
      }
    });
  }
  if (typeof mappings === "string") {
    attributeMappings(mappings, bytesBySource);
  }
  const attributed = bytesBySource.reduce((n, b) => n + b, 0);
  return {
    sources: sourceList.map((s) => (typeof s === "string" ? s : "")),
    bytesBySource,
    unattributedBytes: totalBytes - attributed,
  };
}

function utf8JsonBytes(value: unknown): number {
  return Buffer.byteLength(JSON.stringify(value));
}

function asciiJsonBytes(value: unknown): number {
  return JSON.stringify(value).replace(/[\u0080-\uffff]/g, "\\uXXXX").length;
}

/** `mappings` is all ASCII, so string lengths are byte counts. */
function attributeMappings(mappings: string, bytesBySource: number[]) {
  let sourceIndex = 0;
  for (const [segment, body] of mappings.matchAll(/([^,;]*)[,;]?/g)) {
    if (body.length === 0) {
      continue;
    }
    const fields = decodeSegment(body);
    if (fields.length >= 4) {
      sourceIndex += fields[1];
      if (sourceIndex >= 0 && sourceIndex < bytesBySource.length) {
        bytesBySource[sourceIndex] += segment.length;
      }
    }
  }
}

function decodeSegment(segment: string): number[] {
  const fields: number[] = [];
  let value = 0;
  let shift = 0;
  for (let i = 0; i < segment.length; i++) {
    const c = segment.charCodeAt(i);
    const digit = c < 128 ? BASE64_DIGITS[c] : -1;
    if (digit < 0) {
      return [];
    }
    value += (digit & 31) * 2 ** shift;
    if (digit & 32) {
      shift += 5;
      continue;
    }
    fields.push(value % 2 === 1 ? -Math.floor(value / 2) : value / 2);
    value = 0;
    shift = 0;
  }
  return fields;
}

export type ModuleSourceMapAttribution = {
  /** Keyed by metafile input path. */
  bytesByInput: Map<string, number>;
  /** Bytes that belong to no source: `names`, JSON structure, unmapped segments. */
  overheadBytes: number;
  /**
   * Bytes for sources that don't resolve to an input, keyed by the resolved
   * source (a path relative to the working directory, or a URL).
   */
  unmatchedBytesBySource: Map<string, number>;
};

/**
 * Attributes a module's source map bytes to the metafile inputs of its
 * output.
 *
 * esbuild writes `sources` relative to the map's directory (`out/`, plus the
 * module's directory), so resolving them gives metafile input paths. When an
 * input has its own source map (e.g. a package's `dist/index.js` built from
 * `src/index.ts`), esbuild composes it and lists the input's original sources
 * instead; `originalSources` returns those, as paths relative to the working
 * directory, so they can be traced back to the input.
 */
export function attributeModuleSourceMap(
  map: string,
  modulePath: string,
  outputInputs: string[],
  originalSources: (input: string) => string[],
): ModuleSourceMapAttribution {
  const { sources, bytesBySource, unattributedBytes } = sourceMapBytes(map);
  const mapDir = path.posix.dirname(`out/${modulePath}`);
  const resolved = sources.map((s) => resolveMapSource(mapDir, s));
  const inputSet = new Set(outputInputs);
  const inputBySource = new Map<string, string>();
  for (const r of resolved) {
    if (inputSet.has(r)) {
      inputBySource.set(r, r);
    }
  }
  if (resolved.some((r) => !inputBySource.has(r))) {
    for (const input of outputInputs) {
      if (inputBySource.has(input)) {
        continue;
      }
      for (const original of originalSources(input)) {
        if (!inputBySource.has(original)) {
          inputBySource.set(original, input);
        }
      }
    }
  }
  const bytesByInput = new Map<string, number>();
  const unmatchedBytesBySource = new Map<string, number>();
  resolved.forEach((r, i) => {
    const input = inputBySource.get(r);
    if (input === undefined) {
      unmatchedBytesBySource.set(
        r,
        (unmatchedBytesBySource.get(r) ?? 0) + bytesBySource[i],
      );
    } else {
      bytesByInput.set(
        input,
        (bytesByInput.get(input) ?? 0) + bytesBySource[i],
      );
    }
  });
  return {
    bytesByInput,
    overheadBytes: unattributedBytes,
    unmatchedBytesBySource,
  };
}

/** Namespaced paths like `wasm-stub:foo` are left as they are. */
function resolveMapSource(mapDir: string, source: string): string {
  if (isUrlSource(source)) {
    return source;
  }
  return path.posix.normalize(path.posix.join(mapDir, source));
}

/** A source with a scheme, such as `webpack://` or `wasm-stub:`, but not a Windows drive path. */
function isUrlSource(source: string): boolean {
  return /^[\w-]+:/.test(source) && !/^[a-zA-Z]:[\\/]/.test(source);
}

/**
 * Normalizes a URL source the way esbuild does when it composes an input's
 * map, so `webpack://pkg/./src/a.ts` becomes `webpack://pkg/src/a.ts`. Only the
 * path after an `//authority` is normalized, and `..` never climbs above its
 * root; sources without an authority are left as they are.
 */
function removeDotSegments(url: string): string {
  const match = url.match(/^([\w-]+:\/\/[^/]*)(.*)$/s);
  if (match === null) {
    return url;
  }
  const [, origin, pathname] = match;
  const out: string[] = [];
  for (const segment of pathname.split("/")) {
    if (segment === ".") {
      continue;
    }
    if (segment === "..") {
      if (out.length > 1) {
        out.pop();
      }
      continue;
    }
    out.push(segment);
  }
  return origin + out.join("/");
}

/**
 * Reads the source maps of bundled input files, which esbuild follows through
 * their `sourceMappingURL` comments (line or block form), to find the files
 * they were compiled from.
 */
export class InputSourceMaps {
  private cache = new Map<string, string[]>();

  constructor(private root: string) {}

  /** Original sources of a metafile input, relative to the root, in posix form. */
  originalSources(input: string): string[] {
    let result = this.cache.get(input);
    if (result === undefined) {
      result = this.read(input);
      this.cache.set(input, result);
    }
    return result;
  }

  private read(input: string): string[] {
    if (/^[\w-]+:/.test(input) || input.includes("(disabled):")) {
      return [];
    }
    try {
      const file = path.resolve(this.root, input);
      const url = lastSourceMappingUrl(fs.readFileSync(file, "utf8"));
      if (url === null) {
        return [];
      }
      let mapText: string;
      let mapDir: string;
      if (url.startsWith("data:")) {
        mapText = decodeDataUrl(url);
        mapDir = path.dirname(file);
      } else if (/^https?:/.test(url)) {
        return [];
      } else {
        const mapFile = url.startsWith("file:")
          ? fileURLToPath(url)
          : path.resolve(path.dirname(file), decodeURIComponent(url));
        mapText = fs.readFileSync(mapFile, "utf8");
        mapDir = path.dirname(mapFile);
      }
      const map = JSON.parse(mapText);
      if (!Array.isArray(map.sources)) {
        return [];
      }
      const sourceRoot =
        typeof map.sourceRoot === "string" ? map.sourceRoot : "";
      return map.sources
        .filter((s: unknown): s is string => typeof s === "string")
        .map((s: string) => {
          const joined = sourceRoot
            ? `${sourceRoot.replace(/\/?$/, "/")}${s}`
            : s;
          if (isUrlSource(joined) && !joined.startsWith("file:")) {
            return removeDotSegments(joined);
          }
          const abs = joined.startsWith("file:")
            ? fileURLToPath(joined)
            : path.resolve(mapDir, joined);
          return path.relative(this.root, abs).split(path.sep).join("/");
        });
    } catch {
      return [];
    }
  }
}

function lastSourceMappingUrl(source: string): string | null {
  let url: string | null = null;
  for (const match of source.matchAll(
    /\/\/[#@] sourceMappingURL=(\S+)|\/\*[#@] sourceMappingURL=(\S+?)\s*\*\//g,
  )) {
    url = match[1] ?? match[2];
  }
  return url;
}

function decodeDataUrl(url: string): string {
  const comma = url.indexOf(",");
  const header = url.slice(0, comma);
  const data = url.slice(comma + 1);
  return header.endsWith(";base64")
    ? Buffer.from(data, "base64").toString("utf8")
    : decodeURIComponent(data);
}
