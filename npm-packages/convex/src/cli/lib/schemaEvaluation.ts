import { Context } from "../../bundler/context.js";
import { logVerbose } from "../../bundler/log.js";
import { EvaluateSchemaResponse } from "./deployApi/evaluateSchema.js";
import { StartPushRequest } from "./deployApi/startPush.js";
import { evaluateSchema } from "./deploy2.js";
import { Span } from "./tracing.js";

type EvaluateOptions = {
  url: string;
  deploymentName: string | null;
  adminKey: string;
};

/**
 * A schema evaluation shared by the pre-push checks. Resolves to `null` when
 * the deployment doesn't support `evaluate_schema` or the request failed, in
 * which case each check skips itself.
 */
export type SchemaEvaluation = (
  span: Span,
) => Promise<EvaluateSchemaResponse | null>;

/**
 * Evaluate the schema at most once per push however many checks consume it:
 * the first check to call sends the request under its own span, and the rest
 * await the same promise. Each request makes the backend evaluate the schema
 * module in an isolate and predict against a fresh snapshot, so repeating it
 * per check would add that work and a round trip to the deploy's critical
 * path for every check.
 */
/**
 * Whether a schema module in the push calls `.staged(`. Bundles aren't
 * minified, so the call survives bundling. A false positive from a comment or
 * string costs one evaluation, while a push without staged validators skips
 * the evaluation round trip on its critical path.
 */
export function pushStagesValidators(request: StartPushRequest): boolean {
  return [
    request.appDefinition.schema,
    ...request.componentDefinitions.map((definition) => definition.schema),
  ].some((schema) => schema?.source.includes(".staged(") ?? false);
}

export function sharedSchemaEvaluation(
  ctx: Context,
  request: StartPushRequest,
  options: EvaluateOptions,
): SchemaEvaluation {
  let evaluation: Promise<EvaluateSchemaResponse | null> | undefined;
  return (span) => {
    evaluation ??= evaluateSchemaBestEffort({ ctx, span, request, options });
    return evaluation;
  };
}

// The checks are advisory: if evaluation fails, fall back to skipping them
// rather than aborting the push over a check that isn't required for
// correctness. Plausibly transient failures — network errors and 5xx
// responses (e.g. table summaries still bootstrapping) — are retried with
// backoff by the shared fetch wrapper (see `pushCode`); anything else is
// deterministic, including a 2xx whose shape the CLI doesn't recognize, so
// it skips the checks immediately.
async function evaluateSchemaBestEffort({
  ctx,
  span,
  request,
  options,
}: {
  ctx: Context;
  span: Span;
  request: StartPushRequest;
  options: EvaluateOptions;
}): Promise<EvaluateSchemaResponse | null> {
  try {
    return await evaluateSchema(
      ctx,
      span,
      request,
      options,
      /* bestEffort */ true,
    );
  } catch (error: unknown) {
    logVerbose(
      `Skipping schema evaluation checks: ${error instanceof Error ? error.message : String(error)}`,
    );
    return null;
  }
}
