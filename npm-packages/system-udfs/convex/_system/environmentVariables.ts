import { Doc } from "../_generated/dataModel";

/** Printed by the CLI in place of a secret's value. */
export const SECRET_ENV_VAR_PLACEHOLDER = "[Secret omitted]";

/** Replaces the value of a secret with the placeholder. */
export function withSecretValueHidden(
  doc: Doc<"_environment_variables">,
): Doc<"_environment_variables"> {
  return doc.secret ? { ...doc, value: SECRET_ENV_VAR_PLACEHOLDER } : doc;
}
