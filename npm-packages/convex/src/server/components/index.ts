import { PropertyValidators } from "../../values/index.js";
import { version } from "../../index.js";
import {
  AnyFunctionReference,
  FunctionReference,
  FunctionReference_future,
  FunctionType,
} from "../api.js";
import { performAsyncSyscall } from "../impl/syscall.js";
import { DefaultFunctionArgs } from "../registration.js";
import {
  AppDefinitionAnalysis,
  ComponentDefinitionAnalysis,
  ComponentDefinitionType,
} from "./definition.js";
import {
  getFunctionAddress,
  setReferencePath,
  toReferencePath,
} from "./paths.js";
import type {
  Validator,
  VLiteral,
  VOptional,
  VString,
  VUnion,
} from "../../values/validators.js";
import type { Infer } from "../../values/validator.js";
import { isValidator } from "../../values/validator.js";
import type { Expand } from "../../type_utils.js";
export { getFunctionAddress } from "./paths.js";

/**
 * A serializable reference to a Convex function.
 * Passing a this reference to another component allows that component to call this
 * function during the current function execution or at any later time.
 * Function handles are used like `api.folder.function` FunctionReferences,
 * e.g. `ctx.scheduler.runAfter(0, functionReference, args)`.
 *
 * A function reference is stable across code pushes but it's possible
 * the Convex function it refers to might no longer exist.
 *
 * This is a feature of components, which are in beta.
 * This API is unstable and may change in subsequent releases.
 */
export type FunctionHandle<
  Type extends FunctionType,
  Args extends DefaultFunctionArgs = any,
  ReturnType = any,
> = string & FunctionReference<Type, "internal", Args, ReturnType>;

/**
 * A {@link FunctionHandle} with the type semantics of {@link FunctionReference_future}.
 *
 * @public
 */
export type FunctionHandle_future<
  Type extends FunctionType,
  Args extends DefaultFunctionArgs = any,
  ReturnType = any,
> = string & FunctionReference_future<Type, "internal", Args, ReturnType>;

/**
 * Create a serializable reference to a Convex function.
 * Passing a this reference to another component allows that component to call this
 * function during the current function execution or at any later time.
 * Function handles are used like `api.folder.function` FunctionReferences,
 * e.g. `ctx.scheduler.runAfter(0, functionReference, args)`.
 *
 * A function reference is stable across code pushes but it's possible
 * the Convex function it refers to might no longer exist.
 *
 * This is a feature of components, which are in beta.
 * This API is unstable and may change in subsequent releases.
 */
export async function createFunctionHandle<
  Type extends FunctionType,
  Args extends DefaultFunctionArgs,
  ReturnType,
>(
  functionReference:
    | FunctionReference<Type, "public" | "internal", Args, ReturnType>
    | FunctionReference_future<Type, "public" | "internal", Args, ReturnType>,
): Promise<FunctionHandle<Type, Args, ReturnType>> {
  const address = getFunctionAddress(functionReference);
  return await performAsyncSyscall("1.0/createFunctionHandle", {
    ...address,
    version,
  });
}

interface ComponentExports {
  [key: string]: FunctionReference<any, any, any, any> | ComponentExports;
}

/**
 * An object of this type should be the default export of a
 * convex.config.ts file in a component definition directory.
 *
 * This is a feature of components, which are in beta.
 * This API is unstable and may change in subsequent releases.
 */
export type ComponentDefinition<
  Exports extends ComponentExports = any,
  Env extends EnvDefinition = {},
> = {
  /**
   * Install a component with the given definition in this component definition.
   *
   * Takes a component definition and an optional name.
   *
   * For editor tooling this method expects a {@link ComponentDefinition}
   * but at runtime the object that is imported will be a {@link ImportedComponentDefinition}
   */
  use<Definition extends ComponentDefinition<any, any>>(
    definition: Definition,
    ...args: UseArgs<Definition>
  ): InstalledComponent<Definition>;

  /**
   * Internal type-only property tracking exports provided.
   *
   * @deprecated This is a type-only property, don't use it.
   */
  __exports: Exports;

  /**
   * References to this component's declared env vars. Pass one of these in
   * `app.use(child, { env: { ... } })` to bind a child's env var by
   * reference to this component's env var.
   */
  env: EnvRefFromDefinition<Env>;

  /**
   * Internal type-only property tracking env definition.
   *
   * @deprecated This is a type-only property, don't use it.
   */
  __env: Env;
};

