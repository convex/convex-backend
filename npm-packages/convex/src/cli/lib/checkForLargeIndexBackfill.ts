import { chalkStderr } from "chalk";
import { Context } from "../../bundler/context.js";
import {
  changeSpinner,
  logFinishedStep,
  logMessage,
  stopSpinner,
} from "../../bundler/log.js";
import { formatIndex } from "./indexes.js";
import { promptYesNo } from "./utils/prompts.js";
import { Span } from "./tracing.js";
import {
  EvaluateSchemaResponse,
  IndexPrediction,
} from "./deployApi/evaluateSchema.js";
import { SchemaEvaluation } from "./schemaEvaluation.js";
import { integerFromEnv } from "./utils/utils.js";

// Same size as the large index deletion check, so both index guards fire at
// the same table size. Separate env override so tests can trigger one
// without the other.
const MIN_DOCUMENTS_FOR_INDEX_BACKFILL_WARNING = 100_000;

export type LargeIndexBackfillCheck =
  // Don’t check whether the push creates indexes on large tables
  | "no verification"
  // If it does, ask for confirmation (and fail in non-interactive envs)
  | "ask for confirmation"
  // If it does, proceed (the user has used --skip-large-indexes-check)
  | "has confirmation";

type BlockingIndex = {
  // The component instance path ("" for the root component), matching the
  // keys of `componentSchemaEvaluations`.
  componentPath: string;
  index: IndexPrediction;
};

/**
 * Warn — and on a real deploy require confirmation — when a push creates or
 * enables a non-staged index on a large table. `wait_for_schema` blocks the
 * deploy until every such index is backfilled, which can take a long time on
 * large tables; staging the index instead backfills it in the background.
 *
 * Skipped silently when the deployment doesn't support `evaluate_schema`:
 * the check protects the developer's time, not correctness.
 */
export async function checkForLargeIndexBackfill({
  ctx,
  span,
  schemaEvaluation,
  options,
  mode,
}: {
  ctx: Context;
  span: Span;
  schemaEvaluation: SchemaEvaluation;
  options: { url: string };
  // "warn" prints the warning without prompting, for dry runs.
  mode: "warn" | "ask for confirmation" | "has confirmation";
}): Promise<void> {
  changeSpinner("Verifying that the push isn’t creating large indexes...");
  const response = await schemaEvaluation(span);
  if (response === null) {
    return;
  }

  const blockingIndexes = blockingIndexesFromSchemaEvaluation(
    response,
    minDocumentsForIndexBackfillWarning(),
  );
  if (blockingIndexes.length === 0) {
    return;
  }

  const plural = blockingIndexes.length !== 1;
  logMessage(`⚠️  This push will ${chalkStderr.bold("create")} the following ${plural ? "indexes on large tables" : "index on a large table"}
in your deployment (${options.url}). The deploy will block until ${plural ? "they finish" : "it finishes"} backfilling:

${blockingIndexes.map(formatBlockingIndex).join("\n")}

Tip: stage the index (e.g. \`.index("by_field", { fields: ["field"], staged: true })\`) to backfill
it in the background without blocking the deploy, then remove \`staged\` in a later push once it’s ready.
Learn more at https://docs.convex.dev/database/reading-data/indexes#staged-indexes
`);

  if (mode === "warn") {
    return;
  }

  if (mode === "has confirmation") {
    logFinishedStep(
      "Proceeding with push since --skip-large-indexes-check is set",
    );
    return;
  }

  if (!process.stdin.isTTY) {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `To confirm the push:
• run the deploy command in an ${chalkStderr.bold("interactive terminal")}
• or run the deploy command with the ${chalkStderr.bold("--skip-large-indexes-check")} flag`,
    });
  }

  stopSpinner();
  if (
    !(await promptYesNo(ctx, {
      message: `Create ${plural ? "these indexes" : "this index"} now?`,
      default: false,
    }))
  ) {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Canceling push`,
    });
  }

  logFinishedStep("Proceeding with push.");
}

// An index blocks the deploy when this push starts (`added`) or waits on
// (`enabled` before its staged backfill finished) a backfill that isn't
// staged. Added staged indexes report `needsBackfill` too but never block,
// and an in-progress backfill an earlier push started (`identical`) isn't
// this push's doing.
export function blockingIndexesFromSchemaEvaluation(
  response: EvaluateSchemaResponse,
  minDocuments: number,
): BlockingIndex[] {
  return Object.entries(response.componentSchemaEvaluations).flatMap(
    ([componentPath, prediction]) =>
      prediction.indexes
        .filter(
          (index) =>
            index.needsBackfill &&
            !index.staged &&
            (index.change === "added" || index.change === "enabled") &&
            index.numDocs >= minDocuments,
        )
        .map((index) => ({ componentPath, index })),
  );
}

function formatBlockingIndex({ componentPath, index }: BlockingIndex) {
  const componentNameFormatted =
    componentPath !== "" ? `${chalkStderr.gray(componentPath)}:` : "";
  const documentsCountFormatted = chalkStderr.yellowBright(
    `⚠️  ${index.numDocs.toLocaleString()} documents`,
  );
  const enabledNote =
    index.change === "enabled"
      ? `\n   ${chalkStderr.gray("enabled before its staged backfill finished")}`
      : "";
  return (
    "⛔ " +
    componentNameFormatted +
    formatIndex(index) +
    "  " +
    documentsCountFormatted +
    enabledNote
  );
}

function minDocumentsForIndexBackfillWarning(): number {
  return integerFromEnv(
    "CONVEX_MIN_DOCUMENTS_FOR_INDEX_BACKFILL_WARNING",
    MIN_DOCUMENTS_FOR_INDEX_BACKFILL_WARNING,
  );
}
