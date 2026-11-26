import path from "path";
// eslint-disable-next-line no-restricted-imports
import nodeFs from "fs";
import { Context } from "../../bundler/context.js";
import { TempDir } from "../../bundler/fs.js";
import { logOutput, logVerbose } from "../../bundler/log.js";
import { analyzeTsFunctionsLocally } from "../lib/rust/localAnalysis.js";
import { AnalyzedModule } from "../lib/deployApi/modules.js";
import { ConvexValidator } from "../lib/deployApi/validator.js";
import {
  hasRustFunctions,
  loadRustManifestForCodegen,
  rustApiType,
} from "../lib/rust.js";
import { RUST_API_HEADER, RUST_API_STUB } from "../lib/rust/scaffold.js";
import { compareStrings } from "./common.js";

/**
 * `convex/rust/src/_generated/api.rs`: typed references to every Convex
 * function (TypeScript and Rust), usable from a Rust action as
 *
 * ```rust
 * ctx.run_query(api::messages::list, api::messages::ListArgs {}).await?
 * ```
 *
 * Args and return types come from the function validators (the same ones that
 * type `api.d.ts`); functions without a validator use `serde_json::Value`.
 */

const RELATIVE_PATH = path.join("rust", "src", "_generated", "api.rs");

const RUST_KEYWORDS = new Set([
  "as",
  "break",
  "const",
  "continue",
  "else",
  "enum",
  "extern",
  "false",
  "fn",
  "for",
  "if",
  "impl",
  "in",
  "let",
  "loop",
  "match",
  "mod",
  "move",
  "mut",
  "pub",
  "ref",
  "return",
  "static",
  "struct",
  "trait",
  "true",
  "type",
  "unsafe",
  "use",
  "where",
  "while",
  "async",
  "await",
  "dyn",
  "abstract",
  "become",
  "box",
  "do",
  "final",
  "macro",
  "override",
  "priv",
  "typeof",
  "unsized",
  "virtual",
  "yield",
  "try",
  "gen",
]);
const NON_RAW_KEYWORDS = new Set(["self", "Self", "super", "crate", "_"]);

/** A Rust identifier for `name` (a Convex module, function or field name). */
export function rustIdent(name: string): string {
  let id = name.replace(/[^A-Za-z0-9_]/g, "_");
  if (id === "" || /^[0-9]/.test(id)) id = `_${id}`;
  if (NON_RAW_KEYWORDS.has(id)) return `${id}_`;
  if (RUST_KEYWORDS.has(id)) return `r#${id}`;
  return id;
}

/** `list_messages` / `listMessages` -> `ListMessages`. */
export function pascalCase(name: string): string {
  const parts = name.split(/[^A-Za-z0-9]+/).filter((p) => p !== "");
  const out = parts.map((p) => p[0].toUpperCase() + p.slice(1)).join("");
  return /^[0-9]/.test(out) || out === "" ? `T${out}` : out;
}

function parseValidatorJson(json: string | null): ConvexValidator | null {
  if (!json) return null;
  return JSON.parse(json) as ConvexValidator;
}

interface Generator {
  /** Struct definitions emitted next to the function reference. */
  structs: string[];
}

function derive(): string {
  return `#[derive(Clone, Debug, Serialize, Deserialize)]\n#[serde(crate = "convex_fn::serde")]`;
}

/**
 * Rust type for a validator. Objects become structs named after `hint`
 * (pushed to `gen.structs`); anything the Rust types can't express exactly
 * (general unions, mixed literals) becomes `serde_json::Value`.
 */
export function rustType(
  v: ConvexValidator,
  hint: string,
  gen: Generator,
): string {
  switch (v.type) {
    case "null":
      return "()";
    case "number":
      return "f64";
    case "bigint":
      return "Int64";
    case "boolean":
      return "bool";
    case "string":
    case "id":
      return "String";
    case "bytes":
      return "Bytes";
    case "commitTs":
      return "Int64";
    case "any":
      return "Value";
    case "literal": {
      const value = (v as { value: unknown }).value;
      if (typeof value === "string") return "String";
      if (typeof value === "boolean") return "bool";
      if (typeof value === "number") return "f64";
      return "Value";
    }
    case "array":
      return `Vec<${rustType(v.value, `${hint}Item`, gen)}>`;
    case "record":
      return `BTreeMap<String, ${rustType(v.values.fieldType, `${hint}Value`, gen)}>`;
    case "union": {
      const members = v.value;
      const nonNull = members.filter((m) => m.type !== "null");
      if (members.length === 2 && nonNull.length === 1) {
        return `Option<${rustType(nonNull[0], hint, gen)}>`;
      }
      if (
        members.every(
          (m) => m.type === "literal" && typeof (m as any).value === "string",
        )
      ) {
        return "String";
      }
      return "Value";
    }
    case "object": {
      gen.structs.push(structDef(v, hint, gen));
      return hint;
    }
  }
}

