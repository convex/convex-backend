import { GenericId } from "../values/index.js";
import { CommitTsPlaceholder } from "../values/value.js";
import {
  DocumentByName,
  GenericDataModel,
  NamedTableInfo,
  TableNamesInDataModel,
} from "./data_model.js";
import { QueryInitializer } from "./query.js";
import { SystemDataModel } from "./schema.js";
import {
  WithOptionalSystemFields,
  WithoutSystemFields,
} from "./system_fields.js";

interface BaseDatabaseReader<DataModel extends GenericDataModel> {
  /**
   * Fetch a single document from the database by table name and
   * {@link values.GenericId}.
   *
   * @example
   * ```typescript
   * const user = await ctx.db.get("users", userId);
   * ```
   *
   * @param table - The name of the table to fetch the document from.
   * @param id - The {@link values.GenericId} of the document to fetch from the database.
   * @returns - The {@link GenericDocument} of the document at the given {@link values.GenericId}, or `null` if it no longer exists.
   */
  get<TableName extends TableNamesInDataModel<DataModel>>(
    table: TableName,
    id: GenericId<NonUnion<TableName>>,
  ): Promise<DocumentByName<DataModel, TableName> | null>;

  /**
   * Fetch a single document from the database by its {@link values.GenericId}.
   *
   * Supported for backwards compatibility. Prefer `db.get(tableName, id)` in
   * new code, or `db.system.get(tableName, id)` for system tables.
   *
   * @param id - The {@link values.GenericId} of the document to fetch from the database.
   * @returns - The {@link GenericDocument} of the document at the given {@link values.GenericId}, or `null` if it no longer exists.
   */
  get<TableName extends TableNamesInDataModel<DataModel>>(
    id: GenericId<TableName>,
  ): Promise<DocumentByName<DataModel, TableName> | null>;

  /**
   * Begin a query for the given table name.
   *
   * Queries don't execute immediately, so calling this method and extending its
   * query are free until the results are actually used.
   *
   * @param tableName - The name of the table to query.
   * @returns - A {@link QueryInitializer} object to start building a query.
   */
  query<TableName extends TableNamesInDataModel<DataModel>>(
    tableName: TableName,
  ): QueryInitializer<NamedTableInfo<DataModel, TableName>>;

  /**
   * Returns the string ID format for the ID in a given table, or null if the ID
   * is from a different table or is not a valid ID.
   *
   * This accepts the string ID format as well as the `.toString()` representation
   * of the legacy class-based ID format.
   *
   * This does not guarantee that the ID exists (i.e. `db.get(tableName, id)` may return `null`).
   *
   * @param tableName - The name of the table.
   * @param id - The ID string.
   */
  normalizeId<TableName extends TableNamesInDataModel<DataModel>>(
    tableName: TableName,
    id: string,
  ): GenericId<TableName> | null;
}

interface BaseDatabaseReaderWithTable<DataModel extends GenericDataModel> {
  /**
   * Scope the database to a specific table.
   */
  table<TableName extends TableNamesInDataModel<DataModel>>(
    tableName: TableName,
  ): BaseTableReader<DataModel, TableName>;
}

export interface BaseTableReader<
  DataModel extends GenericDataModel,
  TableName extends TableNamesInDataModel<DataModel>,
> {
  /**
   * Fetch a single document from the table by its {@link values.GenericId}.
   *
   * @param id - The {@link values.GenericId} of the document to fetch from the database.
   * @returns - The {@link GenericDocument} of the document at the given {@link values.GenericId}, or `null` if it no longer exists.
   */
  get(
    id: GenericId<TableName>,
  ): Promise<DocumentByName<DataModel, TableName> | null>;

  /**
   * Begin a query for the table.
   *
   * Queries don't execute immediately, so calling this method and extending its
   * query are free until the results are actually used.
   *
   * @returns - A {@link QueryInitializer} object to start building a query.
   */
  query(): QueryInitializer<NamedTableInfo<DataModel, TableName>>;
}

