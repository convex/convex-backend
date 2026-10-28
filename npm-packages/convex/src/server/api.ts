import {
  EmptyObject,
  DefaultFunctionArgs,
  FunctionVisibility,
  RegisteredAction,
  RegisteredMutation,
  RegisteredQuery,
} from "./registration.js";
import { Expand, UnionToIntersection } from "../type_utils.js";
import { PaginationOptions, PaginationResult } from "./pagination.js";
import { functionName } from "./functionName.js";
import { getFunctionAddress } from "./components/paths.js";

/**
 * The type of a Convex function.
 *
 * @public
 */
export type FunctionType = "query" | "mutation" | "action";

/**
 * A reference to a registered Convex function.
 *
 * You can create a {@link FunctionReference} using the generated `api` utility:
 * ```js
 * import { api } from "../convex/_generated/api";
 *
 * const reference = api.myModule.myFunction;
 * ```
 *
 * If you aren't using code generation, you can create references using
 * {@link anyApi}:
 * ```js
 * import { anyApi } from "convex/server";
 *
 * const reference = anyApi.myModule.myFunction;
 * ```
 *
 * Function references can be used to invoke functions from the client. For
 * example, in React you can pass references to the {@link react.useQuery} hook:
 * ```js
 * const result = useQuery(api.myModule.myFunction);
 * ```
 *
 * If you want to accept a `FunctionReference` as a callback argument, prefer
 * typing the callback parameter as {@link FunctionReference_future}.
 *
 * @typeParam Type - The type of the function ("query", "mutation", or "action").
 * @typeParam Visibility - The visibility of the function ("public" or "internal").
 * @typeParam Args - The arguments to this function. This is an object mapping
 * argument names to their types.
 * @typeParam ReturnType - The return type of this function.
 * @public
 */
export type FunctionReference<
  Type extends FunctionType,
  Visibility extends FunctionVisibility = "public",
  Args extends DefaultFunctionArgs = any,
  ReturnType = any,
  ComponentPath = string | undefined,
> = {
  _type: Type;
  _visibility: Visibility;
  /**
   * To read the arguments for a `FunctionReference`, prefer
   * {@link FunctionArgs}.
   *
   * This slot will be removed in a future version.
   *
   * @deprecated
   */
  _args: Args;
  /**
   * To read the return type of a `FunctionReference`, prefer
   * {@link FunctionReturnType}.
   *
   * This slot will be removed in a future version.
   *
   * @deprecated
   */
  _returnType: ReturnType;
  _componentPath: ComponentPath;
  /**
   * To read the arguments or return type of a `FunctionReference`, prefer
   * {@link FunctionArgs} and {@link FunctionReturnType}.
   */
  // This phantom slot exists so that a `FunctionReference` can be checked
  // against a `FunctionReference_future`. It changes nothing about how two
  // `FunctionReference`s compare with each other.
  //
  // The slot models the function itself: it puts `Args` in a contravariant
  // position and `ReturnType` in a covariant one, so the slot alone is enough
  // to compare a reference the way TypeScript compares two functions.
  //
  // In that comparison the `keys` parameter has an important role. It carries
  // the keys of `Args`, so that checking a `FunctionReference` against a
  // `FunctionReference_future` also checks that every key the consumer passes
  // is a key the function declares. TypeScript treats a surplus property as
  // harmless, but a Convex argument validator rejects it, so this is what
  // makes the comparison match the runtime. See `FunctionReferenceArgKeys`.
  _fn?(args: Args, keys: FunctionReferenceArgKeys<Args>): ReturnType;
};

/**
 * The keys of a reference's arguments.
 *
 * `0 extends 1 & Args` holds only when `Args` is `any`. That maps to `any`
 * rather than `keyof any` so that an untyped reference neither imposes nor
 * fails the key check in either direction. Arguments with an index signature,
 * such as `EmptyObject` and `DefaultFunctionArgs`, map to `string`, which
 * accepts every key and leaves the decision to the first parameter.
 */
type ArgKeys<Args> = 0 extends 1 & Args ? any : keyof Args;

/**
 * The second parameter of `FunctionReference._fn`.
 *
 * Wrapping the keys in a function of a function puts them two parameters deep,
 * so comparing this parameter contravariantly checks that every key the
 * consumer passes is one the function declares. That is the check a Convex
 * argument validator performs and an ordinary function-type comparison skips.
 *
 * The union with {@link UncheckedArgKeys} is what keeps two
 * `FunctionReference`s comparing as they did before this slot existed. Every
 * `FunctionReferenceArgKeys` is assignable to `UncheckedArgKeys`, so between
 * two `FunctionReference`s the keys always relate and only `_args` decides.
 * {@link FunctionReference_futureArgKeys} is not assignable to it, so a
 * `FunctionReference` compared against a `FunctionReference_future` has to
 * satisfy the real check.
 */