function structDef(
  v: Extract<ConvexValidator, { type: "object" }>,
  name: string,
  gen: Generator,
): string {
  const lines: string[] = [derive(), `pub struct ${name} {`];
  const fields = Object.entries(v.value).sort(([a], [b]) =>
    compareStrings(a, b),
  );
  for (const [fieldName, field] of fields) {
    const ident = rustIdent(fieldName);
    const bare = ident.replace(/^r#/, "");
    const ty = rustType(
      field.fieldType,
      `${name}${pascalCase(fieldName)}`,
      gen,
    );
    if (field.fieldType.type === "id") {
      lines.push(`    /// \`Id<"${field.fieldType.tableName}">\``);
    }
    const attrs: string[] = [];
    if (bare !== fieldName) attrs.push(`rename = ${JSON.stringify(fieldName)}`);
    if (field.optional) {
      attrs.push("default", 'skip_serializing_if = "Option::is_none"');
    }
    if (attrs.length > 0) lines.push(`    #[serde(${attrs.join(", ")})]`);
    lines.push(
      `    pub ${ident}: ${field.optional && !ty.startsWith("Option<") ? `Option<${ty}>` : ty},`,
    );
  }
  lines.push("}");
  return lines.join("\n");
}

interface Node {
  fns: string[]; // rendered const definitions
  structs: string[];
  kids: Map<string, Node>;
}

function newNode(): Node {
  return { fns: [], structs: [], kids: new Map() };
}

function descend(root: Node, modulePath: string): Node {
  let node = root;
  for (const part of modulePath.split("/")) {
    const key = rustIdent(part);
    if (!node.kids.has(key)) node.kids.set(key, newNode());
    node = node.kids.get(key)!;
  }
  return node;
}

function renderFunction(
  node: Node,
  fnPath: string,
  name: string,
  argsJson: string | null,
  returnsJson: string | null,
) {
  const gen: Generator = { structs: [] };
  const base = pascalCase(name);
  let argsType = "Value";
  try {
    const args = parseValidatorJson(argsJson);
    if (args && args.type === "object") {
      argsType = rustType(args, `${base}Args`, gen);
    }
  } catch {
    // Keep `Value`.
  }
  let returnsType = "Value";
  try {
    const returns = parseValidatorJson(returnsJson);
    if (returns) returnsType = rustType(returns, `${base}Returns`, gen);
  } catch {
    // Keep `Value`.
  }
  node.structs.push(...gen.structs);
  node.fns.push(
    `pub const ${rustIdent(name)}: FunctionRef<${argsType}, ${returnsType}> =\n    FunctionRef::new(${JSON.stringify(fnPath)});`,
  );
}

function indent(text: string, depth: number): string {
  const pad = "    ".repeat(depth);
  return text
    .split("\n")
    .map((l) => (l === "" ? l : pad + l))
    .join("\n");
}

function renderNode(node: Node, depth: number): string {
  const parts: string[] = [];
  parts.push(
    indent(
      "use convex_fn::serde::{Deserialize, Serialize};\nuse convex_fn::{Bytes, FunctionRef, Int64};\nuse serde_json::Value;\nuse std::collections::BTreeMap;",
      depth,
    ),
  );
  for (const s of node.structs) parts.push(indent(s, depth));
  for (const f of node.fns) parts.push(indent(f, depth));
  for (const [name, kid] of [...node.kids.entries()].sort(([a], [b]) =>
    compareStrings(a, b),
  )) {
    parts.push(
      `${indent(`pub mod ${name} {`, depth)}\n${renderNode(kid, depth + 1)}\n${indent("}", depth)}`,
    );
  }
  return parts.join("\n\n");
}

/** Contents of `api.rs` for the analyzed modules (`messages.js`, ...). */
export function rustApiCodegen(
  modules: Record<string, AnalyzedModule>,
): string {
  const pub = newNode();
  const internal = newNode();
  const paths = Object.keys(modules).sort(compareStrings);
  for (const modulePath of paths) {
    const moduleName = modulePath.replace(/\.js$/, "");
    for (const f of modules[modulePath].functions) {
      const visibility = f.visibility?.kind;
      if (visibility !== "public" && visibility !== "internal") continue;
      const target = visibility === "public" ? pub : internal;
      renderFunction(
        descend(target, moduleName),
        `${moduleName}:${f.name}`,
        f.name,
        f.args,
        f.returns,
      );
    }
  }
  const out = [RUST_API_HEADER, renderNode(pub, 0)];
  if (internal.kids.size > 0) {
    if (pub.kids.has("internal")) {
      out.push(
        "// `internal` functions are omitted: a module is named `internal`.",
      );
    } else {
      out.push(
        `/// \`internal\` functions (\`internalQuery\`, \`internalMutation\`, \`internalAction\`).\npub mod internal {\n${renderNode(internal, 1)}\n}`,
      );
    }
  }
  return out.join("\n") + "\n";
}

async function writeGenerated(
  ctx: Context,
  tmpDir: TempDir,
  destination: string,
  contents: string,
  opts?: { dryRun?: boolean; debug?: boolean },
  onlyIfMissing = false,
) {
  if (opts?.debug) {
    logOutput(`# ${path.resolve(destination)}`);
    logOutput(contents);
    return;
  }
  if (onlyIfMissing && ctx.fs.exists(destination)) return;
  try {
    if (ctx.fs.readUtf8File(destination) === contents) return;
  } catch (err: any) {
    if (err.code !== "ENOENT") {
      // eslint-disable-next-line no-restricted-syntax
      throw err;
    }
  }
  if (opts?.dryRun) {
    logOutput(`Command would write file: ${destination}`);
    return;
  }
  ctx.fs.mkdir(path.dirname(destination), {
    allowExisting: true,
    recursive: true,
  });
  const tmpPath = tmpDir.writeUtf8File(contents);
  ctx.fs.swapTmpFile(tmpPath, destination);
}

/**
 * Writes `rust/src/_generated/api.rs` when the project has a Rust crate.
 * With `modules == null` (before the first analysis) only an empty stub is
 * written, and only if the file does not exist yet.
 */
export async function doRustApiCodegen(
  ctx: Context,
  tmpDir: TempDir,
  functionsDirectory: string,
  modules: Record<string, AnalyzedModule> | null,
  opts?: { dryRun?: boolean; debug?: boolean },
) {
  if (!hasRustFunctions(ctx, functionsDirectory)) return;
  const destination = path.join(functionsDirectory, RELATIVE_PATH);
  if (modules !== null && !opts?.dryRun && !opts?.debug) {
    writeRustAnalysisCache(functionsDirectory, modules);
  }
  if (modules === null) {
    await writeGenerated(ctx, tmpDir, destination, RUST_API_STUB, opts, true);
  } else {
    await writeGenerated(
      ctx,
      tmpDir,
      destination,
      rustApiCodegen(modules),
      opts,
    );
  }
}

/** Last pushed analysis of the Rust modules (`rust/...`), under `rust/target/`. */
function cachePath(functionsDirectory: string): string {
  return path.join(
    functionsDirectory,
    "rust",
    "target",
    ".convex-rust-modules.json",
  );
}

function writeRustAnalysisCache(
  functionsDirectory: string,
  modules: Record<string, AnalyzedModule>,
) {
  const rustModules: Record<string, AnalyzedModule> = {};
  for (const [k, v] of Object.entries(modules)) {
    if (k.startsWith("rust/")) rustModules[k] = v;
  }
  try {
    const file = cachePath(functionsDirectory);
    nodeFs.mkdirSync(path.dirname(file), { recursive: true });
    nodeFs.writeFileSync(file, JSON.stringify(rustModules));
  } catch (e: any) {
    logVerbose(`Could not write the Rust API cache: ${e?.message ?? e}`);
  }
}

function readRustAnalysisCache(
  functionsDirectory: string,
): Record<string, AnalyzedModule> {
  try {
    return JSON.parse(
      nodeFs.readFileSync(cachePath(functionsDirectory), "utf8"),
    );
  } catch {
    return {};
  }
}

/** Function paths (`messages:list`) already present in an `api.rs`. */
export function functionPathsInApiRs(contents: string): Set<string> {
  const out = new Set<string>();
  for (const m of contents.matchAll(/FunctionRef::new\("([^"]+)"\)/g)) {
    out.add(m[1]);
  }
  return out;
}

/** `module:fn` for each public/internal function of `modules`. */
export function functionPaths(
  modules: Record<string, AnalyzedModule>,
): Set<string> {
  const out = new Set<string>();
  for (const [modulePath, mod] of Object.entries(modules)) {
    for (const f of mod.functions) {
      const kind = f.visibility?.kind;
      if (kind === "public" || kind === "internal") {
        out.add(`${modulePath.replace(/\.js$/, "")}:${f.name}`);
      }
    }
  }
  return out;
}

function rustSourcesReferenceApi(rustDir: string): boolean {
  const visit = (dir: string): boolean => {
    let entries: nodeFs.Dirent[];
    try {
      entries = nodeFs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return false;
    }
    for (const e of entries) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) {
        if (e.name === "_generated") continue;
        if (visit(p)) return true;
      } else if (e.name.endsWith(".rs")) {
        try {
          if (/\bapi\s*::/.test(nodeFs.readFileSync(p, "utf8"))) return true;
        } catch {
          // ignore
        }
      }
    }
    return false;
  };
  return visit(path.join(rustDir, "src"));
}

