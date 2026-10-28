import { z } from "zod";
import { canonicalizedModulePath, componentDefinitionPath } from "./paths.js";
import { Identifier, Reference, identifier, reference } from "./types.js";
import { analyzedModule, udfConfig } from "./modules.js";
import { looseObject } from "./utils.js";
import { ConvexValidator } from "./validator.js";

export const componentEnvDependencyValidator = looseObject({
  type: z.literal("value"),
  // Validator serialized to JSON.
  value: z.string(),
  optional: z.boolean().optional(),
});

export const componentDefinitionType = z.union([
  looseObject({
    type: z.literal("app"),
    httpPrefix: z.string().optional(),
  }),
  looseObject({
    type: z.literal("childComponent"),
    name: identifier,
  }),
]);

export const componentEnvBinding = z.union([
  looseObject({
    type: z.literal("value"),
    // Value serialized to JSON.
    value: z.string(),
  }),
  looseObject({
    type: z.literal("envVar"),
    // Name of a parent-declared env var.
    name: z.string(),
  }),
]);

export const componentInstantiation = looseObject({
  name: identifier,
  path: componentDefinitionPath,
  env: z.array(z.tuple([identifier, componentEnvBinding])).nullish(),
});

export type ComponentExports =
  | { type: "leaf"; leaf: Reference }
  | { type: "branch"; branch: [Identifier, ComponentExports][] };

export const componentExports: z.ZodType<ComponentExports> = z.lazy(() =>
  z.union([
    looseObject({
      type: z.literal("leaf"),
      leaf: reference,
    }),
    looseObject({
      type: z.literal("branch"),
      branch: z.array(z.tuple([identifier, componentExports])),
    }),
  ]),
);

export const componentDefinitionMetadata = looseObject({
  path: componentDefinitionPath,
  definitionType: componentDefinitionType,
  childComponents: z.array(componentInstantiation),
  httpMounts: z.record(z.string(), reference),
  exports: looseObject({
    type: z.literal("branch"),
    branch: z.array(z.tuple([identifier, componentExports])),
  }),
  envVars: z
    .array(z.tuple([identifier, componentEnvDependencyValidator]))
    .nullish(),
});

export const indexSchema = looseObject({
  indexDescriptor: z.string(),
  fields: z.array(z.string()),
});

export const vectorIndexSchema = looseObject({
  indexDescriptor: z.string(),
  vectorField: z.string(),
  dimensions: z.number().optional(),
  filterFields: z.array(z.string()),
});

export const searchIndexSchema = looseObject({
  indexDescriptor: z.string(),
  searchField: z.string(),
  filterFields: z.array(z.string()),
});

export const tableDefinition = looseObject({
  tableName: z.string(),
  indexes: z.array(indexSchema),
  searchIndexes: z.array(searchIndexSchema).optional().nullable(),
  vectorIndexes: z.array(vectorIndexSchema).optional().nullable(),
  // We don't validate validators because of performance issues and since this
  // is a server returned value.
  documentType: z.custom<ConvexValidator>(),
});
export type TableDefinition = z.infer<typeof tableDefinition>;

export const analyzedSchema = looseObject({
  tables: z.array(tableDefinition),
  schemaValidation: z.boolean(),
});
export type AnalyzedSchema = z.infer<typeof analyzedSchema>;

export const evaluatedComponentDefinition = looseObject({
  definition: componentDefinitionMetadata,
  schema: analyzedSchema.optional().nullable(),
  functions: z.record(canonicalizedModulePath, analyzedModule),
  udfConfig,
});
export type EvaluatedComponentDefinition = z.infer<
  typeof evaluatedComponentDefinition
>;
