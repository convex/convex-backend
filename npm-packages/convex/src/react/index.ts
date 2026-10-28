/**
 * Tools to integrate Convex into React applications.
 *
 * This module contains:
 * 1. {@link ConvexReactClient}, a client for using Convex in React.
 * 2. {@link ConvexProvider}, a component that stores this client in React context.
 * 3. {@link Authenticated}, {@link Unauthenticated}, {@link AuthLoading} and {@link AuthRefreshing} helper auth components.
 * 4. Hooks {@link useQuery}, {@link useMutation}, {@link useAction} and more for accessing this
 *    client from your React components.
 *
 * ## Usage
 *
 * ### Creating the client
 *
 * ```typescript
 * import { ConvexReactClient } from "convex/react";
 *
 * // typically loaded from an environment variable
 * const address = "https://small-mouse-123.convex.cloud"
 * const convex = new ConvexReactClient(address);
 * ```
 *
 * ### Storing the client in React Context
 *
 * ```typescript
 * import { ConvexProvider } from "convex/react";
 *
 * <ConvexProvider client={convex}>
 *   <App />
 * </ConvexProvider>
 * ```
 *
 * ### Using the auth helpers
 *
 * ```typescript
 * import { Authenticated, Unauthenticated, AuthLoading, AuthRefreshing } from "convex/react";
 *
 * <Authenticated>
 *   Logged in
 * </Authenticated>
 * <Unauthenticated>
 *   Logged out
 * </Unauthenticated>
 * <AuthLoading>
 *   Still loading
 * </AuthLoading>
 * <AuthRefreshing>
 *   Refreshing token...
 * </AuthRefreshing>
 * ```
 *
 * ### Using React hooks
 *
 * ```typescript
 * import { useQuery, useMutation } from "convex/react";
 * import { api } from "../convex/_generated/api";
 *
 * function App() {
 *   const counter = useQuery(api.getCounter.default);
 *   const increment = useMutation(api.incrementCounter.default);
 *   // Your component here!
 * }
 * ```
 * @module
 */
export * from "./use_paginated_query.js";
export {
  usePaginatedQuery_experimental,
  type UsePaginatedQueryOptions,
  type UsePaginatedQueryObjectReturnType,
} from "./use_paginated_query2.js";
export { usePaginatedQuery } from "./use_paginated_query.js";
export { useQueries, type RequestForQueries } from "./use_queries.js";
export type { AuthTokenFetcher } from "../browser/sync/client.js";
export * from "./auth_helpers.js";
export * from "./ConvexAuthState.js";
export * from "./hydration.js";
/* @internal */
export { useSubscription } from "./use_subscription.js";
export {
  type ReactMutation,
  type ReactAction,
  type Watch,
  type WatchQueryOptions,
  type MutationOptions,
  type ConvexReactClientOptions,
  type OptionalRestArgsOrSkip,
  type UseQueryResult,
  ConvexReactClient,
  useConvex,
  ConvexProvider,
  useQuery,
  useQuery_experimental,
  useMutation,
  useAction,
  useConvexConnectionState,
} from "./client.js";
/** @internal */
export { convexQueryOptions } from "../browser/query_options.js";
export type { QueryOptions } from "../browser/query_options.js";