type ComponentDefinitionExports<T extends ComponentDefinition<any, any>> =
  T["__exports"];

type ComponentDefinitionEnv<T extends ComponentDefinition<any, any>> =
  T["__env"];

/**
 * The names in an {@link EnvDefinition} whose validators are required (not
 * wrapped in `v.optional(...)`).
 */
type RequiredEnvKeys<E extends EnvDefinition> = {
  [K in keyof E]: E[K] extends Validator<any, "optional", any> ? never : K;
}[keyof E];

/**
 * Options for installing a component via `app.use()` or `component.use()`.
 *
 * If the component declares any required env vars, the `env` property is
 * required. Otherwise it is optional, so that a component with no env vars (or
 * only optional ones) can be installed without passing `env`.
 */
type UseOptions<Definition extends ComponentDefinition<any, any>> =
  RequiredEnvKeys<ComponentDefinitionEnv<Definition>> extends never
    ? {
        name?: string;
        httpPrefix?: string;
        env?: UseOptionsEnv<ComponentDefinitionEnv<Definition>>;
      }
    : {
        name?: string;
        httpPrefix?: string;
        env: UseOptionsEnv<ComponentDefinitionEnv<Definition>>;
      };

/**
 * The arguments to `use()` after the component definition.
 *
 * When the component declares required env vars the `options` argument is
 * mandatory (so it can't be dropped to skip the required `env`); otherwise it
 * is optional.
 */
type UseArgs<Definition extends ComponentDefinition<any, any>> =
  RequiredEnvKeys<ComponentDefinitionEnv<Definition>> extends never
    ? [options?: UseOptions<Definition>]
    : [options: UseOptions<Definition>];

type UseOptionsEnv<E extends EnvDefinition> = Expand<
  {
    [K in keyof E as E[K] extends Validator<any, "optional", any> ? never : K]:
      | Infer<E[K]>
      | EnvRef;
  } & {
    [K in keyof E as E[K] extends Validator<any, "optional", any>
      ? K
      : never]?: Infer<E[K]> | EnvRef | undefined;
  }
>;

/**
 * A string-like validator: `v.string()`, a string `v.literal("...")`, or a
 * `v.union(...)` of those (recursively). Component env vars are serialized
 * as strings on the wire, so only string-typed validators are allowed.
 *
 * @public
 */
export type StringLikeValidator =
  | VString<string, "required">
  | VLiteral<string, "required">
  | VUnion<string, Validator<any, "required", any>[], "required">;

/**
 * A definition of environment variables for the app.
 *
 * Maps environment variable names to string-like validators. Use
 * `v.string()` for a plain string, `v.literal("a")` for an enum value, or
 * `v.union(v.literal("a"), v.literal("b"))` for an enum. Wrap in
 * `v.optional(...)` for optional vars.
 *
 * @example
 * ```typescript
 * import { defineApp } from "convex/server";
 * import { v } from "convex/values";
 *
 * const app = defineApp({
 *   env: {
 *     OPENAI_API_KEY: v.string(),
 *     DEBUG_MODE: v.optional(v.string()),
 *   },
 * });
 * ```
 *
 * @public
 */
export type EnvDefinition = Record<
  string,
  StringLikeValidator | VOptional<StringLikeValidator>
>;

/**
 * Compute the typed environment object from an {@link EnvDefinition}.
 *
 * Required entries get the validator's inferred string type; optional
 * entries are `T | undefined`.
 *
 * @public
 */
export type EnvFromDefinition<E extends EnvDefinition> = Expand<
  {
    [K in keyof E as E[K] extends Validator<any, "optional", any>
      ? never
      : K]: Infer<E[K]>;
  } & {
    [K in keyof E as E[K] extends Validator<any, "optional", any>
      ? K
      : never]?: Infer<E[K]> | undefined;
  }
