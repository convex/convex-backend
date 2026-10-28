/**
 * Utilities for defining the schema of your Convex project.
 *
 * ## Usage
 *
 * Schemas should be placed in a `schema.ts` file in your `convex/` directory.
 *
 * Schema definitions should be built using {@link defineSchema},
 * {@link defineTable}, and {@link values.v}. Make sure to export the schema as the
 * default export.
 *
 * ```ts
 * import { defineSchema, defineTable } from "convex/server";
 * import { v } from "convex/values";
 *
 *  export default defineSchema({
 *    messages: defineTable({
 *      body: v.string(),
 *      user: v.id("users"),
 *    }),
 *    users: defineTable({
 *      name: v.string(),
 *    }),
 *  });
 * ```
 *
 * To learn more about schemas, see [Defining a Schema](https://docs.convex.dev/using/schemas).
 * @module
 */
import {
  AnyDataModel,
  GenericDataModel,
  GenericTableIndexes,
  GenericTableSearchIndexes,
  GenericTableVectorIndexes,
  TableNamesInDataModel,
} from "../server/data_model.js";
import {
  IdField,
  IndexTiebreakerField,
  SystemFields,
  SystemIndexes,
} from "../server/system_fields.js";
import { Expand } from "../type_utils.js";
import { GenericId } from "../values/index.js";
import {
  GenericValidator,
  ObjectType,
  isValidator,
  v,
} from "../values/validator.js";
import {
  VFloat64,
  VId,
  VObject,
  VUnion,
  Validator,
} from "../values/validators.js";

/**
 * Extract all of the index field paths within a {@link Validator}.
 *
 * This is used within {@link defineTable}.
 * @public
 */
type ExtractFieldPaths<T extends Validator<any, any, any>> =
  // Add in the system fields available in index definitions.
  // This should be everything except for `_id` because thats added to indexes
  // automatically.
  T["fieldPaths"] | keyof SystemFields;

/**
 * Extract the {@link GenericDocument} within a {@link Validator} and
 * add on the system fields.
 *
 * This is used within {@link defineTable}.
 * @public
 */
type ExtractDocument<T extends Validator<any, any, any>> =
  // Add the system fields to `Value` (except `_id` because it depends on
  //the table name) and trick TypeScript into expanding them.
  Expand<SystemFields & T["type"]>;

export interface DbIndexConfig<
  FirstFieldPath extends string,
  RestFieldPaths extends string[],
> {
  /**
   * The fields to index, in order. Must specify at least one field.
   */
  fields: [FirstFieldPath, ...RestFieldPaths];
}

/**
 * The configuration for a full text search index.
 *
 * @public
 */
export interface SearchIndexConfig<
  SearchField extends string,
  FilterFields extends string,
> {
  /**
   * The field to index for full text search.
   *
   * This must be a field of type `string`.
   */
  searchField: SearchField;

  /**
   * Additional fields to index for fast filtering when running search queries.
   */
  filterFields?: FilterFields[];
}

/**
 * The configuration for a vector index.
 *
 * @public
 */
export interface VectorIndexConfig<
  VectorField extends string,
  FilterFields extends string,
> {
  /**
   * The field to index for vector search.
   *
   * This must be a field of type `v.array(v.float64())` (or a union)
   */
  vectorField: VectorField;
  /**
   * The length of the vectors indexed. This must be between 2 and 2048 inclusive.
   */
  dimensions: number;
  /**
   * Additional fields to index for fast filtering when running vector searches.
   */
  filterFields?: FilterFields[];
}

/**
 * @internal
 */
export type VectorIndex = {
  indexDescriptor: string;
  vectorField: string;
  dimensions: number;
  filterFields: string[];
};

/**
 * @internal
 */
export type Index = {
  indexDescriptor: string;
  fields: string[];
};

/**
 * @internal
 */
export type SearchIndex = {
  indexDescriptor: string;
  searchField: string;
  filterFields: string[];
};

/**
 * Options for defining an index.
 *
 * @public
 */
export interface IndexOptions {
  /**
   * Whether the index should be staged.
   *
   * For large tables, index backfill can be slow. Staging an index allows you
   * to push the schema and enable the index later.
   *
   * If `staged` is `true`, the index will be staged and will not be enabled
   * until the staged flag is removed. Staged indexes do not block push
   * completion. Staged indexes cannot be used in queries.
   */
  staged?: boolean;
}

