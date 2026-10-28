import { Expand } from "../type_utils.js";
import { GenericId } from "./index.js";
import {
  OptionalProperty,
  VAny,
  VArray,
  VBoolean,
  VBytes,
  VFloat64,
  VId,
  VCommitTs,
  VInt64,
  VLiteral,
  VNull,
  VObject,
  VOptional,
  VRecord,
  VString,
  VUnion,
  Validator,
} from "./validators.js";

/**
 * The type that all validators must extend.
 *
 * @public
 */
export type GenericValidator = Validator<any, any, any>;

export function isValidator(v: any): v is GenericValidator {
  // Check for the literal `true` marker: a field map can contain a field
  // *named* `isConvexValidator` whose value is a (truthy) validator object.
  return v?.isConvexValidator === true;
}

/**
 * Coerce an object with validators as properties to a validator.
 * If a validator is passed, return it.
 *
 * @public
 */
export function asObjectValidator<
  V extends Validator<any, any, any> | PropertyValidators,
>(
  obj: V,
): V extends Validator<any, any, any>
  ? V
  : V extends PropertyValidators
    ? Validator<ObjectType<V>>
    : never {
  if (isValidator(obj)) {
    return obj as any;
  } else {
    return v.object(obj as PropertyValidators) as any;
  }
}

/**
 * Coerce an object with validators as properties to a validator.
 * If a validator is passed, return it.
 *
 * @public
 */
export type AsObjectValidator<
  V extends Validator<any, any, any> | PropertyValidators,
> =
  V extends Validator<any, any, any>
    ? V
    : V extends PropertyValidators
      ? Validator<ObjectType<V>>
      : never;

/**
 * The validator builder.
 *
 * This builder allows you to build validators for Convex values. Validators
 * are used in two places:
 *
 * 1. **Schema definitions** - to define the shape of documents in your tables.
 * 2. **Function arguments and return values** - to validate inputs and outputs
 *    of your Convex queries, mutations, and actions.
 *
 * Always include `args` and `returns` validators on all Convex functions. If a
 * function doesn't return a value, use `returns: v.null()`.
 *
 * **Convex type reference:**
 *
 * | Convex Type | JS/TS Type    | Validator                      |
 * |-------------|---------------|--------------------------------|
 * | Id          | `string`      | `v.id("tableName")`            |
 * | Null        | `null`        | `v.null()`                     |
 * | Float64     | `number`      | `v.number()`                   |
 * | Int64       | `bigint`      | `v.int64()`                    |
 * | Boolean     | `boolean`     | `v.boolean()`                  |
 * | String      | `string`      | `v.string()`                   |
 * | Bytes       | `ArrayBuffer` | `v.bytes()`                    |
 * | Array       | `Array`       | `v.array(element)`             |
 * | Object      | `Object`      | `v.object({ field: value })`   |
 * | Record      | `Record`      | `v.record(keys, values)`       |
 *
 * **Modifiers and meta-types:**
 * - `v.union(member1, member2)` - a value matching at least one validator
 * - `v.literal("value")` - a specific literal string, number, bigint, or boolean
 * - `v.optional(validator)` - makes a property optional in an object (`T | undefined`)
 *
 * **Important notes:**
 * - JavaScript's `undefined` is **not** a valid Convex value. Functions that
 *   return `undefined` or have no return will return `null` to the client.
 *   Objects with `undefined` values will strip those keys during serialization.
 *   For arrays, use an explicit `null` instead.
 * - `v.bigint()` is deprecated, use `v.int64()` instead.
 * - `v.map()` and `v.set()` are not supported. Use `v.array()` of tuples or
 *   `v.record()` as alternatives.
 *
 * @example
 * ```typescript
 * import { v } from "convex/values";
 *
 * // Use in function definition:
 * export const createUser = mutation({
 *   args: {
 *     name: v.string(),
 *     email: v.string(),
 *     age: v.optional(v.number()),
 *   },
 *   returns: v.id("users"),
 *   handler: async (ctx, args) => {
 *     return await ctx.db.insert("users", args);
 *   },
 * });
 * ```
 *
 * @see https://docs.convex.dev/database/types
 * @see https://docs.convex.dev/functions/validation
 * @public
 */
