import { chalkStderr } from "chalk";
import { Context } from "../../bundler/context.js";
import { logFinishedStep, logMessage, stopSpinner } from "../../bundler/log.js";
import { promptYesNo } from "./utils/prompts.js";
import {
  DiscardedStagedValidator,
  EvaluateSchemaResponse,
} from "./deployApi/evaluateSchema.js";
import { formatDiscardedStagedValidator } from "./formatValidationProgress.js";

export type StagedValidatorDeletionCheck =
  | "ask for confirmation"
  | "has confirmation"
  | "no verification";

/**
 * Confirm before discarding pending or successful staged validation. Unstaging
 * stops write-time checks, so restaging requires a fresh scan.
 *
 * A dry run only reports what the push would discard, so `--dry-run` previews
 * the confirmation the real push will ask for.
 */
export async function checkForStagedValidatorDeletion({
  ctx,
  response,
  askForConfirmation,
  dryRun = false,
}: {
  ctx: Context;
  response: EvaluateSchemaResponse | null;
  askForConfirmation: boolean;
  dryRun?: boolean;
}): Promise<void> {
  // `sharedSchemaEvaluation` has already logged why evaluation failed.
  if (response === null) {
    return;
  }
  const discarded = Object.entries(response.componentSchemaEvaluations).flatMap(
    ([componentPath, prediction]) =>
      (prediction.discardedStagedValidators ?? []).map((validator) => ({
        componentPath,
        validator,
      })),
  );
  if (discarded.length === 0) {
    return;
  }

  logMessage(`\u26a0\ufe0f  This push ${chalkStderr.bold(dryRun ? "would discard" : "discards")} staged schema validation for the following ${discarded.length === 1 ? "table" : "tables"}:

${discarded.map(formatDiscarded).join("\n")}

A discarded staged validator restarts validation from zero if staged again.
`);

  if (dryRun) {
    return;
  }

  if (!askForConfirmation) {
    logFinishedStep(
      "Proceeding with push since --allow-unstaging-schema is set",
    );
    return;
  }

  if (!process.stdin.isTTY) {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `To confirm the push:
\u2022 run the deploy command in an ${chalkStderr.bold("interactive terminal")}
\u2022 or run the deploy command with the ${chalkStderr.bold("--allow-unstaging-schema")} flag`,
    });
  }

  stopSpinner();
  if (
    !(await promptYesNo(ctx, {
      message: `Discard staged validation for ${discarded.length === 1 ? "this table" : "these tables"}?`,
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

function formatDiscarded({
  componentPath,
  validator,
}: {
  componentPath: string;
  validator: DiscardedStagedValidator;
}): string {
  const componentPrefix =
    componentPath !== "" ? `${chalkStderr.gray(componentPath)}:` : "";
  return `\u26d4 ${componentPrefix}${validator.tableName}: ${formatDiscardedStagedValidator(validator)}`;
}