/**
 * The definition of a table within a schema.
 *
 * This should be produced by using {@link defineTable}.
 * @public
 */
export class TableDefinition<
  DocumentType extends Validator<any, any, any> = Validator<any, any, any>,
  Indexes extends GenericTableIndexes = {},
  SearchIndexes extends GenericTableSearchIndexes = {},
  VectorIndexes extends GenericTableVectorIndexes = {},
> {
  private indexes: Index[];
  private stagedDbIndexes: Index[];
  private searchIndexes: SearchIndex[];
  private stagedSearchIndexes: SearchIndex[];
  private vectorIndexes: VectorIndex[];
  private stagedVectorIndexes: VectorIndex[];
  // The type of documents stored in this table.
  validator: DocumentType;
  // The proposed next validator for this table, validated against existing
  // documents in the background while `validator` remains enforced.
  private stagedValidator: Validator<any, any, any> | undefined;

  /**
   * @internal
   */
  constructor(documentType: DocumentType) {
    this.indexes = [];
    this.stagedDbIndexes = [];
    this.searchIndexes = [];
    this.stagedSearchIndexes = [];
    this.vectorIndexes = [];
    this.stagedVectorIndexes = [];
    this.validator = documentType;
  }

  /**
   * This API is experimental: it may change or disappear.
   *
   * Returns indexes defined on this table.
   * Intended for the advanced use cases of dynamically deciding which index to use for a query.
   */
  " indexes"(): { indexDescriptor: string; fields: string[] }[] {
    return this.indexes;
  }

  /**
   * Define an index on this table.
   *
   * Indexes speed up queries by allowing efficient lookups on specific fields.
   * Use `.withIndex()` in your queries to leverage them.
   *
   * Index fields must be queried in the same order they are defined. If you
   * need to query by `field2` then `field1`, create a separate index with
   * that field order.
   *
   * @example
   * ```ts
   * defineTable({
   *   userId: v.id("users"),
   *   status: v.string(),
   *   updatedAt: v.number(),
   * })
   *   // Name indexes after their fields:
   *   .index("by_userId", ["userId"])
   *   .index("by_status_updatedAt", ["status", "updatedAt"])
   * ```
   *
   * **Best practice:** Always include all index fields in the index name
   * (e.g., `"by_field1_and_field2"`).
   *
   * @param name - The name of the index.
   * @param indexConfig - The index configuration object.
   * @returns A {@link TableDefinition} with this index included.
   *
   * @see https://docs.convex.dev/database/reading-data/indexes
   */
  index<
    IndexName extends string,
    FirstFieldPath extends ExtractFieldPaths<DocumentType>,
    RestFieldPaths extends ExtractFieldPaths<DocumentType>[],
  >(
    name: IndexName,
    indexConfig: Expand<
      DbIndexConfig<FirstFieldPath, RestFieldPaths> &
        IndexOptions & { staged?: false }
    >,
  ): TableDefinition<
    DocumentType,
    Expand<
      Indexes &
        Record<
          IndexName,
          [FirstFieldPath, ...RestFieldPaths, IndexTiebreakerField]
        >
    >,
    SearchIndexes,
    VectorIndexes
  >;

  /**
   * Define an index on this table.
   *
   * To learn about indexes, see [Defining Indexes](https://docs.convex.dev/database/reading-data/indexes).
   *
   * @param name - The name of the index.
   * @param fields - The fields to index, in order. Must specify at least one
   * field.
   * @returns A {@link TableDefinition} with this index included.
   */
  index<
    IndexName extends string,
    FirstFieldPath extends ExtractFieldPaths<DocumentType>,
    RestFieldPaths extends ExtractFieldPaths<DocumentType>[],
  >(
    name: IndexName,
    fields: [FirstFieldPath, ...RestFieldPaths],
  ): TableDefinition<
    DocumentType,
    Expand<
      Indexes &
        Record<
          IndexName,
          [FirstFieldPath, ...RestFieldPaths, IndexTiebreakerField]
        >
    >,
    SearchIndexes,
    VectorIndexes
  >;

  /**
   * Define a staged index on this table.
   *
   * For large tables, index backfill can be slow. Staging an index allows you
   * to push the schema and enable the index later.
   *
   * If `staged` is `true`, the index will be staged and will not be enabled
   * until the staged flag is removed. Staged indexes do not block push
   * completion. Staged indexes cannot be used in queries.
   *
   * To learn about indexes, see [Defining Indexes](https://docs.convex.dev/using/indexes).
   *
   * @param name - The name of the index.
   * @param indexConfig - The index configuration object.
   * @returns A {@link TableDefinition} with this index included.
   */
  index<
    IndexName extends string,
    FirstFieldPath extends ExtractFieldPaths<DocumentType>,
    RestFieldPaths extends ExtractFieldPaths<DocumentType>[],
  >(
    name: IndexName,
    indexConfig: Expand<
      DbIndexConfig<FirstFieldPath, RestFieldPaths> &
        IndexOptions & { staged: true }
    >,
  ): TableDefinition<DocumentType, Indexes, SearchIndexes, VectorIndexes>;

  index<
    IndexName extends string,
    FirstFieldPath extends ExtractFieldPaths<DocumentType>,
    RestFieldPaths extends ExtractFieldPaths<DocumentType>[],
  >(
    name: IndexName,
    indexConfig:
      | Expand<DbIndexConfig<FirstFieldPath, RestFieldPaths> & IndexOptions>
      | [FirstFieldPath, ...RestFieldPaths],
  ) {
    if (Array.isArray(indexConfig)) {
      // indexConfig is [FirstFieldPath, ...RestFieldPaths]
      this.indexes.push({
        indexDescriptor: name,
        fields: indexConfig,
      });
    } else if (indexConfig.staged) {
      // indexConfig is object with fields and staged: true
      this.stagedDbIndexes.push({
        indexDescriptor: name,
        fields: indexConfig.fields,
      });
    } else {
      // indexConfig is object with fields (and maybe staged: false/undefined)
      this.indexes.push({
        indexDescriptor: name,
        fields: indexConfig.fields,
      });
    }
    return this;
  }

  /**
   * Define a search index on this table.
   *
   * To learn about search indexes, see [Search](https://docs.convex.dev/text-search).
   *
   * @param name - The name of the index.
   * @param indexConfig - The search index configuration object.
   * @returns A {@link TableDefinition} with this search index included.
   */
  searchIndex<
    IndexName extends string,
    SearchField extends ExtractFieldPaths<DocumentType>,
    FilterFields extends ExtractFieldPaths<DocumentType> = never,
  >(
    name: IndexName,
    indexConfig: Expand<
      SearchIndexConfig<SearchField, FilterFields> &
        IndexOptions & { staged?: false }
    >,
  ): TableDefinition<
    DocumentType,
    Indexes,
    // Update `SearchIndexes` to include the new index and use `Expand` to make
    // the types look pretty in editors.
    Expand<
      SearchIndexes &
        Record<
          IndexName,
          {
            searchField: SearchField;
            filterFields: FilterFields;
          }
        >
    >,
    VectorIndexes
  >;

  /**
   * Define a staged search index on this table.
   *
   * For large tables, index backfill can be slow. Staging an index allows you
   * to push the schema and enable the index later.
   *
   * If `staged` is `true`, the index will be staged and will not be enabled
   * until the staged flag is removed. Staged indexes do not block push
   * completion. Staged indexes cannot be used in queries.
   *
   * To learn about search indexes, see [Search](https://docs.convex.dev/text-search).
   *
   * @param name - The name of the index.
   * @param indexConfig - The search index configuration object.
   * @returns A {@link TableDefinition} with this search index included.
   */
  searchIndex<
    IndexName extends string,
    SearchField extends ExtractFieldPaths<DocumentType>,
    FilterFields extends ExtractFieldPaths<DocumentType> = never,
  >(
    name: IndexName,
    indexConfig: Expand<
      SearchIndexConfig<SearchField, FilterFields> &
        IndexOptions & { staged: true }
    >,
  ): TableDefinition<DocumentType, Indexes, SearchIndexes, VectorIndexes>;

  searchIndex<
    IndexName extends string,
    SearchField extends ExtractFieldPaths<DocumentType>,
    FilterFields extends ExtractFieldPaths<DocumentType> = never,
  >(
    name: IndexName,
    indexConfig: Expand<
      SearchIndexConfig<SearchField, FilterFields> & IndexOptions
    >,
  ) {
    if (indexConfig.staged) {
      this.stagedSearchIndexes.push({
        indexDescriptor: name,
        searchField: indexConfig.searchField,
        filterFields: indexConfig.filterFields || [],
      });
    } else {
      this.searchIndexes.push({
        indexDescriptor: name,
        searchField: indexConfig.searchField,
        filterFields: indexConfig.filterFields || [],
      });
    }
    return this;
  }

  /**
   * Define a vector index on this table.
   *
   * To learn about vector indexes, see [Vector Search](https://docs.convex.dev/vector-search).
   *
   * @param name - The name of the index.
   * @param indexConfig - The vector index configuration object.
   * @returns A {@link TableDefinition} with this vector index included.
   */
  vectorIndex<
    IndexName extends string,
    VectorField extends ExtractFieldPaths<DocumentType>,
    FilterFields extends ExtractFieldPaths<DocumentType> = never,
  >(
    name: IndexName,
    indexConfig: Expand<
      VectorIndexConfig<VectorField, FilterFields> &
        IndexOptions & { staged?: false }
    >,
  ): TableDefinition<
    DocumentType,
    Indexes,
    SearchIndexes,
    Expand<
      VectorIndexes &
        Record<
          IndexName,
          {
            vectorField: VectorField;
            dimensions: number;
            filterFields: FilterFields;
          }
        >
    >
  >;

  /**
   * Define a staged vector index on this table.
   *
   * For large tables, index backfill can be slow. Staging an index allows you
   * to push the schema and enable the index later.
   *
   * If `staged` is `true`, the index will be staged and will not be enabled
   * until the staged flag is removed. Staged indexes do not block push
   * completion. Staged indexes cannot be used in queries.
   *
   * To learn about vector indexes, see [Vector Search](https://docs.convex.dev/vector-search).
   *
   * @param name - The name of the index.
   * @param indexConfig - The vector index configuration object.
   * @returns A {@link TableDefinition} with this vector index included.
   */
  vectorIndex<
    IndexName extends string,
    VectorField extends ExtractFieldPaths<DocumentType>,
    FilterFields extends ExtractFieldPaths<DocumentType> = never,
  >(
    name: IndexName,
    indexConfig: Expand<
      VectorIndexConfig<VectorField, FilterFields> &
        IndexOptions & { staged: true }
    >,
  ): TableDefinition<DocumentType, Indexes, SearchIndexes, VectorIndexes>;

  vectorIndex<
    IndexName extends string,
    VectorField extends ExtractFieldPaths<DocumentType>,
    FilterFields extends ExtractFieldPaths<DocumentType> = never,
  >(
    name: IndexName,
    indexConfig: Expand<
      VectorIndexConfig<VectorField, FilterFields> & IndexOptions
    >,
  ) {
    if (indexConfig.staged) {
      this.stagedVectorIndexes.push({
        indexDescriptor: name,
        vectorField: indexConfig.vectorField,
        dimensions: indexConfig.dimensions,
        filterFields: indexConfig.filterFields || [],
      });
    } else {
      this.vectorIndexes.push({
        indexDescriptor: name,
        vectorField: indexConfig.vectorField,
        dimensions: indexConfig.dimensions,
        filterFields: indexConfig.filterFields || [],
      });
    }
    return this;
  }

  /**
   * Stage changes to this table's document validator.
   *
   * Convex validates the staged validator against the table's existing
   * documents in the background, while the validator passed to
   * {@link defineTable} remains the one enforced on writes. Once
   * the background validation succeeds, apply the staged changes
   * and remove `.staged()`.
   *
   * For an object validator, pass only the fields to add or replace. Fields
   * omitted from the changes keep their enforced validators.
   * Set a field to `undefined` to remove it from the staged validator.
   * For nested objects, you must specify the full object validator.
   *
   * ```ts
   * defineTable({
   *   apple: v.string(),
   *   banana: v.number(),
   *   legacy: v.optional(v.string()),
   * }).staged({
   *   banana: v.union(v.string(), v.number()),
   *   legacy: undefined,
   * })
   * ```
   *
   * See [Staged validators](https://docs.convex.dev/database/schemas#staged-validators).
   *
   * @param documentSchema - Field changes for a table defined as an object or
   * `v.any()`, or a full schema validator specifying the proposed next document
   * type. Use a full schema validator for a table defined as a union.
   * @returns A {@link TableDefinition} with the staged validator attached.
   */
  staged(
    documentSchema:
      | Validator<Record<string, any>, "required", any>
      | Record<string, GenericValidator | undefined>,
  ): TableDefinition<DocumentType, Indexes, SearchIndexes, VectorIndexes> {
    if (this.stagedValidator !== undefined) {
      throw new Error("Table cannot have more than one staged validator.");
    }
    if (isValidator(documentSchema)) {
      this.stagedValidator = documentSchema;
    } else {
      if (this.validator.kind !== "object" && this.validator.kind !== "any") {
        throw new Error(
          "Specifying top-level field changes is only supported for simple table definitions. For a table defined with a union or record validator, you must stage a fully defined object or union validator.",
        );
      }
      const enforcedFields =
        this.validator.kind === "object" ? this.validator.fields : {};
      const fields: Record<string, GenericValidator> = Object.fromEntries(
        Object.entries({ ...enforcedFields, ...documentSchema }).filter(
          (entry): entry is [string, GenericValidator] =>
            entry[1] !== undefined,
        ),
      );
      this.stagedValidator = v.object(fields);
    }
    return this.self();
  }

  /**
   * Work around for https://github.com/microsoft/TypeScript/issues/57035
   */
  protected self(): TableDefinition<
    DocumentType,
    Indexes,
    SearchIndexes,
    VectorIndexes
  > {
    return this;
  }
  /**
   * Export the contents of this definition.
   *
   * This is called internally by the Convex framework.
   * @internal
   */
  export() {
    const documentType = this.validator.json;
    if (typeof documentType !== "object") {
      throw new Error(
        "Invalid validator: please make sure that the parameter of `defineTable` is valid (see https://docs.convex.dev/database/schemas)",
      );
    }

    let stagedDocumentType = undefined;
    if (this.stagedValidator !== undefined) {
      stagedDocumentType = this.stagedValidator.json;
      if (typeof stagedDocumentType !== "object") {
        throw new Error(
          "Invalid staged validator: please make sure that the parameter of `.staged()` is valid (see https://docs.convex.dev/database/schemas)",
        );
      }
    }

    return {
      indexes: this.indexes,
      stagedDbIndexes: this.stagedDbIndexes,
      searchIndexes: this.searchIndexes,
      stagedSearchIndexes: this.stagedSearchIndexes,
      vectorIndexes: this.vectorIndexes,
      stagedVectorIndexes: this.stagedVectorIndexes,
      documentType,
      stagedDocumentType,
    };
  }
}