export const v = {
  /**
   * Validates that the value is a document ID for the given table.
   *
   * IDs are strings at runtime but are typed as `Id<"tableName">` in
   * TypeScript for type safety.
   *
   * @example
   * ```typescript
   * args: { userId: v.id("users") }
   * ```
   *
   * @param tableName The name of the table.
   */
  id: <TableName extends string>(tableName: TableName) => {
    return new VId<GenericId<TableName>>({
      isOptional: "required",
      tableName,
    });
  },

  /**
   * Validates that the value is `null`.
   *
   * Use `returns: v.null()` for functions that don't return a meaningful value.
   * JavaScript `undefined` is not a valid Convex value, it is automatically
   * converted to `null`.
   */
  null: () => {
    return new VNull({ isOptional: "required" });
  },

  /**
   * Validates that the value is a JavaScript `number` (Convex Float64).
   *
   * Supports all IEEE-754 double-precision floating point numbers including
   * NaN and Infinity.
   *
   * Alias for `v.float64()`.
   */
  number: () => {
    return new VFloat64({ isOptional: "required" });
  },

  /**
   * Validates that the value is a JavaScript `number` (Convex Float64).
   *
   * Supports all IEEE-754 double-precision floating point numbers.
   */
  float64: () => {
    return new VFloat64({ isOptional: "required" });
  },

  /**
   * @deprecated Use `v.int64()` instead.
   */
  bigint: () => {
    return new VInt64({ isOptional: "required" });
  },

  /**
   * Validates that the value is a JavaScript `bigint` (Convex Int64).
   *
   * Supports BigInts between -2^63 and 2^63-1.
   *
   * @example
   * ```typescript
   * args: { timestamp: v.int64() }
   * // Usage: createDoc({ timestamp: 1234567890n })
   * ```
   */
  int64: () => {
    return new VInt64({ isOptional: "required" });
  },

  /**
   * Validates that the value is a commit timestamp: an int64 (`bigint`)
   * assigned from the transaction's commit timestamp.
   *
   * An index on a field with this validator orders documents by commit order.
   * Accepts any int64 value; write `db.vars.commitTs` to have the field
   * resolve to the commit timestamp when the mutation commits.
   */
  commitTs: () => {
    return new VCommitTs({ isOptional: "required" });
  },

  /**
   * Validates that the value is a `boolean`.
   */
  boolean: () => {
    return new VBoolean({ isOptional: "required" });
  },

  /**
   * Validates that the value is a `string`.
   *
   * Strings are stored as UTF-8 and their storage size is calculated as their
   * UTF-8 encoded size.
   */
  string: () => {
    return new VString({ isOptional: "required" });
  },

  /**
   * Validates that the value is an `ArrayBuffer` (Convex Bytes).
   *
   * Use for binary data.
   */
  bytes: () => {
    return new VBytes({ isOptional: "required" });
  },

  /**
   * Validates that the value is exactly equal to the given literal.
   *
   * Useful for discriminated unions and enum-like patterns.
   *
   * @example
   * ```typescript
   * // Discriminated union pattern:
   * v.union(
   *   v.object({ kind: v.literal("error"), message: v.string() }),
   *   v.object({ kind: v.literal("success"), value: v.number() }),
   * )
   * ```
   *
   * @param literal The literal value to compare against.
   */
  literal: <T extends string | number | bigint | boolean>(literal: T) => {
    return new VLiteral<T>({ isOptional: "required", value: literal });
  },

  /**
   * Validates that the value is an `Array` where every element matches the
   * given validator.
   *
   * Arrays can have at most 8192 elements.
   *
   * @example
   * ```typescript
   * args: { tags: v.array(v.string()) }
   * args: { coordinates: v.array(v.number()) }
   * args: { items: v.array(v.object({ name: v.string(), qty: v.number() })) }
   * ```
   *
   * @param element The validator for the elements of the array.
   */
  array: <T extends Validator<any, "required", any>>(element: T) => {
    return new VArray<T["type"][], T>({ isOptional: "required", element });
  },

  /**
   * Validates that the value is an `Object` with the specified properties.
   *
   * Objects can have at most 1024 entries. Field names must be non-empty and
   * must not start with `"$"` or `"_"` (`_` is reserved for system fields
   * like `_id` and `_creationTime`; `$` is reserved for Convex internal use).
   *
   * @example
   * ```typescript
   * args: {
   *   user: v.object({
   *     name: v.string(),
   *     email: v.string(),
   *     age: v.optional(v.number()),
   *   })
   * }
   * ```
   *
   * @param fields An object mapping property names to their validators.
   */
  object: <T extends PropertyValidators>(fields: T) => {
    return new VObject<ObjectType<T>, T>({ isOptional: "required", fields });
  },

  /**
   * Validates that the value is a `Record` (object with dynamic keys).
   *
   * Records are objects at runtime but allow dynamic keys, unlike `v.object()`
   * which requires known property names. Keys must be ASCII characters only,
   * non-empty, and not start with `"$"` or `"_"`.
   *
   * @example
   * ```typescript
   * // Map of user IDs to scores:
   * args: { scores: v.record(v.id("users"), v.number()) }
   *
   * // Map of string keys to string values:
   * args: { metadata: v.record(v.string(), v.string()) }
   * ```
   *
   * @param keys The validator for the keys of the record.
   * @param values The validator for the values of the record.
   */
  record: <
    Key extends Validator<string, "required", any>,
    Value extends Validator<any, "required", any>,
  >(
    keys: Key,
    values: Value,
  ) => {
    return new VRecord<Record<Infer<Key>, Value["type"]>, Key, Value>({
      isOptional: "required",
      key: keys,
      value: values,
    });
  },

  /**
   * Validates that the value matches at least one of the given validators.
   *
   * @example
   * ```typescript
   * // Allow string or number:
   * args: { value: v.union(v.string(), v.number()) }
   *
   * // Discriminated union (recommended pattern):
   * v.union(
   *   v.object({ kind: v.literal("text"), body: v.string() }),
   *   v.object({ kind: v.literal("image"), url: v.string() }),
   * )
   *
   * // Nullable value:
   * returns: v.union(v.object({ ... }), v.null())
   * ```
   *
   * @param members The validators to match against.
   */
  union: <T extends Validator<any, "required", any>[]>(...members: T) => {
    return new VUnion<T[number]["type"], T>({
      isOptional: "required",
      members,
    });
  },

  /**
   * A validator that accepts any Convex value without validation.
   *
   * Prefer using specific validators when possible for better type safety
   * and runtime validation.
   */
  any: () => {
    return new VAny({ isOptional: "required" });
  },

  /**
   * Makes a property optional in an object validator.
   *
   * An optional property can be omitted entirely when creating a document or
   * calling a function. This is different from `v.nullable()` which requires
   * the property to be present but allows `null`.
   *
   * @example
   * ```typescript
   * v.object({
   *   name: v.string(),              // required
   *   nickname: v.optional(v.string()), // can be omitted
   * })
   *
   * // Valid: { name: "Alice" }
   * // Valid: { name: "Alice", nickname: "Ali" }
   * // Invalid: { name: "Alice", nickname: null }  - use v.nullable() for this
   * ```
   *
   * @param value The property value validator to make optional.
   */
  optional: <T extends GenericValidator>(value: T) => {
    return value.asOptional() as VOptional<T>;
  },

  /**
   * Allows a value to be either the given type or `null`.
   *
   * This is shorthand for `v.union(value, v.null())`. Unlike `v.optional()`,
   * the property must still be present, but may be `null`.
   *
   * @example
   * ```typescript
   * v.object({
   *   name: v.string(),
   *   deletedAt: v.nullable(v.number()), // must be present, can be null
   * })
   *
   * // Valid: { name: "Alice", deletedAt: null }
   * // Valid: { name: "Alice", deletedAt: 1234567890 }
   * // Invalid: { name: "Alice" }  - deletedAt is required
   * ```
   */
  nullable: <T extends Validator<any, "required", any>>(value: T) => {
    return v.union(value, v.null());
  },
};