/**
 * An interface to read from the database within Convex query functions.
 *
 * Available as `ctx.db` in queries (read-only) and mutations (read-write).
 * You should generally use the `DatabaseReader` type from
 * `"./_generated/server"`.
 *
 * The two entry points are:
 *   - {@link GenericDatabaseReader.get}, which fetches a single document
 *     by table name and {@link values.GenericId}.
 *   - {@link GenericDatabaseReader.query}, which starts building a query.
 *
 * @example
 * ```typescript
 * // Fetch a single document by ID:
 * const user = await ctx.db.get("users", userId);
 *
 * // Query documents with an index:
 * const messages = await ctx.db
 *   .query("messages")
 *   .withIndex("by_channel", (q) => q.eq("channelId", channelId))
 *   .order("desc")
 *   .take(50);
 * ```
 *
 * **Best practice:** Use `.withIndex()` instead of `.filter()` for efficient
 * queries. Define indexes in your schema for fields you query frequently.
 *
 * @see https://docs.convex.dev/database/reading-data
 * @public
 */
export interface GenericDatabaseReader<
  DataModel extends GenericDataModel,
> extends BaseDatabaseReader<DataModel> {
  /**
   * An interface to read from the system tables within Convex query functions.
   *
   * System tables include `_storage` (file metadata) and
   * `_scheduled_functions` (scheduled function state). Use `ctx.db.system.get()`
   * and `ctx.db.system.query()` just like regular tables.
   *
   * @example
   * ```typescript
   * // Get file metadata from the _storage system table:
   * const metadata = await ctx.db.system.get("_storage", storageId);
   * // metadata has: _id, _creationTime, contentType, sha256, size
   * ```
   *
   * @public
   */
  system: BaseDatabaseReader<SystemDataModel>;
}

export interface GenericDatabaseReaderWithTable<
  DataModel extends GenericDataModel,
> extends BaseDatabaseReaderWithTable<DataModel> {
  /**
   * An interface to read from the system tables within Convex query functions
   *
   * The two entry points are:
   *   - {@link GenericDatabaseReader.get}, which fetches a single document
   *     by its {@link values.GenericId}.
   *   - {@link GenericDatabaseReader.query}, which starts building a query.
   *
   * @public
   */
  system: BaseDatabaseReaderWithTable<SystemDataModel>;
}

/**
 * An interface to read from and write to the database within Convex mutation
 * functions.
 *
 * Available as `ctx.db` in mutations. You should generally use the
 * `DatabaseWriter` type from `"./_generated/server"`.
 *
 * Extends {@link GenericDatabaseReader}
 * with write operations. All reads and writes within a single mutation are
 * executed **atomically**, you never have to worry about partial writes
 * leaving your data in an inconsistent state.
 *
 * @example
 * ```typescript
 * // Insert a new document:
 * const userId = await ctx.db.insert("users", { name: "Alice", email: "alice@example.com" });
 *
 * // Update specific fields (shallow merge):
 * await ctx.db.patch("users", userId, { name: "Alice Smith" });
 *
 * // Replace entire document (all non-system fields):
 * await ctx.db.replace("users", userId, { name: "Bob", email: "bob@example.com" });
 *
 * // Delete a document:
 * await ctx.db.delete("users", userId);
 *
 * // Delete multiple documents (collect first, then delete each):
 * const oldTasks = await ctx.db
 *   .query("tasks")
 *   .withIndex("by_completed", (q) => q.eq("completed", true))
 *   .collect();
 * for (const task of oldTasks) {
 *   await ctx.db.delete("tasks", task._id);
 * }
 * ```
 *
 * @see https://docs.convex.dev/database/writing-data
 * @public
 */
export interface GenericDatabaseWriter<
  DataModel extends GenericDataModel,
