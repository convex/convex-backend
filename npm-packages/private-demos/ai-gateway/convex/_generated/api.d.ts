/* eslint-disable */
/**
 * Generated `api` utility.
 *
 * THIS CODE IS AUTOMATICALLY GENERATED.
 *
 * To regenerate, run `npx convex dev`.
 * @module
 */

import type * as fetch from "../fetch.js";
import type * as integrations_anthropic from "../integrations/anthropic.js";
import type * as integrations_chatCompletionAction from "../integrations/chatCompletionAction.js";
import type * as integrations_fetch from "../integrations/fetch.js";
import type * as integrations_listModelsAction from "../integrations/listModelsAction.js";
import type * as integrations_modelList from "../integrations/modelList.js";
import type * as integrations_openai from "../integrations/openai.js";
import type * as node_aiSdkProvider from "../node/aiSdkProvider.js";
import type * as node_anthropic from "../node/anthropic.js";
import type * as node_fetch from "../node/fetch.js";
import type * as node_openai from "../node/openai.js";
import type * as openai from "../openai.js";

import type {
  ApiFromModules,
  FilterApi,
  FunctionReference,
} from "convex/server";

declare const fullApi: ApiFromModules<{
  fetch: typeof fetch;
  "integrations/anthropic": typeof integrations_anthropic;
  "integrations/chatCompletionAction": typeof integrations_chatCompletionAction;
  "integrations/fetch": typeof integrations_fetch;
  "integrations/listModelsAction": typeof integrations_listModelsAction;
  "integrations/modelList": typeof integrations_modelList;
  "integrations/openai": typeof integrations_openai;
  "node/aiSdkProvider": typeof node_aiSdkProvider;
  "node/anthropic": typeof node_anthropic;
  "node/fetch": typeof node_fetch;
  "node/openai": typeof node_openai;
  openai: typeof openai;
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

export declare const components: {};