/**
 * Define a table in a schema.
 *
 * You can either specify the schema of your documents as an object like
 * ```ts
 * defineTable({
 *   field: v.string()
 * });
 * ```
 *
 * or as a schema type like
 * ```ts
 * defineTable(
 *  v.union(
 *    v.object({...}),
 *    v.object({...})
 *  )
 * );
 * ```
 *
 * @param documentSchema - The type of documents stored in this table.
 * @returns A {@link TableDefinition} for the table.
 *
 * @public
 */
export function defineTable<
  DocumentSchema extends Validator<Record<string, any>, "required", any>,
>(documentSchema: DocumentSchema): TableDefinition<DocumentSchema>;
/**
 * Define a table in a schema.
 *
 * You can either specify the schema of your documents as an object like
 * ```ts
 * defineTable({
 *   field: v.string()
 * });
 * ```
 *
 * or as a schema type like
 * ```ts
 * defineTable(
 *  v.union(
 *    v.object({...}),
 *    v.object({...})
 *  )
 * );
 * ```
 *
 * @param documentSchema - The type of documents stored in this table.
 * @returns A {@link TableDefinition} for the table.
 *
 * @public
 */
export function defineTable<
  DocumentSchema extends Record<string, GenericValidator>,
>(
  documentSchema: DocumentSchema,
): TableDefinition<VObject<ObjectType<DocumentSchema>, DocumentSchema>>;
export function defineTable<
  DocumentSchema extends
    | Validator<Record<string, any>, "required", any>
    | Record<string, GenericValidator>,
