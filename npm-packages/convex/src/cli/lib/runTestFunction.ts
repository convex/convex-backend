import type { Context } from "../../bundler/context.js";
import { deploymentFetch } from "./utils/utils.js";

const QUERY_MODULE_PREAMBLE =
  'import { query, internalQuery } from "convex:/_system/repl/wrappers.js";';

/** Shared help text for the query/module string (CLI argument + MCP input). */
export const RUN_ONEOFF_QUERY_SOURCE_DESCRIPTION =
  'JavaScript module source for a single file (testQuery.js) that exports a default readonly query, for example: export default query({ handler: async (ctx) => ({ count: (await ctx.db.query("messages").take(10)).length }) });';

export const INLINE_QUERY_DESCRIPTION = [
  "JavaScript to evaluate as a readonly query. The query is completely sandboxed, so it can only read data and cannot modify the database or access the network.",
  "",
  "This is a one-shot query and cannot be combined with `--watch`.",
  "Use `--component` to target a mounted component.",
  "",
  "To format the query:",
  '• Simple expressions are returned automatically, for example: `await ctx.db.query("messages").take(5)`.',
  '• For multi-statement queries, use an explicit return, for example: `const firstMessage = await ctx.db.query("messages").first(); console.log(firstMessage?._id); return firstMessage;`.',
  '• For full control, pass a module source that exports a default query, for example: `export default query({ handler: async (ctx) => { return await ctx.db.query("messages").take(10); } })`.',
].join("\n");

export type RunTestFunctionQuerySuccess = {
  kind: "success";
  value: unknown;
  logLines: string[];
};

export type RunTestFunctionQueryApplicationFailure = {
  kind: "applicationFailure";
  payload: unknown;
};

export type RunTestFunctionQueryResult =
  | RunTestFunctionQuerySuccess
  | RunTestFunctionQueryApplicationFailure;

export function inlineQueryToQuerySource(inlineQuery: string) {
  const trimmedQuery = inlineQuery.trim();
  if (looksLikeQueryModuleSource(trimmedQuery)) {
    return injectQueryModulePreamble(trimmedQuery);
  }

  const queryBody = inlineQueryBody(trimmedQuery);
  return `${QUERY_MODULE_PREAMBLE}

export default query({
  handler: async (ctx) => {
${indent(queryBody, 4)}
  },
});`;
}

/**
 * POST /api/run_test_function with the same body shape as the dashboard and MCP.
 * Uses deploymentFetch for Convex-Client, auth headers, retries, and error typing.
 * On HTTP failure, throws ThrowingFetchError (from deploymentFetch).
 */
export async function runTestFunctionQuery(
  ctx: Context,
  args: {
    deploymentUrl: string;
    adminKey: string;
    querySource: string;
    componentId?: string;
  },
): Promise<RunTestFunctionQueryResult> {
  const fetchDeployment = deploymentFetch(ctx, {
    deploymentUrl: args.deploymentUrl,
    adminKey: args.adminKey,
  });
  const response = await fetchDeployment("/api/run_test_function", {
    method: "POST",
    body: JSON.stringify({
      adminKey: args.adminKey,
      args: {},
      bundle: {
        path: "testQuery.js",
        source: args.querySource,
      },
      format: "convex_encoded_json",
      ...(args.componentId !== undefined
        ? { componentId: args.componentId }
        : {}),
    }),
  });
  const result: unknown = await response.json();
  if (
    typeof result !== "object" ||
    result === null ||
    !("status" in result) ||
    (result as { status: string }).status !== "success"
  ) {
    return { kind: "applicationFailure", payload: result };
  }
  const ok = result as {
    status: "success";
    value: unknown;
    logLines?: string[];
  };
  return {
    kind: "success",
    value: ok.value,
    logLines: ok.logLines ?? [],
  };
}

function looksLikeQueryModuleSource(querySource: string) {
  if (!querySource.includes("export default")) return false;
  return /\b(?:query|internalQuery)\s*\(/.test(querySource);
}

function injectQueryModulePreamble(querySource: string) {
  if (querySource.includes("convex:/_system/repl/wrappers.js"))
    return querySource;
  return `${QUERY_MODULE_PREAMBLE}

${querySource}`;
}

function inlineQueryBody(inlineQuery: string) {
  const trimmed = inlineQuery.trim();
  if (!isExpression(trimmed)) return trimmed;
  return `return (${trimmed.replace(/;$/, "")});`;
}

function isExpression(inlineQuery: string) {
  if (inlineQuery.includes("\n")) return false;
  return !/^(const|let|var|if|for|while|switch|try|throw|return)\b/.test(
    inlineQuery,
  );
}

function indent(text: string, spaces: number) {
  const prefix = " ".repeat(spaces);
  return text
    .split("\n")
    .map((line) => `${prefix}${line}`)
    .join("\n");
}
