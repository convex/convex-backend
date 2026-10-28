import { z } from "zod";
import { componentDefinitionPath, componentPath } from "./paths.js";
import { nodeDependency, sourcePackage } from "./modules.js";
import { checkedComponent } from "./checkedComponent.js";
import { evaluatedComponentDefinition } from "./componentDefinition.js";
import {
  appDefinitionConfig,
  componentDefinitionConfig,
} from "./definitionConfig.js";
import { authInfo } from "./types.js";
import { looseObject } from "./utils.js";
import { indexDiff } from "./finishPush.js";

export const startPushRequest = looseObject({
  adminKey: z.string(),
  dryRun: z.boolean(),

  functions: z.string(),

  appDefinition: appDefinitionConfig,
  componentDefinitions: z.array(componentDefinitionConfig),

  nodeDependencies: z.array(nodeDependency),

  nodeVersion: z.optional(z.string()),
});
export type StartPushRequest = z.infer<typeof startPushRequest>;

export const schemaChange = looseObject({
  allocatedComponentIds: z.any(),
  schemaIds: z.any(),
  indexDiffs: z.record(componentPath, indexDiff).optional(),
});
export type SchemaChange = z.infer<typeof schemaChange>;

export const startPushResponse = looseObject({
  environmentVariables: z.record(z.string(), z.string()),

  externalDepsId: z.nullable(z.string()),
  componentDefinitionPackages: z.record(componentDefinitionPath, sourcePackage),

  appAuth: z.array(authInfo),
  analysis: z.record(componentDefinitionPath, evaluatedComponentDefinition),

  app: checkedComponent,

  schemaChange,
});
export type StartPushResponse = z.infer<typeof startPushResponse>;

// Values this short are rarely secrets, and replacing them everywhere would
// garble unrelated strings in the output.
const MIN_SCRUBBED_VALUE_LENGTH = 8;

/**
 * Replaces every occurrence of an environment variable's value in `node`'s
 * strings with `<redacted>`. Values reappear in push responses wherever the
 * code read them, such as auth providers built from `process.env` in
 * `auth.config.ts`.
 */
export function redactEnvironmentVariableValues<T>(
  node: T,
  environmentVariables: Record<string, string>,
): T {
  const values = Object.values(environmentVariables)
    .filter((value) => value.length >= MIN_SCRUBBED_VALUE_LENGTH)
    // Strings that hold JSON, like `authDiff` entries, contain the escaped form.
    .flatMap((value) => {
      const escaped = JSON.stringify(value).slice(1, -1);
      return escaped === value ? [value] : [value, escaped];
    })
    .sort((a, b) => b.length - a.length);
  const scrub = (child: unknown): unknown => {
    if (typeof child === "string") {
      return values.reduce(
        (scrubbed, value) => scrubbed.split(value).join("<redacted>"),
        child,
      );
    }
    if (Array.isArray(child)) {
      return child.map(scrub);
    }
    if (child !== null && typeof child === "object") {
      return Object.fromEntries(
        Object.entries(child).map(([key, grandchild]) => [
          key,
          scrub(grandchild),
        ]),
      );
    }
    return child;
  };
  return scrub(node) as T;
}

/**
 * A copy of the response that is safe to print. `environmentVariables` holds
 * every value on the deployment so `finish_push` can detect concurrent changes,
 * and those values are often secrets.
 */
export function startPushResponseForLogging(
  response: StartPushResponse,
): StartPushResponse {
  return {
    ...redactEnvironmentVariableValues(response, response.environmentVariables),
    environmentVariables: Object.fromEntries(
      Object.keys(response.environmentVariables).map((name) => [
        name,
        "<redacted>",
      ]),
    ),
  };
}

export const evaluatePushResponse = looseObject({
  schemaChange,
});
export type EvaluatePushResponse = z.infer<typeof evaluatePushResponse>;

export const componentSchemaStatus = looseObject({
  schemaValidationComplete: z.boolean(),
  indexesComplete: z.number(),
  indexesTotal: z.number(),
});
export type ComponentSchemaStatus = z.infer<typeof componentSchemaStatus>;

export const schemaStatus = z.union([
  looseObject({
    type: z.literal("inProgress"),
    components: z.record(componentPath, componentSchemaStatus),
  }),
  looseObject({
    type: z.literal("failed"),
    error: z.string(),
    componentPath,
    tableName: z.nullable(z.string()),
  }),
  looseObject({
    type: z.literal("raceDetected"),
  }),
  looseObject({
    type: z.literal("complete"),
  }),
]);
export type SchemaStatus = z.infer<typeof schemaStatus>;