>;

/**
 * A reference to a parent-declared env var, produced by `app.env.<NAME>` or
 * `component.env.<NAME>`. Pass this in `use(child, { env: { ... } })` to
 * bind a child's declared env var to the parent's env var by reference
 * instead of snapshotting its current value.
 *
 * @public
 */
export type EnvRef<K extends string = string> = { __envVarRef: K };

/**
 * Compute the typed `env` namespace object from an {@link EnvDefinition}.
 * Each declared name maps to an {@link EnvRef} for that name.
 *
 * @public
 */
export type EnvRefFromDefinition<E extends EnvDefinition> = {
  [K in keyof E & string]: EnvRef<K>;
};

/**
 * Extract the typed environment from an {@link AppDefinition}.
 *
 * @public
 */
export type EnvFromAppDefinition<A> =
  A extends AppDefinition<infer E>
    ? EnvFromDefinition<E>
    : Record<string, never>;

/**
 * An object of this type should be the default export of a
 * convex.config.ts file in a component-aware convex directory.
 *
 * This is a feature of components, which are in beta.
 * This API is unstable and may change in subsequent releases.
 */
export type AppDefinition<Env extends EnvDefinition = EnvDefinition> = {
  /**
   * Install a component with the given definition in this component definition.
   *
   * Takes a component definition and an optional name.
   *
   * For editor tooling this method expects a {@link ComponentDefinition}
   * but at runtime the object that is imported will be a {@link ImportedComponentDefinition}
   */
  use<Definition extends ComponentDefinition<any, any>>(
    definition: Definition,
    ...args: UseArgs<Definition>
  ): InstalledComponent<Definition>;

  /**
   * References to this app's declared env vars. Pass one of these in
   * `app.use(child, { env: { ... } })` to bind a child's env var by
   * reference to this app's env var.
   */
  env: EnvRefFromDefinition<Env>;

  /**
   * Internal type-only property tracking env definition.
   *
   * @deprecated This is a type-only property, don't use it.
   */
  __env: Env;
};

interface ExportTree {
  // Tree with serialized `Reference`s as leaves.
  [key: string]: string | ExportTree;
}

type CommonDefinitionData = {
  _isRoot: boolean;
  _childComponents: [
    string,
    ImportedComponentDefinition,
    Record<string, any> | null,
    string | undefined,
  ][];
  _exportTree: ExportTree;
};

type ComponentDefinitionData = CommonDefinitionData & {
  _env: PropertyValidators;
  _name: string;
  _onInitCallbacks: Record<string, (argsStr: string) => string>;
};
type AppDefinitionData = CommonDefinitionData & {
  _httpPrefix?: string;
  _env?: EnvDefinition;
};

/**
 * Used to refer to an already-installed component.
 */
class InstalledComponent<Definition extends ComponentDefinition<any, any>> {
  /**
   * @internal
   */
  _definition: Definition;

  /**
   * @internal
   */
  _name: string;

  constructor(definition: Definition, name: string) {
    this._definition = definition;
    this._name = name;
    setReferencePath(this, `_reference/childComponent/${name}`);
  }

  get exports(): ComponentDefinitionExports<Definition> {
    return createExports(this._name, []);
  }
}

function createExports(name: string, pathParts: string[]): any {
  const handler: ProxyHandler<any> = {
    get(_, prop: string | symbol) {
      if (typeof prop === "string") {
        const newParts = [...pathParts, prop];
        return createExports(name, newParts);
      } else if (prop === toReferencePath) {
        let reference = `_reference/childComponent/${name}`;
        for (const part of pathParts) {
          reference += `/${part}`;
        }
        return reference;
      } else {
        return undefined;
      }
    },
  };
  return new Proxy({}, handler);
}