>(documentSchema: DocumentSchema): TableDefinition<any, any, any> {
  if (isValidator(documentSchema)) {
    return new TableDefinition(documentSchema);
  } else {
    return new TableDefinition(v.object(documentSchema));
  }
}

/**
 * The validators for the system fields Convex adds to every document.
 *
 * @public
 */
export type SystemFieldValidators<TableName extends string> = {
  _id: VId<GenericId<TableName>>;
  _creationTime: VFloat64<number>;
};

/**
 * Add the system field validators to an object validator, leaving any other
 * validator untouched.
 */
type WithSystemFieldValidators<
  TableName extends string,
  DocumentType extends Validator<any, any, any>,
> =
  DocumentType extends VObject<infer Type, infer Fields, any, any>
    ? VObject<
        Expand<IdField<TableName> & SystemFields & Type>,
        Expand<SystemFieldValidators<TableName> & Fields>
      >
    : DocumentType;

/**
 * The validator for whole documents of a table: the table's own validator with
 * the `_id` and `_creationTime` system fields added.
 *
 * For a table defined with a union, the system fields are added to each member
 * of the union.
 *
 * @public
 */
export type DocValidator<
  TableName extends string,
  DocumentType extends Validator<any, any, any>,
> =
  DocumentType extends VUnion<any, infer Members, any, any>
    ? {
        [Index in keyof Members]: WithSystemFieldValidators<
          TableName,
          Members[Index]
        >;
      } extends infer NewMembers extends Validator<any, "required", any>[]
      ? VUnion<
          WithSystemFieldValidators<TableName, Members[number]>["type"],
          NewMembers
        >
      : never
    : WithSystemFieldValidators<TableName, DocumentType>;