/**
 * Called right before `cargo build`. `api.rs` normally learns the TS functions
 * from the server's analysis during a push, which happens after the Rust
 * build, so a Rust action calling a brand-new TS function would not compile on
 * the first push. If the Rust sources use `api::` and some TS function is
 * missing from `api.rs`, analyze the TS locally (see localAnalysis.ts) and
 * regenerate `api.rs` with real types, merged with the Rust modules cached from
 * the last push. Does nothing when `api.rs` already knows every function, so a
 * typed file from a previous push is never churned.
 */
export async function prepareRustApiForBuild(
  ctx: Context,
  functionsDirectory: string,
): Promise<void> {
  if (!hasRustFunctions(ctx, functionsDirectory)) return;
  const rustDir = path.join(functionsDirectory, "rust");
  if (!rustSourcesReferenceApi(rustDir)) return;
  const destination = path.join(functionsDirectory, RELATIVE_PATH);
  let existing = "";
  try {
    existing = nodeFs.readFileSync(destination, "utf8");
  } catch {
    // missing: written below
  }
  const local = await analyzeTsFunctionsLocally(ctx, functionsDirectory);
  if (local === null) return;
  const wanted = { ...readRustAnalysisCache(functionsDirectory), ...local };
  const have = functionPathsInApiRs(existing);
  const missing = [...functionPaths(wanted)].filter((p) => !have.has(p));
  if (missing.length === 0) return;
  logVerbose(`Adding ${missing.join(", ")} to rust/src/_generated/api.rs`);
  nodeFs.mkdirSync(path.dirname(destination), { recursive: true });
  nodeFs.writeFileSync(destination, rustApiCodegen(wanted));
}