function createEnvRefs(
  ownerLabel: string,
  declared: Record<string, any> | undefined,
): any {
  const handler: ProxyHandler<any> = {
    get(_, prop: string | symbol) {
      if (typeof prop !== "string") {
        return undefined;
      }
      if (!declared || !Object.prototype.hasOwnProperty.call(declared, prop)) {
        throw new Error(
          `Env var "${prop}" is not declared on ${ownerLabel}. Add it to the \`env\` option of ${ownerLabel === "this app" ? "defineApp" : "defineComponent"}.`,
        );
      }
      return { __envVarRef: prop };
    },
  };
  return new Proxy({}, handler);
}

function isEnvRef(value: unknown): value is EnvRef {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as EnvRef).__envVarRef === "string"
  );
}

function use<Definition extends ComponentDefinition<any, any>>(
  this: CommonDefinitionData,
  definition: Definition,
  options?: {
    name?: string;
    httpPrefix?: string;
    env?: Record<string, any>;
  },
): InstalledComponent<Definition> {
  // At runtime an imported component will have this shape.
  const importedComponentDefinition =
    definition as unknown as ImportedComponentDefinition;
  if (typeof importedComponentDefinition.componentDefinitionPath !== "string") {
    throw new Error(
      "Component definition does not have the required componentDefinitionPath property. This code only works in Convex runtime.",
    );
  }
  const name =
    options?.name ??
    // added recently
    importedComponentDefinition.defaultName ??
    // can be removed once backend is out
    importedComponentDefinition.componentDefinitionPath.split("/").pop()!;

  if (typeof name !== "string") {
    throw new Error(
      `Component name must be a string. Received: ${typeof name}`,
    );
  }
  if (name.length === 0) {
    // "" is used internally as the name for the root component, so
    // users shouldn't try to define child components with an empty name.
    throw new Error("Component name cannot be empty.");
  }

  const httpPrefix = options?.httpPrefix;
  if (httpPrefix !== undefined) {
    if (!httpPrefix.startsWith("/")) {
      throw new Error(
        `httpPrefix must start with "/". Received: "${httpPrefix}"`,
      );
    }
  }

  const envValues: Record<string, any> = {};
  if (options?.env) {
    for (const [key, value] of Object.entries(options.env)) {
      if (value !== undefined) {
        envValues[key] = value;
      }
    }
  }

  this._childComponents.push([
    name,
    importedComponentDefinition,
    envValues,
    httpPrefix,
  ]);
  return new InstalledComponent(definition, name);
}

/**
 * The runtime type of a ComponentDefinition. TypeScript will claim
 * the default export of a module like "cool-component/convex.config.js"
 * is a `@link ComponentDefinition}, but during component definition evaluation
 * this is its type instead.
 *
 * This is a feature of components, which are in beta.
 * This API is unstable and may change in subsequent releases.
 */
export type ImportedComponentDefinition = {
  componentDefinitionPath: string;
  defaultName: string;
};

function exportAppForAnalysis(
  this: ComponentDefinition<any, any> & AppDefinitionData,
): AppDefinitionAnalysis {
  const definitionType = { type: "app" as const };
  const childComponents = serializeChildComponents(this._childComponents);
  const httpMounts = buildHttpMounts(this._childComponents);
  const envVars = this._env
    ? Object.entries(this._env).map(
        ([name, validator]) =>
          [
            name,
            {
              type: "value" as const,
              value: JSON.stringify(validator.json),
              ...(validator.isOptional === "optional"
                ? { optional: true }
                : {}),
            },
          ] as [string, { type: "value"; value: string; optional?: boolean }],
      )
    : undefined;
  return {
    definitionType,
    ...(this._httpPrefix !== undefined
      ? { httpPrefix: normalizeHttpPrefix(this._httpPrefix) }
      : {}),
    childComponents: childComponents as any,
    httpMounts,
    exports: serializeExportTree(this._exportTree),
    ...(envVars !== undefined ? { envVars } : {}),
  };
}

function serializeExportTree(tree: ExportTree): any {
  const branch: any[] = [];
  for (const [key, child] of Object.entries(tree)) {
    let node;
    if (typeof child === "string") {
      node = { type: "leaf", leaf: child };
    } else {
      node = serializeExportTree(child);
    }
    branch.push([key, node]);
  }
  return { type: "branch", branch };
}