function addSystemFields(
  tableName: string,
  validator: GenericValidator,
): GenericValidator {
  switch (validator.kind) {
    case "object":
      return validator.extend({
        _id: v.id(tableName),
        _creationTime: v.number(),
      });
    case "union":
      return v.union(
        ...validator.members.map((member) =>
          addSystemFields(tableName, member),
        ),
      );
    // `v.any()` already permits the system fields.
    case "any":
      return validator;
    default:
      throw new Error(
        `Invalid validator for table "${tableName}": a table's documents must be objects, or a union of objects (see https://docs.convex.dev/database/schemas)`,
      );
  }
}

/**
 * Build the validator for whole documents of a table, by adding the `_id` and
 * `_creationTime` system fields to the table's own validator.
 *
 * Prefer {@link SchemaDefinition.doc} when you have the schema in hand: it
 * checks the table name against the schema.
 *
 * @example
 * ```ts
 * const messageDoc = docValidator("messages", schema.tables.messages);
 *
 * export const get = query({
 *   args: { id: v.id("messages") },
 *   returns: v.union(messageDoc, v.null()),
 *   handler: (ctx, args) => ctx.db.get(args.id),
 * });
 * ```
 *
 * @param tableName - The name of the table, used for the `_id` validator.
 * @param table - The {@link TableDefinition} for that table.
 * @returns A validator matching documents of the table.
 *
 * @public
 */