/**
 * `"use rust"` actions as a TypeScript type for the codegen'd `api`, or null
 * when the project has no Rust crate.
 */
export async function rustApiFor(
  ctx: Context,
  functionsDir: string,
): Promise<string | null> {
  return rustApiType(await loadRustManifestForCodegen(ctx, functionsDir));
}

const FULL_API_END = /(ApiFromModules<\{[\s\S]*?\n\s*\}>)/;

function intersectRustApi(contents: string, rustApi: string): string {
  return contents.replace(FULL_API_END, (m) => `${m} & ${rustApi}`);
}

/**
 * Merge the Rust action references into generated `api` source by
 * intersecting the `ApiFromModules<{...}>` type. Accepts a source string or the
 * `{ DTS, JS, TS }` result of `apiCodegen`. No-op when `rustApi` is null.
 */
export function withRustApi(contents: string, rustApi: string | null): string;
export function withRustApi<
  T extends { DTS?: string; JS?: string; TS?: string },
>(contents: T, rustApi: string | null): T;
export function withRustApi(
  contents: string | { DTS?: string; JS?: string; TS?: string },
  rustApi: string | null,
): any {
  if (!rustApi) return contents;
  if (typeof contents === "string") return intersectRustApi(contents, rustApi);
  return {
    ...contents,
    DTS: contents.DTS && intersectRustApi(contents.DTS, rustApi),
    TS: contents.TS && intersectRustApi(contents.TS, rustApi),
  };
}

/** `_generated/api.rs` for the root component only; `functions` null = stub. */
export async function rustComponentCodegen(
  ctx: Context,
  tmpDir: TempDir,
  componentDirectory: { isRoot: boolean; path: string },
  functions: Parameters<typeof doRustApiCodegen>[3],
  opts?: Parameters<typeof doRustApiCodegen>[4],
): Promise<void> {
  if (!componentDirectory.isRoot) return;
  await doRustApiCodegen(ctx, tmpDir, componentDirectory.path, functions, opts);
}

// ---------------------------------------------------------------------------
// `_generated/rustHttp`: V8 `httpAction`s that run `#[http_action]` Rust functions
// ---------------------------------------------------------------------------

