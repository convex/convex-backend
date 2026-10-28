/* eslint-disable */
/**
 * Generated `api` utility.
 *
 * THIS CODE IS AUTOMATICALLY GENERATED.
 *
 * To regenerate, run `npx convex dev`.
 * @module
 */

import type * as agentSkillManifest from "../agentSkillManifest.js";
import type * as crons from "../crons.js";
import type * as cursorRules from "../cursorRules.js";
import type * as guidelines from "../guidelines.js";
import type * as http from "../http.js";
import type * as localBackend from "../localBackend.js";
import type * as npm from "../npm.js";
import type * as util_convexClientHeader from "../util/convexClientHeader.js";
import type * as util_cursorRules from "../util/cursorRules.js";
import type * as util_github from "../util/github.js";
import type * as util_guidelines from "../util/guidelines.js";
import type * as util_hash from "../util/hash.js";
import type * as util_isStale from "../util/isStale.js";
import type * as util_localBackend from "../util/localBackend.js";
import type * as util_message from "../util/message.js";
import type * as util_npm from "../util/npm.js";
import type * as util_oldCursorRules from "../util/oldCursorRules.js";

import type {
  ApiFromModules,
  FilterApi,
  FunctionReference,
} from "convex/server";

declare const fullApi: ApiFromModules<{
  agentSkillManifest: typeof agentSkillManifest;
  crons: typeof crons;
  cursorRules: typeof cursorRules;
  guidelines: typeof guidelines;
  http: typeof http;
  localBackend: typeof localBackend;
  npm: typeof npm;
  "util/convexClientHeader": typeof util_convexClientHeader;
  "util/cursorRules": typeof util_cursorRules;
  "util/github": typeof util_github;
  "util/guidelines": typeof util_guidelines;
  "util/hash": typeof util_hash;
  "util/isStale": typeof util_isStale;
  "util/localBackend": typeof util_localBackend;
  "util/message": typeof util_message;
  "util/npm": typeof util_npm;
  "util/oldCursorRules": typeof util_oldCursorRules;
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