> extends GenericDatabaseReader<DataModel> {
  /**
   * Insert a new document into a table.
   *
   * @example
   * ```typescript
   * const taskId = await ctx.db.insert("tasks", {
   *   text: "Buy groceries",
   *   completed: false,
   * });
   * ```
   *
   * @param table - The name of the table to insert a new document into.
   * @param value - The document to insert. System fields (`_id`, `_creationTime`)
   * are added automatically and should not be included.
   * @returns The {@link values.GenericId} of the new document.
   */
  insert<TableName extends TableNamesInDataModel<DataModel>>(
    table: TableName,
    value: WithoutSystemFields<DocumentByName<DataModel, TableName>>,
  ): Promise<GenericId<TableName>>;

  /**
   * Patch an existing document, shallow merging it with the given partial
   * document.
   *
   * New fields are added. Existing fields are overwritten. Fields set to
   * `undefined` are removed. Fields not specified in the patch are left
   * unchanged.
   *
   * This method will throw if the document does not exist.
   *
   * @example
   * ```typescript
   * // Update only the "completed" field, leaving other fields unchanged:
   * await ctx.db.patch("tasks", taskId, { completed: true });
   *
   * // Remove an optional field by setting it to undefined:
   * await ctx.db.patch("tasks", taskId, { assignee: undefined });
   * ```
   *
   * **Tip:** Use `patch` for partial updates. Use `replace` when you want to
   * overwrite the entire document.
   *
   * @param table - The name of the table the document is in.
   * @param id - The {@link values.GenericId} of the document to patch.
   * @param value - The partial document to merge into the existing document.
   */
  patch<TableName extends TableNamesInDataModel<DataModel>>(
    table: TableName,
    id: GenericId<NonUnion<TableName>>,
    value: PatchValue<DocumentByName<DataModel, TableName>>,
  ): Promise<void>;

  /**
   * Patch an existing document, shallow merging it with the given partial
   * document.
   *
   * New fields are added. Existing fields are overwritten. Fields set to
   * `undefined` are removed. Fields not specified in the patch are left
   * unchanged.
   *
   * This method will throw if the document does not exist.
   *
   * Supported for backwards compatibility. Prefer `db.patch(tableName, id, value)`
   * in new code.
   *
   * @param id - The {@link values.GenericId} of the document to patch.
   * @param value - The partial document to merge into the existing document.
   */
  patch<TableName extends TableNamesInDataModel<DataModel>>(
    id: GenericId<TableName>,
    value: PatchValue<DocumentByName<DataModel, TableName>>,
  ): Promise<void>;

  /**
   * Replace the value of an existing document, overwriting its old value
   * completely.
   *
   * Unlike `patch`, which does a shallow merge, `replace` overwrites the
   * entire document. Any fields not included in the new value will be removed
   * (except system fields `_id` and `_creationTime`).
   *
   * This method will throw if the document does not exist.
   *
   * @example
   * ```typescript
   * // Replace the entire document:
   * await ctx.db.replace("users", userId, {
   *   name: "New Name",
   *   email: "new@example.com",
   * });
   * ```
   *
   * @param table - The name of the table the document is in.
   * @param id - The {@link values.GenericId} of the document to replace.
   * @param value - The new document. System fields can be omitted.
   */
  replace<TableName extends TableNamesInDataModel<DataModel>>(
    table: TableName,
    id: GenericId<NonUnion<TableName>>,
    value: WithOptionalSystemFields<DocumentByName<DataModel, TableName>>,
  ): Promise<void>;

  /**
   * Replace the value of an existing document, overwriting its old value
   * completely.
   *
   * Unlike `patch`, which does a shallow merge, `replace` overwrites the
   * entire document.
   *
   * Supported for backwards compatibility. Prefer `db.replace(tableName, id, value)`
   * in new code.
   *
   * @param id - The {@link values.GenericId} of the document to replace.
   * @param value - The new document. System fields can be omitted.
   */
  replace<TableName extends TableNamesInDataModel<DataModel>>(
    id: GenericId<TableName>,
    value: WithOptionalSystemFields<DocumentByName<DataModel, TableName>>,
  ): Promise<void>;

  /**
   * Delete an existing document.
   *
   * @example
   * ```typescript
   * await ctx.db.delete("tasks", taskId);
   * ```
   *
   * @param table - The name of the table the document is in.
   * @param id - The {@link values.GenericId} of the document to remove.
   */
  delete<TableName extends TableNamesInDataModel<DataModel>>(
    table: TableName,
    id: GenericId<NonUnion<TableName>>,
  ): Promise<void>;

  /**
   * Delete an existing document.
   *
   * Supported for backwards compatibility. Prefer `db.delete(tableName, id)` in
   * new code.
   *
   * **Note:** Convex queries do not support `.delete()` directly on query
   * results. To delete multiple documents, `.collect()` them first, then
   * delete each one individually.
   *
   * @param id - The {@link values.GenericId} of the document to remove.
   */
  delete(id: GenericId<TableNamesInDataModel<DataModel>>): Promise<void>;

  /**
   * Values that are not known until the mutation commits.
   */
  vars: {
    /**
     * The placeholder for the transaction's commit timestamp. Written into a
     * document field via `db.insert`, it resolves at commit to an int64
     * (`bigint`) ordered by commit order. Within the writing mutation,
     * reading the field back yields the placeholder, which cannot be used as
     * a number.
     */
    commitTs: CommitTsPlaceholder;
  };
}