export function docValidator<
  TableName extends string,
  Table extends TableDefinition<any, any, any, any>,
>(
  tableName: TableName,
  table: Table,
): DocValidator<TableName, Table["validator"]> {
  return addSystemFields(tableName, table.validator) as DocValidator<
    TableName,
    Table["validator"]
  >;
}

/**
 * A type describing the schema of a Convex project.
 *
 * This should be constructed using {@link defineSchema}, {@link defineTable},
 * and {@link v}.
 * @public
 */
export type GenericSchema = Record<string, TableDefinition>;

function tableInSchema<
  Schema extends GenericSchema,
  TableName extends keyof Schema & string,
>(
  schema: SchemaDefinition<Schema, boolean>,
  tableName: TableName,
): Schema[TableName] {
  const table = schema.tables[tableName];
  if (table === undefined) {
    throw new Error(
      `Table "${tableName}" is not in this schema. Tables in this schema: ${Object.keys(
        schema.tables,
      ).join(", ")}`,
    );
  }
  return table;
}

/**
 *
 * The definition of a Convex project schema.
 *
 * This should be produced by using {@link defineSchema}.
 * @public
 */
export class SchemaDefinition<
  Schema extends GenericSchema,
  StrictTableTypes extends boolean,
> {
  public tables: Schema;
  public strictTableNameTypes!: StrictTableTypes;
  public readonly schemaValidation: boolean;

  /**
   * @internal
   */
  constructor(tables: Schema, options?: DefineSchemaOptions<StrictTableTypes>) {
    this.tables = tables;
    this.schemaValidation =
      options?.schemaValidation === undefined ? true : options.schemaValidation;
  }

  /**
   * The validator for whole documents of a table in this schema: the table's
   * own validator with the `_id` and `_creationTime` system fields added.
   *
   * @example
   * ```ts
   * export const get = query({
   *   args: { id: schema.id("messages") },
   *   returns: v.union(schema.doc("messages"), v.null()),
   *   handler: (ctx, args) => ctx.db.get(args.id),
   * });
   * ```
   *
   * @param tableName - The name of a table in this schema.
   * @returns A validator matching documents of that table.
   */
  doc<TableName extends keyof Schema & string>(
    tableName: TableName,
  ): DocValidator<TableName, Schema[TableName]["validator"]> {
    return docValidator(tableName, tableInSchema(this, tableName));
  }

  /**
   * The validator for IDs of a table in this schema.
   *
   * Same as `v.id(tableName)`, but only accepts tables in this schema.
   *
   * @param tableName - The name of a table in this schema.
   * @returns A validator matching IDs of that table.
   */
  id<TableName extends keyof Schema & string>(
    tableName: TableName,
  ): VId<GenericId<TableName>> {
    tableInSchema(this, tableName);
    return v.id(tableName);
  }

  /**
   * Export the contents of this definition.
   *
   * This is called internally by the Convex framework.
   * @internal
   */
  export(): string {
    return JSON.stringify({
      tables: Object.entries(this.tables).map(([tableName, definition]) => {
        const {
          indexes,
          stagedDbIndexes,
          searchIndexes,
          stagedSearchIndexes,
          vectorIndexes,
          stagedVectorIndexes,
          documentType,
          stagedDocumentType,
        } = definition.export();
        return {
          tableName,
          indexes,
          stagedDbIndexes,
          searchIndexes,
          stagedSearchIndexes,
          vectorIndexes,
          stagedVectorIndexes,
          documentType,
          stagedDocumentType,
        };
      }),
      schemaValidation: this.schemaValidation,
    });
  }
}

