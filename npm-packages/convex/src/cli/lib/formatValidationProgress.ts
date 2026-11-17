import { chalkStderr } from "chalk";
import { DiscardedStagedValidator } from "./deployApi/evaluateSchema.js";

export function formatValidationProgress({
  numDocsValidated,
  totalDocs,
}: {
  numDocsValidated: number;
  totalDocs?: number | null | undefined;
}): string {
  const checked = numDocsValidated.toLocaleString();
  return totalDocs === null || totalDocs === undefined
    ? `${checked} documents checked`
    : `${checked} of approximately ${totalDocs.toLocaleString()} documents checked`;
}

/** "validated; the push removes the staged validator" */
export function formatDiscardedStagedValidator(
  validator: DiscardedStagedValidator,
): string {
  let status;
  switch (validator.state.state) {
    case "pending": {
      status = `validating in the background (${formatValidationProgress(validator.state)})`;
      break;
    }
    case "valid":
      status = chalkStderr.green("validated");
      break;
  }
  const action = validator.replaced
    ? "the push stages a different validator"
    : "the push removes the staged validator";
  return `${status}; ${action}`;
}
