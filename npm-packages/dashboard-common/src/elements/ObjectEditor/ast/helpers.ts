import { ValidatorJSON, Value } from "convex/values";
import isPlainObject from "lodash/isPlainObject";
import { UNDEFINED_PLACEHOLDER } from "system-udfs/convex/_system/frontend/lib/values";
import * as IdEncoding from "id-encoding";

export const isValidValue = (
  validator: ValidatorJSON,
  value: Value,
  shallow = true,
): boolean => {
  switch (validator.type) {
    case "null":
      return value === null;
    case "string":
      return typeof value === "string";
    case "boolean":
      return typeof value === "boolean";
    case "number":
      return typeof value === "number";
    case "bigint":
    case "commitTs":
      return typeof value === "bigint";
    case "bytes":
      return value instanceof Uint8Array;
    case "any":
      return true;
    case "literal":
      return value === validator.value && value !== UNDEFINED_PLACEHOLDER;
    case "id":
      // TODO: Validate table names
      return typeof value === "string" && IdEncoding.isId(value);
    case "object": {
      const isObject =
        typeof value === "object" && value !== null && isPlainObject(value);
      if (shallow || !isObject) {
        return isObject;
      }
      for (const [key, valueValidator] of Object.entries(validator.value)) {
        const nextValue = (value as Record<string, Value>)[key];
        if (valueValidator.optional && nextValue === undefined) {
          continue;
        }

        if (!isValidValue(valueValidator.fieldType, nextValue, shallow)) {
          return false;
        }
      }
      return true;
    }
    case "record": {
      const isObject =
        typeof value === "object" && value !== null && isPlainObject(value);
      if (shallow || !isObject) {
        return isObject;
      }
      for (const [key, keyValue] of Object.entries(value)) {
        if (
          !isValidValue(validator.keys, key, shallow) ||
          !isValidValue(validator.values.fieldType, keyValue, shallow)
        ) {
          return false;
        }
      }
      return true;
    }
    case "array": {
      const isArray = Array.isArray(value);
      if (shallow || !isArray) {
        return isArray;
      }

      for (const v of value) {
        if (!isValidValue(validator.value, v, shallow)) {
          return false;
        }
      }

      return true;
    }
    case "union":
      return validator.value.some((v) =>
        // This is still shallow because we need to check every type in the union.
        isValidValue(v, value, shallow),
      );
    default: {
      validator satisfies never;
      return false;
    }
  }
};

export const typeForValue = (value: Value): ValidatorJSON["type"] => {
  if (value === null) {
    return "null";
  }
  if (typeof value === "string") {
    return "string";
  }
  if (typeof value === "boolean") {
    return "boolean";
  }
  if (typeof value === "number") {
    return "number";
  }
  if (typeof value === "bigint") {
    return "bigint";
  }
  if (value instanceof Uint8Array) {
    return "bytes";
  }
  if (isPlainObject(value)) {
    return "object";
  }
  if (Array.isArray(value)) {
    return "array";
  }
  return "any";
};