type FunctionReferenceArgKeys<Args> = 0 extends 1 & Args
  ? any
  : ((keys: (key: ArgKeys<Args>) => void) => void) | UncheckedArgKeys;

/**
 * A keys parameter that every {@link FunctionReferenceArgKeys} is assignable
 * to, and that no {@link FunctionReference_futureArgKeys} is: `unknown` for
 * the second parameter accepts the former's missing parameter and rejects the
 * latter's `never`.
 */
type UncheckedArgKeys = (keys: never, unchecked?: unknown) => void;

/**
 * The second parameter of `FunctionReference_future._fn`. The same shape as
 * {@link FunctionReferenceArgKeys} plus a `guard` parameter that keeps it out
 * of {@link UncheckedArgKeys}.
 */
type FunctionReference_futureArgKeys<Args> = 0 extends 1 & Args
  ? any
  : (keys: (key: ArgKeys<Args>) => void, guard?: never) => void;

/**
 * A reference to a Convex function whose arguments are checked closer to the
 * way Convex checks them at runtime.
 *
 * Use this instead of {@link FunctionReference} when you accept someone else's
 * Convex function as a callback and know which arguments you will pass it.
 *
 * A plain `FunctionReference` gets a couple things backwards: it accepts a
 * function requiring arguments you never pass, and rejects a function
 * accepting broader values than you pass. An ordinary TypeScript function type
 * doesn't map perfectly either: it treats a surplus argument as harmless,
 * where a Convex validator rejects it.
 *
 * This type melds regular TypeScript function reference behavior with top
 * level checking of arguments to prevent surplus arguments from hitting
 * runtime validation errors. The caveat: a surplus key inside a nested object,
 * an array element, or one arm of a union passes the type check and fails the
 * validator at runtime.
 *
 * ```ts
 * import { FunctionReference_future } from "convex/server";
 *
 * declare function onComplete(
 *   fn: FunctionReference_future<
 *     "mutation",
 *     "internal",
 *     { taskId: string; force: boolean },
 *     null
 *   >,
 * ): void;
 *
 * // Takes exactly `{ taskId: string; force: boolean }`.
 * onComplete(internal.tasks.finish);
 * // Takes `{ taskId: string | number; force?: boolean }`: calling it is safe.
 * onComplete(internal.tasks.finishLoosely);
 * // Requires a `reason` argument that `onComplete` never passes.
 * onComplete(internal.tasks.finishWithReason);
 * //          ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~ rejected
 * // Takes only `{ taskId: string }`: its validator would reject `force`.
 * onComplete(internal.tasks.finishById);
 * //          ~~~~~~~~~~~~~~~~~~~~~~~~ rejected
 * ```
 *
 * A value of this type is usable with `ctx.runMutation`,
 * `ctx.scheduler.runAfter`, `createFunctionHandle`, and everything else in
 * this package that takes a reference, all of which accept either kind.
 *
 * It is *not* assignable to a plain `FunctionReference`. Code that wants to
 * accept both kinds should take
 * `FunctionReference<...> | FunctionReference_future<...>` and read arguments
 * and return types through {@link FunctionArgs} and {@link FunctionReturnType}.
 *
 * Argument checking here relies on `strictFunctionTypes` (implied by `strict`)
 * and is skipped for projects that disable it. Everything else about the
 * reference is compared exactly as `FunctionReference` compares it.
 *
 * @typeParam Type - The type of the function ("query", "mutation", or "action").
 * @typeParam Visibility - The visibility of the function ("public" or "internal").
 * @typeParam Args - The arguments the consumer of the reference will pass.
 * @typeParam ReturnType - The return type of this function.
 * @public
 */
export type FunctionReference_future<
  Type extends FunctionType,
  Visibility extends FunctionVisibility = "public",
  Args extends DefaultFunctionArgs = any,
  ReturnType = any,
  ComponentPath = string | undefined,
> = {
  _type: Type;
  _visibility: Visibility;
  _componentPath: ComponentPath;
  // Declared as a *property* rather than a method: TypeScript compares method
  // parameters bivariantly and would not check anything. The explicit
  // `| undefined` is needed because an optional method on the source side
  // (`FunctionReference._fn`) carries a real `undefined` in its type, which
  // `exactOptionalPropertyTypes` would otherwise reject.
  //
  // Both parameters are compared contravariantly: the first checks that the
  // consumer's arguments satisfy the function's, the second that the consumer
  // passes no key the function does not declare. See
  // `FunctionReference_futureArgKeys`.
  _fn?:
    | ((args: Args, keys: FunctionReference_futureArgKeys<Args>) => ReturnType)
    | undefined;
};