function normalizeHttpPrefix(prefix: string): string {
  // Ensure the prefix ends with "/" as required by HttpMountPath in Rust.
  return prefix.endsWith("/") ? prefix : prefix + "/";
}

function buildHttpMounts(
  childComponents: [
    string,
    ImportedComponentDefinition,
    Record<string, any> | null,
    string | undefined,
  ][],
): Record<string, string> {
  const httpMounts: Record<string, string> = {};
  for (const [name, , , httpPrefix] of childComponents) {
    if (httpPrefix !== undefined) {
      const normalized = normalizeHttpPrefix(httpPrefix);
      httpMounts[normalized] = `_reference/childComponent/${name}`;
    }
  }
  return httpMounts;
}

type SerializedEnvArg =
  | { type: "value"; value: string }
  | { type: "envVar"; name: string };

function serializeChildComponents(
  childComponents: [
    string,
    ImportedComponentDefinition,
    Record<string, any> | null,
    string | undefined,
  ][],
): {
  name: string;
  path: string;
  env: [string, SerializedEnvArg][] | null;
}[] {
  return childComponents.map(([name, definition, p]) => {
    // Note: httpPrefix (4th element) is used separately in buildHttpMounts()
    let env: [string, SerializedEnvArg][] | null = null;
    if (p !== null) {
      env = [];
      for (const [name, value] of Object.entries(p)) {
        if (value === undefined) {
          continue;
        }
        if (isEnvRef(value)) {
          env.push([name, { type: "envVar", name: value.__envVarRef }]);
        } else if (typeof value === "string") {
          env.push([name, { type: "value", value }]);
        } else {
          throw new Error(
            `Env var "${name}" must be a string or an env var reference. ` +
              `Received: ${typeof value}`,
          );
        }
      }
    }
    // we know that components carry this extra information
    const path = definition.componentDefinitionPath;
    if (!path)
      throw new Error(
        "no .componentPath for component definition " +
          JSON.stringify(definition, null, 2),
      );

    return {
      name: name!,
      path: path!,
      args: [],
      env,
    };
  });
}

function exportComponentForAnalysis(
  this: ComponentDefinition<any, any> & ComponentDefinitionData,
): ComponentDefinitionAnalysis {
  const envVars = Object.entries(this._env).map(
    ([name, validator]) =>
      [
        name,
        {
          type: "value" as const,
          value: JSON.stringify((validator as any).json),
          ...((validator as any).isOptional === "optional"
            ? { optional: true }
            : {}),
        },
      ] as [string, { type: "value"; value: string; optional?: boolean }],
  );
  const definitionType: ComponentDefinitionType = {
    type: "childComponent" as const,
    name: this._name,
    args: [],
  };
  const childComponents = serializeChildComponents(this._childComponents);
  const httpMounts = buildHttpMounts(this._childComponents);
  return {
    name: this._name,
    definitionType,
    childComponents: childComponents as any,
    httpMounts,
    exports: serializeExportTree(this._exportTree),
    ...(envVars.length > 0 ? { envVars } : {}),
  };
}

// This is what is actually contained in a ComponentDefinition.
type RuntimeComponentDefinition = Omit<
  ComponentDefinition<any, any>,
  "__exports" | "__env"
> &
  ComponentDefinitionData & {
    export: () => ComponentDefinitionAnalysis;
  };
type RuntimeAppDefinition = Omit<AppDefinition<any>, "__env"> &
  AppDefinitionData & {
    export: () => AppDefinitionAnalysis;
  };

/**
 * Define a component, a piece of a Convex deployment with namespaced resources.
 *
 * Optionally define typed environment variables that will be available via
 * the `env` export from `_generated/server` in all Convex functions within
 * this component. Values are passed by the parent via
 * `app.use(component, { env: { ... } })`.
 *
 * @param name Name must be alphanumeric plus underscores. Typically these are
 * lowercase with underscores like `"onboarding_flow_tracker"`.
 *
 * This is a feature of components, which are in beta.
 * This API is unstable and may change in subsequent releases.
 */
