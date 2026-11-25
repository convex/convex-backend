import { Doc } from "../../_generated/dataModel";
import { PaginationResult, paginationOptsValidator } from "convex/server";
import { UdfType, Visibility } from "../frontend/common";
import { maximumBytesRead, maximumRowsRead } from "../paginationLimits";
import { queryPrivateSystem } from "../secretSystemTables";
import { v, Value, jsonToConvex } from "convex/values";

/**
 * A function in the API spec. `Validator` is how its argument and return
 * validators are represented, e.g. parsed into a `Value`.
 */
type FunctionSpec<Validator> = {
  identifier: string;
  functionType: UdfType;
  visibility: Visibility;
  args: Validator;
  returns: Validator;
};

type HttpFunctionSpec = {
  functionType: "HttpAction";
  method: string;
  path: string;
};

type ApiSpecEntry<Validator> = FunctionSpec<Validator> | HttpFunctionSpec;

export const DEFAULT_ARGS_VALIDATOR = '{ "type": "any" }';
export const DEFAULT_RETURN_VALIDATOR = '{ "type": "any" }';

/**
 * The API spec entries of one module. `parseValidator` receives each
 * validator's JSON as stored on the module.
 */
function moduleApiSpec<Validator>(
  module: Doc<"_modules">,
  parseValidator: (validatorJson: string) => Validator,
): ApiSpecEntry<Validator>[] {
  const analyzeResult = module.analyzeResult;
  if (!analyzeResult) {
    return [];
  }
  const entries: ApiSpecEntry<Validator>[] = [];
  for (const fn of analyzeResult.functions || []) {
    entries.push({
      identifier: module.path + ":" + fn.name,
      functionType: fn.udfType,
      visibility: fn.visibility ?? { kind: "public" },
      args: parseValidator(fn.args ?? DEFAULT_ARGS_VALIDATOR),
      returns: parseValidator(fn.returns ?? DEFAULT_RETURN_VALIDATOR),
    });
  }
  for (const httpFn of analyzeResult.httpRoutes || []) {
    entries.push({
      functionType: "HttpAction",
      method: httpFn.route.method,
      path: httpFn.route.path,
    });
  }
  return entries;
}

export const apiSpec = queryPrivateSystem("ViewData")({
  args: {
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async ({ db }): Promise<ApiSpecEntry<Value>[]> => {
    const result: ApiSpecEntry<Value>[] = [];
    for await (const module of db.query("_modules")) {
      result.push(
        ...moduleApiSpec(module, (json) => jsonToConvex(JSON.parse(json))),
      );
    }
    return result;
  },
});

// Validators stay as the JSON stored on the module. Returning them unparsed
// keeps the per-function cost low enough to fit large pages under the query
// time limit.
type PaginatedApiSpecResult = PaginationResult<ApiSpecEntry<string>> & {
  // Opaque token that identifies the deployed functions. Every push changes
  // it, so a client that sees it change between pages knows its earlier pages
  // are stale and must start over. `null` if no functions were ever pushed.
  version: string | null;
};

/**
 * Paginated version of `apiSpec`, paging over modules.
 *
 * Each page reads its own snapshot, so callers combining pages must restart
 * when `version` changes.
 */
export const paginatedApiSpec = queryPrivateSystem("ViewData")({
  args: {
    componentId: v.optional(v.union(v.string(), v.null())),
    paginationOpts: paginationOptsValidator,
  },
  handler: async (
    { db },
    { paginationOpts },
  ): Promise<PaginatedApiSpecResult> => {
    // `_source_packages` is not in the system UDF schema.
    const latestSourcePackage = await (db as any)
      .query("_source_packages")
      .order("desc")
      .first();
    const { page: modules, ...rest } = await db
      .query("_modules")
      .paginate({ ...paginationOpts, maximumRowsRead, maximumBytesRead });

    return {
      ...rest,
      page: modules.flatMap((module) => moduleApiSpec(module, (json) => json)),
      version: latestSourcePackage
        ? await sha256Hex(latestSourcePackage._id)
        : null,
    };
  },
});

// Hashes the source package id so that `version` stays opaque to clients.
async function sha256Hex(value: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(value),
  );
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}
