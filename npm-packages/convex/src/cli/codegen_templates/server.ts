import { header } from "./common.js";
import { parseValidator, validatorToType } from "./validator_helpers.js";

export type EnvVarMeta = [
  string,
  { type: "value"; value: string; optional?: boolean | undefined },
];

// Environment variables the platform always injects into every deployment,
// independent of the user's `convex.config.ts` declarations. Both are always
// present, so they're typed as required `string`.
const PLATFORM_ENV_VARS: EnvVarMeta[] = [
  ["CONVEX_CLOUD_URL", { type: "value", value: '{"type":"string"}' }],
  ["CONVEX_SITE_URL", { type: "value", value: '{"type":"string"}' }],
];

// Prepend the always-present platform env vars. Users can't declare env vars
// with these names — the backend rejects overriding built-in system env vars —
// so there's no collision to worry about.
export function withPlatformEnvVars(envVars: EnvVarMeta[]): EnvVarMeta[] {
  return [...PLATFORM_ENV_VARS, ...envVars];
}

const ENV_DOCSTRING = `/**
 * Typesafe environment variables.
 *
 * This includes platform-provided env vars and any variables declared in
 * \`convex.config.ts\`.
 */`;

export function serverCodegen({
  useTypeScript,
  envVars,
}: {
  useTypeScript: boolean;
  // - undefined: env var declarations not yet known (initial codegen before
  //   server analysis). Emits an untyped `env` stub so imports don't break.
  // - array (empty or not): emit a typed `Env` interface and typed `env`
  //   export. The interface always includes the platform-provided env vars, so
  //   an empty array still yields a typed `env` with just those.
  envVars: EnvVarMeta[] | undefined;
}) {
  const emittedEnvVars =
    envVars === undefined ? undefined : withPlatformEnvVars(envVars);
  const hasEnv = emittedEnvVars !== undefined;
  const envInterface = hasEnv ? generateEnvInterface(emittedEnvVars!) : "";
  const envDeclDTS = hasEnv
    ? `
${ENV_DOCSTRING}
export declare const env: Env;
`
    : `
export declare const env: Record<string, string | undefined>;
`;
  // Route through `globalThis` rather than the bare `process` global so the
  // generated file typechecks even in projects without `@types/node`.
  const envDeclTS = hasEnv
    ? `
${ENV_DOCSTRING}
export const env: Env = (globalThis as unknown as { process: { env: Env } }).process.env;
`
    : `
export const env: Record<string, string | undefined> = (globalThis as unknown as { process: { env: Record<string, string | undefined> } }).process.env;
`;
  const envDeclJS = hasEnv
    ? `
${ENV_DOCSTRING}
export const env = process.env;
`
    : `
export const env = process.env;
`;

  if (!useTypeScript) {
    // Generate separate .js and .d.ts files
    const serverDTS = `
    ${header(
      "Generated utilities for implementing server-side Convex query and mutation functions.",
    )}
    import {
      ActionBuilder,
      HttpActionBuilder,
      MutationBuilder,
      QueryBuilder,
      GenericActionCtx,
      GenericMutationCtx,
      GenericQueryCtx,
      GenericDatabaseReader,
      GenericDatabaseWriter,
    } from "convex/server";
    import type { DataModel } from "./dataModel.js";
    ${envInterface}

    /**
     * Define a query in this Convex app's public API.
     *
     * This function will be allowed to read your Convex database and will be accessible from the client.
     *
     * @param func - The query function. It receives a {@link QueryCtx} as its first argument.
     * @returns The wrapped query. Include this as an \`export\` to name it and make it accessible.
     */
    export declare const query: QueryBuilder<DataModel, "public">;

    /**
     * Define a query that is only accessible from other Convex functions (but not from the client).
     *
     * This function will be allowed to read from your Convex database. It will not be accessible from the client.
     *
     * @param func - The query function. It receives a {@link QueryCtx} as its first argument.
     * @returns The wrapped query. Include this as an \`export\` to name it and make it accessible.
     */
    export declare const internalQuery: QueryBuilder<DataModel, "internal">;

    /**
     * Define a mutation in this Convex app's public API.
     *
     * This function will be allowed to modify your Convex database and will be accessible from the client.
     *
     * @param func - The mutation function. It receives a {@link MutationCtx} as its first argument.
     * @returns The wrapped mutation. Include this as an \`export\` to name it and make it accessible.
     */
    export declare const mutation: MutationBuilder<DataModel, "public">;

    /**
     * Define a mutation that is only accessible from other Convex functions (but not from the client).
     *
     * This function will be allowed to modify your Convex database. It will not be accessible from the client.
     *
     * @param func - The mutation function. It receives a {@link MutationCtx} as its first argument.
     * @returns The wrapped mutation. Include this as an \`export\` to name it and make it accessible.
     */
    export declare const internalMutation: MutationBuilder<DataModel, "internal">;

    /**
     * Define an action in this Convex app's public API.
     *
     * An action is a function which can execute any JavaScript code, including non-deterministic
     * code and code with side-effects, like calling third-party services.
     * They can be run in Convex's JavaScript environment or in Node.js using the "use node" directive.
     * They can interact with the database indirectly by calling queries and mutations using the {@link ActionCtx}.
     *
     * @param func - The action. It receives an {@link ActionCtx} as its first argument.
     * @returns The wrapped action. Include this as an \`export\` to name it and make it accessible.
     */
    export declare const action: ActionBuilder<DataModel, "public">;

    /**
     * Define an action that is only accessible from other Convex functions (but not from the client).
     *
     * @param func - The function. It receives an {@link ActionCtx} as its first argument.
     * @returns The wrapped function. Include this as an \`export\` to name it and make it accessible.
     */
    export declare const internalAction: ActionBuilder<DataModel, "internal">;

    /**
     * Define an HTTP action.
     *
     * The wrapped function will be used to respond to HTTP requests received
     * by a Convex deployment if the requests matches the path and method where
     * this action is routed. Be sure to route your httpAction in \`convex/http.js\`.
     *
     * @param func - The function. It receives an {@link ActionCtx} as its first argument
     * and a Fetch API \`Request\` object as its second.
     * @returns The wrapped function. Import this function from \`convex/http.js\` and route it to hook it up.
     */
    export declare const httpAction: HttpActionBuilder;
${envDeclDTS}
    /**
     * A set of services for use within Convex query functions.
     *
     * The query context is passed as the first argument to any Convex query
     * function run on the server.
     *
     * This differs from the {@link MutationCtx} because all of the services are
     * read-only.
     */
    export type QueryCtx = GenericQueryCtx<DataModel>;

    /**
     * A set of services for use within Convex mutation functions.
     *
     * The mutation context is passed as the first argument to any Convex mutation
     * function run on the server.
     */
    export type MutationCtx = GenericMutationCtx<DataModel>;

    /**
     * A set of services for use within Convex action functions.
     *
     * The action context is passed as the first argument to any Convex action
     * function run on the server.
     */
    export type ActionCtx = GenericActionCtx<DataModel>;

    /**
     * An interface to read from the database within Convex query functions.
     *
     * The two entry points are {@link DatabaseReader.get}, which fetches a single
     * document by its {@link Id}, or {@link DatabaseReader.query}, which starts
     * building a query.
     */
    export type DatabaseReader = GenericDatabaseReader<DataModel>;

    /**
     * An interface to read from and write to the database within Convex mutation
     * functions.
     *
     * Convex guarantees that all writes within a single mutation are
     * executed atomically, so you never have to worry about partial writes leaving
     * your data in an inconsistent state. See [the Convex Guide](https://docs.convex.dev/understanding/convex-fundamentals/functions#atomicity-and-optimistic-concurrency-control)
     * for the guarantees Convex provides your functions.
     */
    export type DatabaseWriter = GenericDatabaseWriter<DataModel>;`;

    const serverJS = `
    ${header(
      "Generated utilities for implementing server-side Convex query and mutation functions.",
    )}
    import {
      actionGeneric,
      httpActionGeneric,
      queryGeneric,
      mutationGeneric,
      internalActionGeneric,
      internalMutationGeneric,
      internalQueryGeneric,
    } from "convex/server";

    /**
     * Define a query in this Convex app's public API.
     *
     * This function will be allowed to read your Convex database and will be accessible from the client.
     *
     * @param func - The query function. It receives a {@link QueryCtx} as its first argument.
     * @returns The wrapped query. Include this as an \`export\` to name it and make it accessible.
     */
    export const query = queryGeneric;

    /**
     * Define a query that is only accessible from other Convex functions (but not from the client).
     *
     * This function will be allowed to read from your Convex database. It will not be accessible from the client.
     *
     * @param func - The query function. It receives a {@link QueryCtx} as its first argument.
     * @returns The wrapped query. Include this as an \`export\` to name it and make it accessible.
     */
    export const internalQuery = internalQueryGeneric;

    /**
     * Define a mutation in this Convex app's public API.
     *
     * This function will be allowed to modify your Convex database and will be accessible from the client.
     *
     * @param func - The mutation function. It receives a {@link MutationCtx} as its first argument.
     * @returns The wrapped mutation. Include this as an \`export\` to name it and make it accessible.
     */
    export const mutation = mutationGeneric;

    /**
     * Define a mutation that is only accessible from other Convex functions (but not from the client).
     *
     * This function will be allowed to modify your Convex database. It will not be accessible from the client.
     *
     * @param func - The mutation function. It receives a {@link MutationCtx} as its first argument.
     * @returns The wrapped mutation. Include this as an \`export\` to name it and make it accessible.
     */
    export const internalMutation = internalMutationGeneric;

    /**
     * Define an action in this Convex app's public API.
     *
     * An action is a function which can execute any JavaScript code, including non-deterministic
     * code and code with side-effects, like calling third-party services.
     * They can be run in Convex's JavaScript environment or in Node.js using the "use node" directive.
     * They can interact with the database indirectly by calling queries and mutations using the {@link ActionCtx}.
     *
     * @param func - The action. It receives an {@link ActionCtx} as its first argument.
     * @returns The wrapped action. Include this as an \`export\` to name it and make it accessible.
     */
    export const action = actionGeneric;

    /**
     * Define an action that is only accessible from other Convex functions (but not from the client).
     *
     * @param func - The function. It receives an {@link ActionCtx} as its first argument.
     * @returns The wrapped function. Include this as an \`export\` to name it and make it accessible.
     */
    export const internalAction = internalActionGeneric;

    /**
     * Define an HTTP action.
     *
     * The wrapped function will be used to respond to HTTP requests received
     * by a Convex deployment if the requests matches the path and method where
     * this action is routed. Be sure to route your httpAction in \`convex/http.js\`.
     *
     * @param func - The function. It receives an {@link ActionCtx} as its first argument
     * and a Fetch API \`Request\` object as its second.
     * @returns The wrapped function. Import this function from \`convex/http.js\` and route it to hook it up.
     */
    export const httpAction = httpActionGeneric;
${envDeclJS}
    `;

    return {
      DTS: serverDTS,
      JS: serverJS,
    };
  } else {
    // Generate combined .ts file
    const serverTS = `${header(
      "Generated utilities for implementing server-side Convex query and mutation functions.",
    )}
import {
  actionGeneric,
  httpActionGeneric,
  queryGeneric,
  mutationGeneric,
  internalActionGeneric,
  internalMutationGeneric,
  internalQueryGeneric,
} from "convex/server";
import type {
  ActionBuilder,
  HttpActionBuilder,
  MutationBuilder,
  QueryBuilder,
  GenericActionCtx,
  GenericMutationCtx,
  GenericQueryCtx,
  GenericDatabaseReader,
  GenericDatabaseWriter,
} from "convex/server";
import type { DataModel } from "./dataModel.js";
${envInterface}

/**
 * Define a query in this Convex app's public API.
 *
 * This function will be allowed to read your Convex database and will be accessible from the client.
 *
 * @param func - The query function. It receives a {@link QueryCtx} as its first argument.
 * @returns The wrapped query. Include this as an \`export\` to name it and make it accessible.
 */
export const query: QueryBuilder<DataModel, "public"> = queryGeneric;

/**
 * Define a query that is only accessible from other Convex functions (but not from the client).
 *
 * This function will be allowed to read from your Convex database. It will not be accessible from the client.
 *
 * @param func - The query function. It receives a {@link QueryCtx} as its first argument.
 * @returns The wrapped query. Include this as an \`export\` to name it and make it accessible.
 */
export const internalQuery: QueryBuilder<DataModel, "internal"> = internalQueryGeneric;

/**
 * Define a mutation in this Convex app's public API.
 *
 * This function will be allowed to modify your Convex database and will be accessible from the client.
 *
 * @param func - The mutation function. It receives a {@link MutationCtx} as its first argument.
 * @returns The wrapped mutation. Include this as an \`export\` to name it and make it accessible.
 */
export const mutation: MutationBuilder<DataModel, "public"> = mutationGeneric;

/**
 * Define a mutation that is only accessible from other Convex functions (but not from the client).
 *
 * This function will be allowed to modify your Convex database. It will not be accessible from the client.
 *
 * @param func - The mutation function. It receives a {@link MutationCtx} as its first argument.
 * @returns The wrapped mutation. Include this as an \`export\` to name it and make it accessible.
 */
export const internalMutation: MutationBuilder<DataModel, "internal"> = internalMutationGeneric;

/**
 * Define an action in this Convex app's public API.
 *
 * An action is a function which can execute any JavaScript code, including non-deterministic
 * code and code with side-effects, like calling third-party services.
 * They can be run in Convex's JavaScript environment or in Node.js using the "use node" directive.
 * They can interact with the database indirectly by calling queries and mutations using the {@link ActionCtx}.
 *
 * @param func - The action. It receives an {@link ActionCtx} as its first argument.
 * @returns The wrapped action. Include this as an \`export\` to name it and make it accessible.
 */
export const action: ActionBuilder<DataModel, "public"> = actionGeneric;

/**
 * Define an action that is only accessible from other Convex functions (but not from the client).
 *
 * @param func - The function. It receives an {@link ActionCtx} as its first argument.
 * @returns The wrapped function. Include this as an \`export\` to name it and make it accessible.
 */
export const internalAction: ActionBuilder<DataModel, "internal"> = internalActionGeneric;

/**
 * Define an HTTP action.
 *
 * The wrapped function will be used to respond to HTTP requests received
 * by a Convex deployment if the requests matches the path and method where
 * this action is routed. Be sure to route your httpAction in \`convex/http.js\`.
 *
 * @param func - The function. It receives an {@link ActionCtx} as its first argument
 * and a Fetch API \`Request\` object as its second.
 * @returns The wrapped function. Import this function from \`convex/http.js\` and route it to hook it up.
 */
export const httpAction: HttpActionBuilder = httpActionGeneric;
${envDeclTS}
/**
 * A set of services for use within Convex query functions.
 *
 * The query context is passed as the first argument to any Convex query
 * function run on the server.
 *
 * This differs from the {@link MutationCtx} because all of the services are
 * read-only.
 */
export type QueryCtx = GenericQueryCtx<DataModel>;

/**
 * A set of services for use within Convex mutation functions.
 *
 * The mutation context is passed as the first argument to any Convex mutation
 * function run on the server.
 */
export type MutationCtx = GenericMutationCtx<DataModel>;

/**
 * A set of services for use within Convex action functions.
 *
 * The action context is passed as the first argument to any Convex action
 * function run on the server.
 */
export type ActionCtx = GenericActionCtx<DataModel>;

/**
 * An interface to read from the database within Convex query functions.
 *
 * The two entry points are {@link DatabaseReader.get}, which fetches a single
 * document by its {@link Id}, or {@link DatabaseReader.query}, which starts
 * building a query.
 */
export type DatabaseReader = GenericDatabaseReader<DataModel>;

/**
 * An interface to read from and write to the database within Convex mutation
 * functions.
 *
 * Convex guarantees that all writes within a single mutation are
 * executed atomically, so you never have to worry about partial writes leaving
 * your data in an inconsistent state. See [the Convex Guide](https://docs.convex.dev/understanding/convex-fundamentals/functions#atomicity-and-optimistic-concurrency-control)
 * for the guarantees Convex provides your functions.
 */
export type DatabaseWriter = GenericDatabaseWriter<DataModel>;`;

    return {
      TS: serverTS,
    };
  }
}

export function generateEnvInterface(envVars: EnvVarMeta[]): string {
  const lines: string[] = [];
  for (const [name, decl] of envVars) {
    const parsed = parseValidator(decl.value);
    let tsType = parsed ? validatorToType(parsed, false) : "unknown";
    if (decl.optional) {
      tsType += " | undefined";
    }
    lines.push(`  readonly ${name}: ${tsType};`);
  }
  return `
${ENV_DOCSTRING}
type Env = {
${lines.join("\n")}
};`;
}
