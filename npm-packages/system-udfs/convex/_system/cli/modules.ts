import { Doc } from "../../_generated/dataModel";
import { UdfType, Visibility } from "../frontend/common";
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
