/* eslint-disable */
/**
 * Generated `api` utility.
 *
 * THIS CODE IS AUTOMATICALLY GENERATED.
 *
 * To regenerate, run `npx convex dev`.
 * @module
 */

import type * as actions_componentsNode from "../actions/componentsNode.js";
import type * as actions_externalNodeDeps from "../actions/externalNodeDeps.js";
import type * as actions_insert from "../actions/insert.js";
import type * as common from "../common.js";
import type * as components_ from "../components.js";
import type * as http from "../http.js";
import type * as insert from "../insert.js";
import type * as log from "../log.js";
import type * as loopForever from "../loopForever.js";
import type * as openclaurd from "../openclaurd.js";
import type * as query_index from "../query_index.js";
import type * as schedule from "../schedule.js";
import type * as search from "../search.js";
import type * as setup from "../setup.js";
import type * as update from "../update.js";
import type * as vectorSearch from "../vectorSearch.js";

import type {
  ApiFromModules,
  FilterApi,
  FunctionReference,
} from "convex/server";

declare const fullApi: ApiFromModules<{
  "actions/componentsNode": typeof actions_componentsNode;
  "actions/externalNodeDeps": typeof actions_externalNodeDeps;
  "actions/insert": typeof actions_insert;
  common: typeof common;
  components: typeof components_;
  http: typeof http;
  insert: typeof insert;
  log: typeof log;
  loopForever: typeof loopForever;
  openclaurd: typeof openclaurd;
  query_index: typeof query_index;
  schedule: typeof schedule;
  search: typeof search;
  setup: typeof setup;
  update: typeof update;
  vectorSearch: typeof vectorSearch;
}>;

/**
 * A utility for referencing Convex functions in your app's public API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = api.myModule.myFunction;
 * ```
 */
export declare const api: FilterApi<
  typeof fullApi,
  FunctionReference<any, "public">
>;

/**
 * A utility for referencing Convex functions in your app's internal API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = internal.myModule.myFunction;
 * ```
 */
export declare const internal: FilterApi<
  typeof fullApi,
  FunctionReference<any, "internal">
>;

export declare const components: {
  counterComponent: import("../../counterComponent/_generated/component.js").ComponentApi<"counterComponent">;
};