/**
 * Get the name of a function from a {@link FunctionReference}.
 *
 * The name is a string like "myDir/myModule:myFunction". If the exported name
 * of the function is `"default"`, the function name is omitted
 * (e.g. "myDir/myModule").
 *
 * @param functionReference - A {@link FunctionReference} to get the name of.
 * @returns A string of the function's name.
 *
 * @public
 */
export function getFunctionName(
  functionReference:
    | FunctionReference<any, any>
    | FunctionReference_future<any, any>,
): string {
  const address = getFunctionAddress(functionReference);

  if (address.name === undefined) {
    if (address.functionHandle !== undefined) {
      throw new Error(
        `Expected function reference like "api.file.func" or "internal.file.func", but received function handle ${address.functionHandle}`,
      );
    } else if (address.reference !== undefined) {
      throw new Error(
        `Expected function reference in the current component like "api.file.func" or "internal.file.func", but received reference ${address.reference}`,
      );
    }
    throw new Error(
      `Expected function reference like "api.file.func" or "internal.file.func", but received ${JSON.stringify(address)}`,
    );
  }
  // Both a legacy thing and also a convenience for interactive use:
  // the types won't check but a string is always allowed at runtime.
  if (typeof functionReference === "string") return functionReference;

  // Two different runtime values for FunctionReference implement this
  // interface: api objects returned from `createApi()` and standalone
  // function reference objects returned from makeFunctionReference.
  const name = (functionReference as any)[functionName];
  if (!name) {
    throw new Error(`${functionReference as any} is not a functionReference`);
  }
  return name;
}

/**
 * FunctionReferences generally come from generated code, but in custom clients
 * it may be useful to be able to build one manually.
 *
 * Real function references are empty objects at runtime, but the same interface
 * can be implemented with an object for tests and clients which don't use
 * code generation.
 *
 * @param name - The identifier of the function. E.g. `path/to/file:functionName`
 * @public
 */
export function makeFunctionReference<
  type extends FunctionType,
  args extends DefaultFunctionArgs = any,
  ret = any,
>(name: string): FunctionReference<type, "public", args, ret> {
  return { [functionName]: name } as unknown as FunctionReference<
    type,
    "public",
    args,
    ret
  >;
}

/**
 * Create a runtime API object that implements {@link AnyApi}.
 *
 * This allows accessing any path regardless of what directories, modules,
 * or functions are defined.
 *
 * @param pathParts - The path to the current node in the API.
 * @returns An {@link AnyApi}
 * @public
 */
function createApi(pathParts: string[] = []): AnyApi {
  const handler: ProxyHandler<object> = {
    get(_, prop: string | symbol) {
      if (typeof prop === "string") {
        const newParts = [...pathParts, prop];
        return createApi(newParts);
      } else if (prop === functionName) {
        if (pathParts.length < 2) {
          const found = ["api", ...pathParts].join(".");
          throw new Error(
            `API path is expected to be of the form \`api.moduleName.functionName\`. Found: \`${found}\``,
          );
        }
        const path = pathParts.slice(0, -1).join("/");
        const exportName = pathParts[pathParts.length - 1];
        if (exportName === "default") {
          return path;
        } else {
          return path + ":" + exportName;
        }
      } else if (prop === Symbol.toStringTag) {
        return "FunctionReference";
      } else {
        return undefined;
      }
    },
  };

  return new Proxy({}, handler);
}

/**
 * Given an export from a module, convert it to a {@link FunctionReference}
 * if it is a Convex function.
 */
export type FunctionReferenceFromExport<Export> =
  Export extends RegisteredQuery<
    infer Visibility,
    infer Args,
    infer ReturnValue
  >
    ? FunctionReference<
        "query",
        Visibility,
        Args,
        ConvertReturnType<ReturnValue>
      >
    : Export extends RegisteredMutation<
          infer Visibility,
          infer Args,
          infer ReturnValue
        >
      ? FunctionReference<
          "mutation",
          Visibility,
          Args,
          ConvertReturnType<ReturnValue>
        >
      : Export extends RegisteredAction<
            infer Visibility,
            infer Args,
            infer ReturnValue
          >
        ? FunctionReference<
            "action",
            Visibility,
            Args,
            ConvertReturnType<ReturnValue>
          >
        : never;