/**
 * Options for {@link defineSchema}.
 *
 * @public
 */
export interface DefineSchemaOptions<StrictTableNameTypes extends boolean> {
  /**
   * Whether Convex should validate at runtime that all documents match
   * your schema.
   *
   * If `schemaValidation` is `true`, Convex will:
   * 1. Check that all existing documents match your schema when your schema
   * is pushed.
   * 2. Check that all insertions and updates match your schema during mutations.
   *
   * If `schemaValidation` is `false`, Convex will not validate that new or
   * existing documents match your schema. You'll still get schema-specific
   * TypeScript types, but there will be no validation at runtime that your
   * documents match those types.
   *
   * By default, `schemaValidation` is `true`.
   */
  schemaValidation?: boolean;

  /**
   * Whether the TypeScript types should allow accessing tables not in the schema.
   *
   * If `strictTableNameTypes` is `true`, using tables not listed in the schema
   * will generate a TypeScript compilation error.
   *
   * If `strictTableNameTypes` is `false`, you'll be able to access tables not
   * listed in the schema and their document type will be `any`.
   *
   * `strictTableNameTypes: false` is useful for rapid prototyping.
   *
   * Regardless of the value of `strictTableNameTypes`, your schema will only
   * validate documents in the tables listed in the schema. You can still create
   * and modify other tables on the dashboard or in JavaScript mutations.
   *
   * By default, `strictTableNameTypes` is `true`.
   */
  strictTableNameTypes?: StrictTableNameTypes;
}

