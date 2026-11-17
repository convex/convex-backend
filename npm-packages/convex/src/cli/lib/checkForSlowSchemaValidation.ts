import { chalkStderr } from "chalk";
import { logMessage } from "../../bundler/log.js";
import { Span } from "./tracing.js";
import {
  EvaluateSchemaResponse,
  TablePrediction,
} from "./deployApi/evaluateSchema.js";
import { SchemaEvaluation } from "./schemaEvaluation.js";
import { formatSize, integerFromEnv } from "./utils/utils.js";
import {
  formatDiscardedStagedValidator,
  formatValidationProgress,
} from "./formatValidationProgress.js";

const MIN_BYTES_FOR_SCHEMA_WALK_WARNING = 1 << 27; // 128 MiB

export async function checkForSlowSchemaValidation({
  span,
  schemaEvaluation,
  dryRun,
}: {
  span: Span;
  schemaEvaluation: SchemaEvaluation;
  dryRun: boolean;
}): Promise<void> {
  const response = await schemaEvaluation(span);
  if (response === null) {
    return;
  }
  reportStagedValidators(response);
  if (!dryRun) {
    return;
  }

  const walkedTables = Object.entries(
    response.componentSchemaEvaluations,
  ).flatMap(([componentPath, prediction]) =>
    prediction.tables
      .filter((table) => table.outcome === "mustWalk")
      .map((table) => ({ componentPath, table })),
  );
  if (walkedTables.length === 0) {
    return;
  }

  const totalBytes = walkedTables.reduce(
    (sum, { table }) => sum + table.sizeBytes,
    0,
  );
  if (totalBytes < minBytesForSchemaWalkWarning()) {
    return;
  }

  const shouldSuggestStaging = walkedTables.some(
    ({ table }) => !table.canSkipAfterStagedValidation,
  );
  logMessage(`⚠️  This schema change requires checking every document in the following ${walkedTables.length === 1 ? "table" : "tables"} against your new schema, totaling ${chalkStderr.yellowBright(formatSize(totalBytes))}. This deploy may take a while:

${walkedTables
  .map(({ componentPath, table }) =>
    formatWalkedTable({ componentPath, table }),
  )
  .join("\n")}
${
  shouldSuggestStaging
    ? `\nTip: stage the new validator (\`defineTable({...}).staged({...})\`) to validate it in the background without blocking deploys.`
    : ""
}`);
}

// The prediction includes staged validators from active and in-flight schemas.
function reportStagedValidators(response: EvaluateSchemaResponse) {
  const stagedTables = Object.entries(
    response.componentSchemaEvaluations,
  ).flatMap(([componentPath, prediction]) => {
    const discarded = new Map(
      (prediction.discardedStagedValidators ?? []).map((validator) => [
        validator.tableName,
        validator,
      ]),
    );
    return prediction.tables
      .filter((table) => table.staged !== undefined)
      .map((table) => ({
        componentPath,
        table,
        discarded: discarded.get(table.name),
      }));
  });
  if (stagedTables.length === 0) {
    return;
  }
  const lines = stagedTables.map(({ componentPath, table, discarded }) => {
    const componentPrefix =
      componentPath !== "" ? `${chalkStderr.gray(componentPath)}: ` : "";
    // The state describes the validator this push discards, so advertising
    // it as ready to promote would contradict the unstaging warning.
    if (discarded !== undefined) {
      return `  ${componentPrefix}${table.name}: ${formatDiscardedStagedValidator(discarded)}`;
    }
    const staged = table.staged!;
    switch (staged.state) {
      case "pending": {
        return `  ${componentPrefix}${table.name}: validating in the background (${formatValidationProgress(staged)})`;
      }
      case "valid":
        // `supersetOfStagedValidated` means this push already promotes it.
        return table.outcome === "supersetOfStagedValidated"
          ? `  ${componentPrefix}${table.name}: ${chalkStderr.green("validated")}. This deploy applies the staged schema without scanning the table.`
          : `  ${componentPrefix}${table.name}: ${chalkStderr.green("validated")}. Safe to apply the staged schema changes and deploy.`;
      case "failed":
        return `  ${componentPrefix}${table.name}: ${chalkStderr.red("failed")}: ${staged.error}${chalkStderr.gray(" (fix the offending documents and redeploy to revalidate)")}`;
    }
  });
  logMessage(`Staged schema validation status:\n${lines.join("\n")}\n`);
}

function formatWalkedTable({
  componentPath,
  table,
}: {
  componentPath: string;
  table: TablePrediction;
}) {
  const componentPrefix =
    componentPath !== "" ? `${chalkStderr.gray(componentPath)}: ` : "";
  const docsFormatted = `${table.numDocs.toLocaleString()} documents, `;
  const sizeFormatted = formatSize(table.sizeBytes);
  let stagedNote = "";
  if (table.staged?.state === "pending" && table.canSkipAfterStagedValidation) {
    stagedNote = `. Staged schema validation in progress. Wait for validation to complete before deploying with the staged changes. Deploying now will block on a full table revalidation scan.`;
  }
  return `  ${componentPrefix}${table.name} (${docsFormatted}${sizeFormatted})${stagedNote}`;
}

function minBytesForSchemaWalkWarning(): number {
  return integerFromEnv(
    "CONVEX_MIN_BYTES_FOR_SCHEMA_WALK_WARNING",
    MIN_BYTES_FOR_SCHEMA_WALK_WARNING,
  );
}
