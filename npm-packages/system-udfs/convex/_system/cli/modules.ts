import { UdfType, Visibility } from "../frontend/common";
import { queryPrivateSystem } from "../secretSystemTables";
import { v, Value, jsonToConvex } from "convex/values";

type FunctionSpec = {
  identifier: string;
  functionType: UdfType;
  visibility: Visibility;
  args?: Value;
  returns?: Value;
};

type HttpFunctionSpec = {
  functionType: "HttpAction";
  method: string;
  path: string;
};

type FunctionSpecs = (FunctionSpec | HttpFunctionSpec)[];

export const DEFAULT_ARGS_VALIDATOR = '{ "type": "any" }';
export const DEFAULT_RETURN_VALIDATOR = '{ "type": "any" }';

const MAX_VALIDATOR_NESTING = 64 - 2;
const ANY_VALIDATOR = jsonToConvex(JSON.parse(DEFAULT_RETURN_VALIDATOR));

function validatorNesting(value: Value): number {
  if (Array.isArray(value)) {
    let deepest = 0;
    for (const item of value) {
      deepest = Math.max(deepest, validatorNesting(item));
    }
    return 1 + deepest;
  }
  if (
    typeof value === "object" &&
    value !== null &&
    !(value instanceof ArrayBuffer)
  ) {
    let deepest = 0;
    for (const item of Object.values(value)) {
      if (item !== undefined) {
        deepest = Math.max(deepest, validatorNesting(item));
      }
    }
    return 1 + deepest;
  }
  return 0;
}

function degradeIfTooNested(
  value: Value,
  identifier: string,
  kind: "args" | "returns",
): Value {
  if (validatorNesting(value) <= MAX_VALIDATOR_NESTING) {
    return value;
  }
  // eslint-disable-next-line no-console
  console.warn(
    `${identifier}: ${kind} validator is too deeply nested to include in the ` +
      "function spec; reporting it as `v.any()`.",
  );
  return ANY_VALIDATOR;
}

export const apiSpec = queryPrivateSystem("ViewData")({
  args: {
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async ({ db }): Promise<FunctionSpecs> => {
    const result: FunctionSpecs = [];
    for await (const module of db.query("_modules")) {
      const analyzeResult = module.analyzeResult;
      if (!analyzeResult) {
        // `Skipping ${module.path}`
        continue;
      }
      for (const fn of analyzeResult.functions || []) {
        const argsValidator = fn.args ?? DEFAULT_ARGS_VALIDATOR;
        const returnsValidator = fn.returns ?? DEFAULT_RETURN_VALIDATOR;
        const identifier = module.path + ":" + fn.name;
        result.push({
          identifier,
          functionType: fn.udfType,
          visibility: fn.visibility ?? { kind: "public" },
          args: degradeIfTooNested(
            jsonToConvex(JSON.parse(argsValidator)),
            identifier,
            "args",
          ),
          returns: degradeIfTooNested(
            jsonToConvex(JSON.parse(returnsValidator)),
            identifier,
            "returns",
          ),
        });
      }

      for (const httpFn of analyzeResult.httpRoutes || []) {
        result.push({
          functionType: "HttpAction",
          method: httpFn.route.method,
          path: httpFn.route.path,
        });
      }
    }

    return result;
  },
});