/**
 * Validators for each property of an object.
 *
 * This is represented as an object mapping the property name to its
 * {@link Validator}.
 *
 * @public
 */
export type PropertyValidators = Record<
  string,
  Validator<any, OptionalProperty, any>
>;

/**
 * Compute the type of an object from {@link PropertyValidators}.
 *
 * @public
 */
export type ObjectType<Fields extends PropertyValidators> = Expand<
  // Map each key to the corresponding property validator's type making
  // the optional ones optional.
  {
    // This `Exclude<..., undefined>` does nothing unless
    // the tsconfig.json option `"exactOptionalPropertyTypes": true,`
    // is used. When it is it results in a more accurate type.
    // When it is not the `Exclude` removes `undefined` but it is
    // added again by the optional property.
    [Property in OptionalKeys<Fields>]?: Exclude<
      Infer<Fields[Property]>,
      undefined
    >;
  } & {
    [Property in RequiredKeys<Fields>]: Infer<Fields[Property]>;
  }
>;

type OptionalKeys<PropertyValidators extends Record<string, GenericValidator>> =
  {
    [Property in keyof PropertyValidators]: PropertyValidators[Property]["isOptional"] extends "optional"
      ? Property
      : never;
  }[keyof PropertyValidators];

type RequiredKeys<PropertyValidators extends Record<string, GenericValidator>> =
  Exclude<keyof PropertyValidators, OptionalKeys<PropertyValidators>>;

/**
 * Extract a TypeScript type from a validator.
 *
 * Example usage:
 * ```ts
 * const objectSchema = v.object({
 *   property: v.string(),
 * });
 * type MyObject = Infer<typeof objectSchema>; // { property: string }
 * ```
 * @typeParam V - The type of a {@link Validator} constructed with {@link v}.
 *
 * @public
 */
export type Infer<T extends Validator<any, OptionalProperty, any>> = T["type"];