export function defineComponent<
  Exports extends ComponentExports = any,
  const Env extends EnvDefinition = {},
>(
  name: string,
  options?: {
    env?: Env;
  },
): ComponentDefinition<Exports, Env> {
  const envValidators: PropertyValidators = {};
  if (options?.env) {
    for (const [key, decl] of Object.entries(options.env)) {
      if (decl !== null && decl !== undefined && isValidator(decl)) {
        envValidators[key] = decl as any;
      } else {
        throw new Error(
          `Environment variable "${key}" must be defined with a validator (e.g. v.string()).`,
        );
      }
    }
  }

  const ret: RuntimeComponentDefinition = {
    _isRoot: false,
    _name: name,
    _env: envValidators,
    _childComponents: [],
    _exportTree: {},
    _onInitCallbacks: {},

    env: createEnvRefs(`component "${name}"`, options?.env),

    export: exportComponentForAnalysis,
    use,

    ...({} as { __exports: any; __env: any }),
  };
  return ret as any as ComponentDefinition<Exports, Env>;
}

/**
 * Attach components, reuseable pieces of a Convex deployment, to this Convex app.
 *
 * Optionally define typed environment variables that will be available via
 * the `env` export from `_generated/server` in all Convex functions.
 *
 * @example
 * ```typescript
 * import { defineApp } from "convex/server";
 * import { v } from "convex/values";
 *
 * const app = defineApp({
 *   env: {
 *     OPENAI_API_KEY: v.string(),
 *     DEBUG_MODE: v.optional(v.string()),
 *   },
 * });
 * export default app;
 * ```
 *
 * This is a feature of components, which are in beta.
 * This API is unstable and may change in subsequent releases.
 */
export function defineApp<Env extends EnvDefinition = EnvDefinition>(options?: {
  httpPrefix?: string;
  env?: Env;
}): AppDefinition<Env> {
  const httpPrefix = options?.httpPrefix;
  if (httpPrefix !== undefined && !httpPrefix.startsWith("/")) {
    throw new Error(
      `httpPrefix must start with "/". Received: "${httpPrefix}"`,
    );
  }
  const env = options?.env;
  if (env !== undefined) {
    for (const [name, validator] of Object.entries(env)) {
      if (!isValidator(validator)) {
        throw new Error(
          `Environment variable "${name}" must be defined with a validator (e.g. v.string()).`,
        );
      }
    }
  }
  const ret: RuntimeAppDefinition = {
    _isRoot: true,
    _childComponents: [],
    _exportTree: {},
    ...(httpPrefix !== undefined ? { _httpPrefix: httpPrefix } : {}),
    ...(env !== undefined ? { _env: env } : {}),

    env: createEnvRefs("this app", env),

    export: exportAppForAnalysis,
    use,
  };
  return ret as unknown as AppDefinition<Env>;
}

type AnyInterfaceType = {
  [key: string]: AnyInterfaceType;
} & AnyFunctionReference;
export type AnyComponentReference = Record<string, AnyInterfaceType>;

export type AnyChildComponents = Record<string, AnyComponentReference>;

/**
 * @internal
 */
export function currentSystemUdfInComponent(
  componentId: string,
): AnyComponentReference {
  return {
    [toReferencePath]: `_reference/currentSystemUdfInComponent/${componentId}`,
  };
}

function createChildComponents(
  root: string,
  pathParts: string[],
): AnyChildComponents {
  const handler: ProxyHandler<object> = {
    get(_, prop: string | symbol) {
      if (typeof prop === "string") {
        const newParts = [...pathParts, prop];
        return createChildComponents(root, newParts);
      } else if (prop === toReferencePath) {
        if (pathParts.length < 1) {
          const found = [root, ...pathParts].join(".");
          throw new Error(
            `API path is expected to be of the form \`${root}.childComponent.functionName\`. Found: \`${found}\``,
          );
        }
        return `_reference/childComponent/` + pathParts.join("/");
      } else {
        return undefined;
      }
    },
  };
  return new Proxy({}, handler);
}

export const componentsGeneric = () => createChildComponents("components", []);

export type AnyComponents = AnyChildComponents;
