import { chalkStderr } from "chalk";
import { Context } from "../../bundler/context.js";
import {
  changeSpinner,
  logFinishedStep,
  logMessage,
  logVerbose,
  stopSpinner,
} from "../../bundler/log.js";
import { formatIndex } from "./indexes.js";
import { promptYesNo } from "./utils/prompts.js";
import { Span } from "./tracing.js";
import { StartPushRequest } from "./deployApi/startPush.js";
import { evaluatePush } from "./deploy2.js";
import { DeveloperIndexConfig } from "./deployApi/finishPush.js";
import { EvaluateSchemaResponse } from "./deployApi/evaluateSchema.js";
import { runSystemQuery } from "./run.js";
import { SchemaEvaluation } from "./schemaEvaluation.js";
import { integerFromEnv } from "./utils/utils.js";

const MIN_DOCUMENTS_FOR_INDEX_DELETE_WARNING = 100_000;

type CheckOptions = {
  url: string;
  deploymentName: string | null;
  adminKey: string;
};

type DeletedIndex = {
  // The component instance path ("" for the root component), matching the
  // keys of both `componentSchemaEvaluations` and `indexDiffs`.
  componentPath: string;
  index: DeveloperIndexConfig;
  documentsCount: number;
  replacedBy: DeveloperIndexConfig | null;
};

export async function checkForLargeIndexDeletion({
  ctx,
  span,
  request,
  options,
  schemaEvaluation,
  askForConfirmation,
}: {
  ctx: Context;
  span: Span;
  request: StartPushRequest;
  options: CheckOptions;
  schemaEvaluation: SchemaEvaluation;
  askForConfirmation: boolean;
}): Promise<void> {
  changeSpinner("Verifying that the push isn’t deleting large indexes...");

  const schemaEvaluationResponse = await schemaEvaluation(span);
  let deletedIndexes: DeletedIndex[];
  if (schemaEvaluationResponse !== null) {
    deletedIndexes = deletedIndexesFromSchemaEvaluation(
      schemaEvaluationResponse,
    );
  } else {
    logVerbose(
      "Schema evaluation unavailable, sizing deleted indexes per table",
    );
    deletedIndexes = await deletedIndexesFromTableSizes({
      ctx,
      span,
      request,
      options,
    });
  }

  if (deletedIndexes.length === 0) {
    logFinishedStep("No indexes are deleted by this push");
    return;
  }

  const minDocumentsForWarning = minDocumentsForIndexDeleteWarning();
  if (
    !deletedIndexes.some(
      ({ documentsCount }) => documentsCount >= minDocumentsForWarning,
    )
  ) {
    logFinishedStep("No large indexes are deleted by this push");
    return;
  }

  logMessage(`⚠️  This code push will ${chalkStderr.bold("delete")} the following ${deletedIndexes.length === 1 ? "index" : "indexes"}
from your production deployment (${options.url}):

${deletedIndexes
  .map((deletedIndex) =>
    formatDeletedIndex({ ...deletedIndex, minDocumentsForWarning }),
  )
  .join("\n")}

The documents that are in the index won’t be deleted, but the index will need
to be backfilled again if you want to restore it later.
`);

  // Either --skip-large-indexes-check or its deletion-only predecessor
  // --allow-deleting-large-indexes gets here, so name neither.
  if (!askForConfirmation) {
    logFinishedStep(
      "Proceeding with push since deleting large indexes was allowed by flag",
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
      message: `Delete ${deletedIndexes.length === 1 ? "this index" : "these indexes"}?`,
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

// Preferred source: one schema evaluation reports every dropped index
// together with its table's document count, requiring only the
// `deployment:deploy` permission the push itself needs.
export function deletedIndexesFromSchemaEvaluation(
  response: EvaluateSchemaResponse,
): DeletedIndex[] {
  return Object.entries(response.componentSchemaEvaluations).flatMap(
    ([componentPath, prediction]) =>
      prediction.indexes
        .filter((index) => index.change === "dropped")
        .map((index) => ({
          componentPath,
          index,
          documentsCount: index.numDocs,
          replacedBy:
            prediction.indexes.find(
              (other) => other.change === "added" && other.name === index.name,
            ) ?? null,
        })),
  );
}

// Fallback for deployments without `evaluate_schema`: diff indexes with
// `evaluate_push`, then size each affected table with a system query. The
// system query needs the `deployment:data:view` permission, which a
// deploy-scoped key may lack.
export async function deletedIndexesFromTableSizes({
  ctx,
  span,
  request,
  options,
}: {
  ctx: Context;
  span: Span;
  request: StartPushRequest;
  options: CheckOptions;
}): Promise<DeletedIndex[]> {
  const { schemaChange } = await evaluatePush(ctx, span, request, options);

  const indexDiffs = schemaChange.indexDiffs ?? {};
  const deletedIndexes = Object.entries(indexDiffs).flatMap(
    ([componentPath, indexDiff]) =>
      indexDiff.removed_indexes.map((index) => ({
        componentPath,
        index,
        replacedBy:
          indexDiff.added_indexes.find((other) => other.name === index.name) ??
          null,
      })),
  );

  if (deletedIndexes.length === 0) {
    return [];
  }

  const tablesWithDeletedIndexes = [
    ...new Set(
      deletedIndexes.map(
        ({ componentPath, index }) => `${componentPath}:${getTableName(index)}`,
      ),
    ),
  ].map((str) => {
    const [componentPath, table] = str.split(":");
    return { componentPath, table };
  });
  changeSpinner("Checking whether the deleted indexes are on large tables...");
  const documentCounts = await Promise.all(
    tablesWithDeletedIndexes.map(async ({ componentPath, table }) => ({
      componentPath,
      table,
      count: (await runSystemQuery(ctx, {
        deploymentUrl: options.url,
        adminKey: options.adminKey,
        functionName: "_system/cli/tableSize:default",
        componentPath,
        args: { tableName: table },
      })) as number,
    })),
  );
  return deletedIndexes.map(({ componentPath, index, replacedBy }) => ({
    componentPath,
    index,
    replacedBy,
    documentsCount: documentCounts.find(
      (count) =>
        count.table === getTableName(index) &&
        count.componentPath === componentPath,
    )!.count,
  }));
}

function formatDeletedIndex({
  componentPath,
  index,
  documentsCount,
  replacedBy,
  minDocumentsForWarning,
}: DeletedIndex & { minDocumentsForWarning: number }) {
  const componentNameFormatted =
    componentPath !== "" ? `${chalkStderr.gray(componentPath)}:` : "";

  const documentsCountFormatted =
    documentsCount >= minDocumentsForWarning
      ? `  ${chalkStderr.yellowBright(`⚠️  ${documentsCount.toLocaleString()} documents`)}`
      : `  ${documentsCount.toLocaleString()} ${documentsCount === 1 ? "document" : "documents"}`;

  const replacedByFormatted = replacedBy
    ? `\n   ${chalkStderr.green("→ replaced by:")} ${formatIndex(replacedBy)}`
    : "";

  return (
    "⛔ " +
    componentNameFormatted +
    formatIndex(index) +
    documentsCountFormatted +
    replacedByFormatted
  );
}

function getTableName(index: DeveloperIndexConfig) {
  const [tableName, _indexName] = index.name.split(".");
  return tableName;
}

function minDocumentsForIndexDeleteWarning(): number {
  return integerFromEnv(
    "CONVEX_MIN_DOCUMENTS_FOR_INDEX_DELETE_WARNING",
    MIN_DOCUMENTS_FOR_INDEX_DELETE_WARNING,
  );
}