/**
 * Given a module, convert all the Convex functions into
 * {@link FunctionReference}s and remove the other exports.
 *
 * BE CAREFUL WHEN EDITING THIS!
 *
 * This is written carefully to preserve jumping to function definitions using
 * cmd+click. If you edit it, please test that cmd+click still works.
 */
type FunctionReferencesInModule<Module extends Record<string, any>> = {
  -readonly [ExportName in keyof Module as Module[ExportName]["isConvexFunction"] extends true
    ? ExportName
    : never]: FunctionReferenceFromExport<Module[ExportName]>;
};

/**
 * Given a path to a module and it's type, generate an API type for this module.
 *
 * This is a nested object according to the module's path.
 */
type ApiForModule<
  ModulePath extends string,
  Module extends object,
> = ModulePath extends `${infer First}/${infer Second}`
  ? {
      [_ in First]: ApiForModule<Second, Module>;
    }
  : { [_ in ModulePath]: FunctionReferencesInModule<Module> };

/**
 * Given the types of all modules in the `convex/` directory, construct the type
 * of `api`.
 *
 * `api` is a utility for constructing {@link FunctionReference}s.
 *
 * @typeParam AllModules - A type mapping module paths (like `"dir/myModule"`) to
 * the types of the modules.
 * @public
 */
export type ApiFromModules<AllModules extends Record<string, object>> =
  FilterApi<
    ApiFromModulesAllowEmptyNodes<AllModules>,
    FunctionReference<any, any, any, any>
  >;

type ApiFromModulesAllowEmptyNodes<AllModules extends Record<string, object>> =
  ExpandModulesAndDirs<
    UnionToIntersection<
      {
        [ModulePath in keyof AllModules]: ApiForModule<
          ModulePath & string,
          AllModules[ModulePath]
        >;
      }[keyof AllModules]
    >
  >;

type FilterKeysInApi<key, API, Predicate> = API extends Predicate
  ? key
  : API extends FunctionReference<any, any, any, any>
    ? never
    : FilterApi<API, Predicate> extends Record<string, never>
      ? never
      : key;

/**
 * @public
 *
 * Filter a Convex deployment api object for functions which meet criteria,
 * for example all public queries.
 */
export type FilterApi<API, Predicate> = Expand<{
  [mod in keyof API as FilterKeysInApi<
    mod,
    API[mod],
    Predicate
  >]: API[mod] extends Predicate ? API[mod] : FilterApi<API[mod], Predicate>;
}>;

/**
 * Given an api of type API and a FunctionReference subtype, return an api object
 * containing only the function references that match.
 *
 * ```ts
 * const q = filterApi<typeof api, FunctionReference<"query">>(api)
 * ```
 *
 * @public
 */
export function filterApi<API, Predicate>(api: API): FilterApi<API, Predicate> {
  return api as any;
}

// These just* API filter helpers require no type parameters so are useable from JavaScript.
/** @public */
export function justInternal<API>(
  api: API,
): FilterApi<API, FunctionReference<any, "internal", any, any>> {
  return api as any;
}

/** @public */
export function justPublic<API>(
  api: API,
): FilterApi<API, FunctionReference<any, "public", any, any>> {
  return api as any;
}

/** @public */
export function justQueries<API>(
  api: API,
): FilterApi<API, FunctionReference<"query", any, any, any>> {
  return api as any;
}

/** @public */
export function justMutations<API>(
  api: API,
): FilterApi<API, FunctionReference<"mutation", any, any, any>> {
  return api as any;
}

/** @public */
export function justActions<API>(
  api: API,
): FilterApi<API, FunctionReference<"action", any, any, any>> {
  return api as any;
}

/** @public */
export function justPaginatedQueries<API>(
  api: API,
): FilterApi<
  API,
  FunctionReference<
    "query",
    any,
    { paginationOpts: PaginationOptions },
    PaginationResult<any>
  >
> {
  return api as any;
}

/** @public */
export function justSchedulable<API>(
  api: API,
): FilterApi<API, FunctionReference<"mutation" | "action", any, any, any>> {
  return api as any;
}

/**
 * Like {@link Expand}, this simplifies how TypeScript displays object types.
 * The differences are:
 * 1. This version is recursive.
 * 2. This stops recursing when it hits a {@link FunctionReference}.
 */
type ExpandModulesAndDirs<ObjectType> = ObjectType extends AnyFunctionReference
  ? ObjectType
  : {
      [Key in keyof ObjectType]: ExpandModulesAndDirs<ObjectType[Key]>;
    };

