/**
 * Utilities for implementing server-side Convex query and mutation functions.
 *
 * ## Usage
 *
 * ### Code Generation
 *
 * This module is typically used alongside generated server code.
 *
 * To generate the server code, run `npx convex dev` in your Convex project.
 * This will create a `convex/_generated/server.js` file with the following
 * functions, typed for your schema:
 * - [query](https://docs.convex.dev/generated-api/server#query)
 * - [mutation](https://docs.convex.dev/generated-api/server#mutation)
 *
 * If you aren't using TypeScript and code generation, you can use these untyped
 * functions instead:
 * - {@link queryGeneric}
 * - {@link mutationGeneric}
 *
 * ### Example
 *
 * Convex functions are defined by using either the `query` or
 * `mutation` wrappers.
 *
 * Queries receive a `db` that implements the {@link GenericDatabaseReader} interface.
 *
 * ```js
 * import { query } from "./_generated/server";
 *
 * export default query({
 *   handler: async ({ db }, { arg1, arg2 }) => {
 *     // Your (read-only) code here!
 *   },
 * });
 * ```
 *
 * If your function needs to write to the database, such as inserting, updating,
 * or deleting documents, use `mutation` instead which provides a `db` that
 * implements the {@link GenericDatabaseWriter} interface.
 *
 * ```js
 * import { mutation } from "./_generated/server";
 *
 * export default mutation({
 *   handler: async ({ db }, { arg1, arg2 }) => {
 *     // Your mutation code here!
 *   },
 * });
 * ```
 * @module
 */

export type {
  Auth,
  AuthConfig,
  AuthProvider,
  MutationAuth,
  UserIdentity,
  ReservedJwtClaim,
  ReservedJwtClaimPrefix,
  SignJwtClaims,
  SignJwtOptions,
  UserIdentityAttributes,
} from "./authentication.js";
export * from "./database.js";
export type {
  GenericDocument,
  GenericFieldPaths,
  GenericIndexFields,
  GenericTableIndexes,
  GenericSearchIndexConfig,
  GenericTableSearchIndexes,
  GenericVectorIndexConfig,
  GenericTableVectorIndexes,
  FieldTypeFromFieldPath,
  FieldTypeFromFieldPathInner,
  GenericTableInfo,
  DocumentByInfo,
  FieldPaths,
  Indexes,
  IndexNames,
  NamedIndex,
  SearchIndexes,
  SearchIndexNames,
  NamedSearchIndex,
  VectorIndexes,
  VectorIndexNames,
  NamedVectorIndex,
  GenericDataModel,
  AnyDataModel,
  TableNamesInDataModel,
  NamedTableInfo,
  DocumentByName,
} from "./data_model.js";

export type {
  Expression,
  ExpressionOrValue,
  FilterBuilder,
} from "./filter_builder.js";
export {
  actionGeneric,
  httpActionGeneric,
  mutationGeneric,
  queryGeneric,
  internalActionGeneric,
  internalMutationGeneric,
  internalQueryGeneric,
} from "./impl/registration_impl.js";
export {
  getServiceToken,
  getServiceUrl,
  type ServiceName,
} from "./impl/actions_impl.js";
export type {
  TransactionMetric,
  TransactionMetrics,
  TransactionLimits,
  FunctionMetadata,
  DeploymentMetadata,
  RequestMetadata,
  QueryMeta,
  MutationMeta,
  ActionMeta,
} from "./meta.js";
export type { IndexRange, IndexRangeBuilder } from "./index_range_builder.js";
export * from "./pagination.js";
export type { OrderedQuery, Query, QueryInitializer } from "./query.js";
export type {
  AdvancedRunQueryOptions,
  ArgsArray,
  DefaultFunctionArgs,
  FunctionVisibility,
  ActionBuilder,
  MutationBuilder,
  MutationBuilderWithTable,
  QueryBuilder,
  QueryBuilderWithTable,
  HttpActionBuilder,
  GenericActionCtx,
  GenericMutationCtx,
  GenericMutationCtxWithTable,
  GenericQueryCtx,
  GenericQueryCtxWithTable,
  RegisteredAction,
  RegisteredMutation,
  RegisteredQuery,
  PublicHttpAction,
  UnvalidatedFunction,
  ValidatedFunction,
  ReturnValueForOptionalValidator,
  ArgsArrayForOptionalValidator,
  ArgsArrayToObject,
  DefaultArgsForOptionalValidator,
} from "./registration.js";
export * from "./search_filter_builder.js";
export * from "./storage.js";
export type { Scheduler, SchedulableFunctionReference } from "./scheduler.js";
export { cronJobs } from "./cron.js";
export type { CronJob, Crons } from "./cron.js";
export type {
  SystemFields,
  IdField,
  WithoutSystemFields,
  WithOptionalSystemFields,
  SystemIndexes,
  IndexTiebreakerField,
} from "./system_fields.js";
export { httpRouter, HttpRouter, ROUTABLE_HTTP_METHODS } from "./router.js";
export type {
  RoutableMethod,
  RouteSpec,
  RouteSpecWithPath,
  RouteSpecWithPathPrefix,
} from "./router.js";
export {
  anyApi,
  getFunctionName,
  makeFunctionReference,
  filterApi,
} from "./api.js";
export type {
  ApiFromModules,
  AnyApi,
  FunctionReference_future,
  FilterApi,
  FunctionType,
  FunctionReference,
  FunctionArgs,
  OptionalRestArgs,
  PartialApi,
  ArgsAndOptions,
  FunctionReturnType,
  ValidatorTypeToReturnType,
} from "./api.js";
export {
  defineApp,
  defineComponent,
  componentsGeneric,
  createFunctionHandle,
  type AnyChildComponents,
  type EnvDefinition,
  type EnvFromDefinition,
  type EnvFromAppDefinition,
  type AppDefinition,
} from "./components/index.js";
/**
 * @internal
 */
export { currentSystemUdfInComponent } from "./components/index.js";
export { getFunctionAddress } from "./components/index.js";
export type {
  ComponentDefinition,
  AnyComponents,
  FunctionHandle,
  FunctionHandle_future,
} from "./components/index.js";

/**
 * @internal
 */
export type { Index, SearchIndex, VectorIndex } from "./schema.js";

export { log } from "./log.js";
export type { AuditLogBody, AuditLogValue } from "./audit_logging.js";

export type {
  SearchIndexConfig,
  VectorIndexConfig,
  TableDefinition,
  SchemaDefinition,
  DefineSchemaOptions,
  GenericSchema,
  DataModelFromSchemaDefinition,
  SystemDataModel,
  SystemTableNames,
  DocValidator,
  SystemFieldValidators,
} from "./schema.js";
export { defineTable, defineSchema, docValidator } from "./schema.js";

export type {
  VectorSearch,
  VectorSearchQuery,
  VectorFilterBuilder,
  FilterExpression,
} from "./vector_search.js";

/**
 * @public
 */
export type { BetterOmit, Expand } from "../type_utils.js";
