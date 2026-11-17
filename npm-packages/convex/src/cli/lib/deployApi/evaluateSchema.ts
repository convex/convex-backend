import { z } from "zod";
import { componentDefinitionPath, componentPath } from "./paths.js";
import { developerIndexConfig } from "./finishPush.js";
import { looseObject } from "./utils.js";

// Hand-mirrors `TableValidationOutcome` in
// `crates/common/src/schemas/mod.rs` (`#[serde(rename_all = "camelCase")]`),
// like the rest of this deployApi directory: there's no codegen sharing
// types between the Rust backend and this package.
export const tableValidationOutcome = z.enum([
  "notValidated",
  "supersetOfEnforced",
  "supersetOfStagedValidated",
  "supersetOfShape",
  "mustWalk",
]);
export type TableValidationOutcome = z.infer<typeof tableValidationOutcome>;

export const indexChangePrediction = z.enum([
  "added",
  "identical",
  "enabled",
  "disabled",
  "dropped",
]);
export type IndexChangePrediction = z.infer<typeof indexChangePrediction>;

// Mirrors `StagedValidatorState` in
// `crates/application/src/deploy_config.rs`.
export const stagedValidatorState = z.union([
  looseObject({
    state: z.literal("pending"),
    numDocsValidated: z.number(),
    totalDocs: z.number().nullable().optional(),
  }),
  looseObject({ state: z.literal("valid") }),
  looseObject({ state: z.literal("failed"), error: z.string() }),
]);
export type StagedValidatorState = z.infer<typeof stagedValidatorState>;

export const tablePrediction = looseObject({
  name: z.string(),
  outcome: tableValidationOutcome,
  numDocs: z.number(),
  sizeBytes: z.number(),
  staged: stagedValidatorState.optional(),
  canSkipAfterStagedValidation: z.boolean().optional(),
});
export type TablePrediction = z.infer<typeof tablePrediction>;

export const indexPrediction = z.intersection(
  developerIndexConfig,
  looseObject({
    change: indexChangePrediction,
    needsBackfill: z.boolean(),
    numDocs: z.number(),
  }),
);
export type IndexPrediction = z.infer<typeof indexPrediction>;

// Mirrors `SerializedDiscardedStagedValidator` in
// `crates/local_backend/src/deploy_config2.rs`. The backend never discards a
// failed validator, since a redeploy retries it anyway.
export const discardedStagedValidator = looseObject({
  tableName: z.string(),
  state: z.union([
    looseObject({
      state: z.literal("pending"),
      numDocsValidated: z.number(),
      totalDocs: z.number().nullable().optional(),
    }),
    looseObject({ state: z.literal("valid") }),
  ]),
  replaced: z.boolean(),
});
export type DiscardedStagedValidator = z.infer<typeof discardedStagedValidator>;

export const componentSchemaPrediction = looseObject({
  definitionPath: z.string(),
  schemaValidation: z.boolean(),
  tables: z.array(tablePrediction),
  indexes: z.array(indexPrediction),
  // Optional: responses from backends predating the field.
  discardedStagedValidators: z.array(discardedStagedValidator).optional(),
});
export type ComponentSchemaPrediction = z.infer<
  typeof componentSchemaPrediction
>;

export const evaluateSchemaResponse = looseObject({
  componentSchemaEvaluations: z.record(
    componentPath,
    componentSchemaPrediction,
  ),
  newComponentDefinitions: z.array(componentDefinitionPath),
});
export type EvaluateSchemaResponse = z.infer<typeof evaluateSchemaResponse>;
