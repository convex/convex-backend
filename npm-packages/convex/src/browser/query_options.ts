// Inspired by https://tanstack.com/query/v5/docs/framework/react/guides/query-options
import type {
  FunctionArgs,
  FunctionReference,
  FunctionReference_future,
} from "../server/api.js";

/**
 * Options for a Convex query: the query function reference and its arguments.
 *
 * Used with the object-form overload of {@link useQuery}.
 *
 * @public
 */
export type QueryOptions<
  Query extends FunctionReference<"query"> | FunctionReference_future<"query">,
> = {
  /**
   * The query function to run.
   */
  query: Query;
  /**
   * The arguments to the query function.
   */
  args: FunctionArgs<Query>;
};

/**
 * Creates a type-safe {@link QueryOptions} object for a Convex query.
 *
 * This is an identity function that exists to provide type inference — passing
 * your query and args through this helper ensures TypeScript infers the correct
 * `Query` type parameter, which enables precise return types on hooks like
 * {@link useQuery}.
 *
 * ```typescript
 * const opts = convexQueryOptions({
 *   query: api.users.getById,
 *   args: { id: userId },
 * });
 * // opts is typed as QueryOptions<typeof api.users.getById>
 * client.prewarmQuery(opts);
 * ```
 *
 * @param options - The query and its arguments.
 * @returns The same object, typed as `QueryOptions<Query>`.
 * @internal
 */
export function convexQueryOptions<
  Query extends FunctionReference<"query"> | FunctionReference_future<"query">,
>(options: QueryOptions<Query>): QueryOptions<Query> {
  return options;
}