/**
 * An interface to read from and write to the database within Convex mutation
 * functions.
 *
 * You should generally use the `DatabaseWriter` type from
 * `"./_generated/server"`.
 *
 * Convex guarantees that all writes within a single mutation are
 * executed atomically, so you never have to worry about partial writes leaving
 * your data in an inconsistent state. See [the Convex Guide](https://docs.convex.dev/understanding/convex-fundamentals/functions#atomicity-and-optimistic-concurrency-control)
 * for the guarantees Convex provides your functions.
 *
 * @public
 */
export interface GenericDatabaseWriterWithTable<
  DataModel extends GenericDataModel,
> extends GenericDatabaseReaderWithTable<DataModel> {
  /**
   * Scope the database to a specific table.
   */
  table<TableName extends TableNamesInDataModel<DataModel>>(
    tableName: TableName,
  ): BaseTableWriter<DataModel, TableName>;
}

export interface BaseTableWriter<
  DataModel extends GenericDataModel,
  TableName extends TableNamesInDataModel<DataModel>,
> extends BaseTableReader<DataModel, TableName> {
  /**
   * Insert a new document into the table.
   *
   * @param value - The {@link values.Value} to insert into the given table.
   * @returns - {@link values.GenericId} of the new document.
   */
  insert(
    value: WithoutSystemFields<DocumentByName<DataModel, TableName>>,
  ): Promise<GenericId<TableName>>;

  /**
   * Patch an existing document, shallow merging it with the given partial
   * document.
   *
   * New fields are added. Existing fields are overwritten. Fields set to
   * `undefined` are removed.
   *
   * @param id - The {@link values.GenericId} of the document to patch.
   * @param value - The partial {@link GenericDocument} to merge into the specified document. If this new value
   * specifies system fields like `_id`, they must match the document's existing field values.
   */
  patch(
    id: GenericId<TableName>,
    value: PatchValue<DocumentByName<DataModel, TableName>>,
  ): Promise<void>;

  /**
   * Replace the value of an existing document, overwriting its old value.
   *
   * @param id - The {@link values.GenericId} of the document to replace.
   * @param value - The new {@link GenericDocument} for the document. This value can omit the system fields,
   * and the database will fill them in.
   */
  replace(
    id: GenericId<TableName>,
    value: WithOptionalSystemFields<DocumentByName<DataModel, TableName>>,
  ): Promise<void>;

  /**
   * Delete an existing document.
   *
   * @param id - The {@link values.GenericId} of the document to remove.
   */
  delete(id: GenericId<TableName>): Promise<void>;
}

/**
 * This prevents TypeScript from inferring that the generic `TableName` type is
 * a union type when `table` and `id` disagree.
 */
type NonUnion<T> = T extends never // `never` is the bottom type for TypeScript unions
  ? never
  : T;

/**
 * This is like Partial, but it also allows undefined to be passed to optional
 * fields when `exactOptionalPropertyTypes` is enabled in the tsconfig.
 */
type PatchValue<T> = {
  [P in keyof T]?: undefined extends T[P] ? T[P] | undefined : T[P];
};