/**
 * A {@link FunctionReference} of any type and any visibility with any
 * arguments and any return type.
 *
 * @public
 */
export type AnyFunctionReference = FunctionReference<any, any>;

type AnyModuleDirOrFunc = {
  [key: string]: AnyModuleDirOrFunc;
} & AnyFunctionReference;

/**
 * The type that Convex api objects extend. If you were writing an api from
 * scratch it should extend this type.
 *
 * @public
 */
export type AnyApi = Record<string, Record<string, AnyModuleDirOrFunc>>;

/**
 * Recursive partial API, useful for defining a subset of an API when mocking
 * or building custom api objects.
 *
 * @public
 */
export type PartialApi<API> = {
  [mod in keyof API]?: API[mod] extends FunctionReference<any, any, any, any>
    ? API[mod]
    : PartialApi<API[mod]>;
};

/**
 * A utility for constructing {@link FunctionReference}s in projects that
 * are not using code generation.
 *
 * You can create a reference to a function like:
 * ```js
 * const reference = anyApi.myModule.myFunction;
 * ```
 *
 * This supports accessing any path regardless of what directories and modules
 * are in your project. All function references are typed as
 * {@link AnyFunctionReference}.
 *
 *
 * If you're using code generation, use `api` from `convex/_generated/api`
 * instead. It will be more type-safe and produce better auto-complete
 * in your editor.
 *
 * @public
 */
export const anyApi: AnyApi = createApi() as any;

/**
 * Given a {@link FunctionReference} or {@link FunctionReference_future}, get
 * the arguments of the function.
 *
 * This is represented as an object mapping argument names to values.
 *
 * @public
 */
export type FunctionArgs<
  FuncRef extends
    | FunctionReference<any, any>
    | FunctionReference_future<any, any>,
> = ExtractSignature<FuncRef>["args"];

// The arguments and return type a reference declares. A `FunctionReference`
// declares them in `_args` and `_returnType`, a `FunctionReference_future`
// only in its `_fn` slot. Reading the named slots first also covers a
// reference typed by an older copy of this package, which has no `_fn`.
type ExtractSignature<FuncRef> = FuncRef extends {
  _args: infer Args;
  _returnType: infer R;
}
  ? { args: Args; returnType: R }
  : FuncRef extends {
        _fn?: ((args: infer Args, keys: never) => infer R) | undefined;
      }
    ? { args: Args; returnType: R }
    : { args: any; returnType: any };

/**
 * A tuple type of the (maybe optional) arguments to `FuncRef`.
 *
 * This type is used to make methods involving arguments type safe while allowing
 * skipping the arguments for functions that don't require arguments.
 *
 * @public
 */
export type OptionalRestArgs<
  FuncRef extends
    | FunctionReference<any, any>
    | FunctionReference_future<any, any>,
> =
  FunctionArgs<FuncRef> extends EmptyObject
    ? [args?: EmptyObject]
    : [args: FunctionArgs<FuncRef>];

/**
 * A tuple type of the (maybe optional) arguments to `FuncRef`, followed by an options
 * object of type `Options`.
 *
 * This type is used to make methods like `useQuery` type-safe while allowing
 * 1. Skipping arguments for functions that don't require arguments.
 * 2. Skipping the options object.
 * @public
 */
export type ArgsAndOptions<
  FuncRef extends
    | FunctionReference<any, any>
    | FunctionReference_future<any, any>,
  Options,
> =
  FunctionArgs<FuncRef> extends EmptyObject
    ? [args?: EmptyObject, options?: Options]
    : [args: FunctionArgs<FuncRef>, options?: Options];

/**
 * Given a {@link FunctionReference} or {@link FunctionReference_future}, get
 * the return type of the function.
 *
 * @public
 */
export type FunctionReturnType<
  FuncRef extends
    | FunctionReference<any, any>
    | FunctionReference_future<any, any>,
> = ExtractSignature<FuncRef>["returnType"];

type UndefinedToNull<T> = T extends void ? null : T;

type NullToUndefinedOrNull<T> = T extends null ? T | undefined | void : T;

/**
 * Convert the return type of a function to it's client-facing format.
 *
 * This means:
 * - Converting `undefined` and `void` to `null`
 * - Removing all `Promise` wrappers
 */
export type ConvertReturnType<T> = UndefinedToNull<Awaited<T>>;

export type ValidatorTypeToReturnType<T> =
  | Promise<NullToUndefinedOrNull<T>>
  | NullToUndefinedOrNull<T>;