const RUST_HTTP_BODY = `/**
 * Runs an \`#[http_action]\` Rust function (native or backend-hosted wasm runtime) as the handler of
 * an HTTP route:
 *
 *   http.route({ path: "/hook", method: "POST", handler: rustHttpAction("web:hook") });
 *
 * \`name\` is \`<convex module>:<function>\` of the Rust function (\`web:hook\` for \`web::hook\`). The
 * request is passed to the function as an internal action call (head, base64 body), and its
 * \`{status, headers, body}\` answer becomes the Response. The body is buffered both ways (a streaming
 * response is collected and sent whole); the request body has to fit the 16 MiB action argument limit
 * once base64 encoded (about 12 MiB).
 */
`;

const RUST_HTTP_IMPL = `
function toBase64(bytes) {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  }
  return btoa(s);
}

function fromBase64(b64) {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

export function rustHttpAction(name) {
  const ref = makeFunctionReference("rust/" + name);
  return httpActionGeneric(async (ctx, request) => {
    const headers = [];
    request.headers.forEach((value, key) => headers.push([key, value]));
    const bytes = new Uint8Array(await request.arrayBuffer());
    const out = await ctx.runAction(ref, {
      method: request.method,
      url: request.url,
      headers,
      body: bytes.length > 0 ? toBase64(bytes) : null,
      buffered: true,
    });
    if (out === null || typeof out !== "object" || typeof out.status !== "number") {
      throw new Error('The "use rust" HTTP action returned something other than an HttpResponse');
    }
    const responseHeaders = new Headers();
    for (const [k, v] of out.headers ?? []) responseHeaders.append(k, v);
    const noBody = out.status === 204 || out.status === 205 || out.status === 304;
    return new Response(!noBody && typeof out.body === "string" ? fromBase64(out.body) : null, {
      status: out.status,
      headers: responseHeaders,
    });
  });
}
`;

const RUST_HTTP_HEADER = "/* eslint-disable */\n";

/** Contents of \`_generated/rustHttp.{js,d.ts}\` (JS projects) or \`rustHttp.ts\` (TypeScript projects). */
export function rustHttpCodegen(useTypeScript: boolean): {
  JS?: string;
  DTS?: string;
  TS?: string;
} {
  const imports = 'import { httpActionGeneric, makeFunctionReference } from "convex/server";\n';
  if (!useTypeScript) {
    return {
      JS: `${RUST_HTTP_HEADER}${RUST_HTTP_BODY}${imports}${RUST_HTTP_IMPL}`,
      DTS: `${RUST_HTTP_HEADER}import type { PublicHttpAction } from "convex/server";\n\n${RUST_HTTP_BODY}export declare function rustHttpAction(name: string): PublicHttpAction;\n`,
    };
  }
  const ts = RUST_HTTP_IMPL.replace("function toBase64(bytes)", "function toBase64(bytes: Uint8Array)")
    .replace("function fromBase64(b64)", "function fromBase64(b64: string)")
    .replace("export function rustHttpAction(name)", "export function rustHttpAction(name: string)")
    .replace('makeFunctionReference("rust/" + name)', 'makeFunctionReference<"action">("rust/" + name)')
    .replace("const headers = [];", "const headers: [string, string][] = [];")
    .replace("(value, key) => headers.push", "(value: string, key: string) => headers.push")
    .replace("for (const [k, v] of out.headers ?? [])", "for (const [k, v] of (out.headers ?? []) as [string, string][])")
    .replace("bytes.subarray(i, i + 0x8000))", "bytes.subarray(i, i + 0x8000) as unknown as number[])")
    .replace("fromBase64(out.body) : null", "(fromBase64(out.body) as BufferSource) : null");
  return {
    TS: `${RUST_HTTP_HEADER}${RUST_HTTP_BODY}${imports}${ts}`,
  };
}

/**
 * Writes \`_generated/rustHttp\` when the project has a Rust crate; returns the file names written
 * (so stale-file cleanup keeps them).
 */
export async function doRustHttpCodegen(
  ctx: Context,
  tmpDir: TempDir,
  functionsDirectory: string,
  codegenDir: string,
  useTypeScript: boolean,
  opts?: { dryRun?: boolean; debug?: boolean },
): Promise<string[]> {
  if (!hasRustFunctions(ctx, functionsDirectory)) return [];
  const gen = rustHttpCodegen(useTypeScript);
  const files: [string, string][] = [];
  if (gen.JS !== undefined) files.push(["rustHttp.js", gen.JS]);
  if (gen.DTS !== undefined) files.push(["rustHttp.d.ts", gen.DTS]);
  if (gen.TS !== undefined) files.push(["rustHttp.ts", gen.TS]);
  for (const [name, contents] of files) {
    await writeGenerated(ctx, tmpDir, path.join(codegenDir, name), contents, opts);
  }
  return files.map(([name]) => name);
}