/**
 * Define the schema of this Convex project.
 *
 * This should be exported as the default export from a `schema.ts` file in
 * your `convex/` directory. The schema enables runtime validation of documents
 * and provides end-to-end TypeScript type safety.
 *
 * Every document in Convex automatically has two system fields:
 * - `_id` - a unique document ID with validator `v.id("tableName")`
 * - `_creationTime` - a creation timestamp with validator `v.number()`
 *
 * You do not need to include these in your schema definition, they are added
 * automatically.
 *
 * @example
 * ```ts
 * // convex/schema.ts
 * import { defineSchema, defineTable } from "convex/server";
 * import { v } from "convex/values";
 *
 * export default defineSchema({
 *   users: defineTable({
 *     name: v.string(),
 *     email: v.string(),
 *   }).index("by_email", ["email"]),
 *
 *   messages: defineTable({
 *     body: v.string(),
 *     userId: v.id("users"),
 *     channelId: v.id("channels"),
 *   }).index("by_channel", ["channelId"]),
 *
 *   channels: defineTable({
 *     name: v.string(),
 *   }),
 *
 *   // Discriminated union table:
 *   results: defineTable(
 *     v.union(
 *       v.object({ kind: v.literal("error"), message: v.string() }),
 *       v.object({ kind: v.literal("success"), value: v.number() }),
 *     )
 *   ),
 * });
 * ```
 *
 * **Best practice:** Always include all index fields in the index name. For
 * example, an index on `["field1", "field2"]` should be named
 * `"by_field1_field2"`.
 *
 * @param schema - A map from table name to {@link TableDefinition} for all of
 * the tables in this project.
 * @param options - Optional configuration. See {@link DefineSchemaOptions} for
 * a full description.
 * @returns The schema.
 *
 * @see https://docs.convex.dev/database/schemas
 * @public
 */
export function defineSchema<
  Schema extends GenericSchema,
  StrictTableNameTypes extends boolean = true,
>(
  schema: Schema,
  options?: DefineSchemaOptions<StrictTableNameTypes>,
): SchemaDefinition<Schema, StrictTableNameTypes> {
  return new SchemaDefinition(schema, options);
}

/**
 * Internal type used in Convex code generation!
 *
 * Convert a {@link SchemaDefinition} into a {@link server.GenericDataModel}.
 *
 * @public
 */
export type DataModelFromSchemaDefinition<
  SchemaDef extends SchemaDefinition<any, boolean>,
> = MaybeMakeLooseDataModel<
  {
    [TableName in keyof SchemaDef["tables"] &
      string]: SchemaDef["tables"][TableName] extends TableDefinition<
      infer DocumentType,
      infer Indexes,
      infer SearchIndexes,
      infer VectorIndexes
    >
      ? {
          // We've already added all of the system fields except for `_id`.
          // Add that here.
          document: Expand<IdField<TableName> & ExtractDocument<DocumentType>>;
          fieldPaths:
            | keyof IdField<TableName>
            | ExtractFieldPaths<DocumentType>;
          indexes: Expand<Indexes & SystemIndexes>;
          searchIndexes: SearchIndexes;
          vectorIndexes: VectorIndexes;
        }
      : never;
  },
  SchemaDef["strictTableNameTypes"]
>;

type MaybeMakeLooseDataModel<
  DataModel extends GenericDataModel,
  StrictTableNameTypes extends boolean,
> = StrictTableNameTypes extends true
  ? DataModel
  : Expand<DataModel & AnyDataModel>;

const _systemSchema = defineSchema({
  _scheduled_functions: defineTable({
    name: v.string(),
    args: v.array(v.any()),
    scheduledTime: v.float64(),
    completedTime: v.optional(v.float64()),
    state: v.union(
      v.object({ kind: v.literal("pending") }),
      v.object({ kind: v.literal("inProgress") }),
      v.object({ kind: v.literal("success") }),
      v.object({ kind: v.literal("failed"), error: v.string() }),
      v.object({ kind: v.literal("canceled") }),
    ),
  }),
  _storage: defineTable({
    sha256: v.string(),
    size: v.float64(),
    contentType: v.optional(v.string()),
  }),
});

export interface SystemDataModel extends DataModelFromSchemaDefinition<
  typeof _systemSchema
> {}

export type SystemTableNames = TableNamesInDataModel<SystemDataModel>;
