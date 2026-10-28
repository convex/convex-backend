/**
 * Calculate the size of a Convex value in bytes.
 *
 * This matches the Rust implementation in crates/value/src/ which is used
 * to compute bandwidth and document size limits.
 *
 * Size formula by type:
 * - Undefined: 0 bytes (not a valid Convex value, but returns 0 for convenience)
 * - Null: 1 byte (type marker)
 * - Boolean: 1 byte (type marker, value stored in marker)
 * - Int64 (bigint): 9 bytes (1 type marker + 8 bytes for 64-bit value)
 * - Float64 (number): 9 bytes (1 type marker + 8 bytes for 64-bit value)
 * - String: 2 + string.length bytes (1 type marker + UTF-8 bytes + 1 null terminator)
 * - Bytes (ArrayBuffer): 2 + bytes.length bytes (1 type marker + bytes + 1 terminator)
 * - Array: 2 + sum(element sizes) bytes (1 type marker + elements + 1 terminator)
 * - Object: 2 + sum(field_name.length + 1 + value.size) bytes
 *           (1 type marker + (field_name + null terminator + value)* + 1 terminator)
 *
 * For documents with system fields (_id and _creationTime), the size includes:
 * - _id field: 4 bytes (field name + null) + string size (2 + 31+ chars)
 * - _creationTime field: 14 bytes (field name + null) + 9 bytes (Float64)
 *
 * @module
 */

import { CommitTsPlaceholder, type Value } from "./value.js";
import { isSimpleObject } from "../common/index.js";

/**
 * Calculate the size in bytes of a Convex value.
 *
 * This matches how Convex calculates document size for bandwidth tracking
 * and size limit enforcement.
 *
 * @param value - A Convex value to measure
 * @returns The size in bytes
 *
 * @public
 */
export function getConvexSize(value: Value | undefined): number {
  if (value === undefined) {
    return 0;
  }
  if (value === null) {
    return 1;
  }
  if (typeof value === "boolean") {
    return 1;
  }
  if (typeof value === "bigint") {
    // Int64: 1 byte type marker + 8 bytes value
    return 9;
  }
  if (value instanceof CommitTsPlaceholder) {
    // Same as bigint
    return 9;
  }
  if (typeof value === "number") {
    // Float64: 1 byte type marker + 8 bytes value
    return 9;
  }
  if (typeof value === "string") {
    // String: 1 byte type marker + UTF-8 bytes + 1 byte null terminator
    // Use TextEncoder to get the actual UTF-8 byte length
    return 2 + getUtf8ByteLength(value);
  }
  if (value instanceof ArrayBuffer) {
    // Bytes: 1 byte type marker + byte array length + 1 byte terminator
    return 2 + value.byteLength;
  }
  if (Array.isArray(value)) {
    // Array: 1 byte type marker + sum of element sizes + 1 byte terminator
    let size = 2; // marker + terminator
    for (const element of value) {
      size += getConvexSize(element);
    }
    return size;
  }
  if (isSimpleObject(value)) {
    // Object: 1 byte type marker + sum(field_name + null + value) + 1 byte terminator
    let size = 2; // marker + terminator
    for (const [key, val] of Object.entries(value)) {
      if (val !== undefined) {
        // field name length + null terminator + value size
        size += getUtf8ByteLength(key) + 1 + getConvexSize(val);
      }
    }
    return size;
  }

  throw new Error(`Unsupported value type: ${typeof value}`);
}

/**
 * Threshold above which we use TextEncoder instead of manual counting.
 * For short strings, the JS loop is ~15x faster due to avoiding allocation.
 * For long strings (500+ chars), TextEncoder's native implementation wins.
 */
const UTF8_LENGTH_THRESHOLD = 500;

/**
 * Get the UTF-8 byte length of a string.
 *
 * For short strings, counts bytes directly from UTF-16 code units to avoid allocation.
 * For long strings, uses native TextEncoder which is faster despite allocation.
 *
 * @internal
 */
function getUtf8ByteLength(str: string): number {
  // For long strings, native TextEncoder is faster despite allocation
  if (str.length > UTF8_LENGTH_THRESHOLD) {
    return new TextEncoder().encode(str).length;
  }

  // For short strings, avoid allocation overhead with manual counting
  let bytes = 0;
  for (let i = 0; i < str.length; i++) {
    const code = str.charCodeAt(i);
    if (code < 0x80) {
      // ASCII: 1 byte
      bytes += 1;
    } else if (code < 0x800) {
      // 2-byte UTF-8
      bytes += 2;
    } else if (code >= 0xd800 && code <= 0xdbff) {
      // High surrogate - part of a surrogate pair encoding a code point >= U+10000
      // These are encoded as 4 bytes in UTF-8
      bytes += 4;
      i++; // Skip the low surrogate
    } else {
      // 3-byte UTF-8 (includes low surrogates if encountered alone, which is invalid but handled)
      bytes += 3;
    }
  }
  return bytes;
}

// Note: The exact _id size varies based on the table number:
// - Tables 1-127: 31 char ID
// - Tables 128-16383: 32 char ID
// We use 32 chars as a typical value (tables 128-16383).
export const SYSTEM_FIELD_ID_ESTIMATE = 38;
// _creationTime: field name (14) + Float64 (9)
export const SYSTEM_FIELD_CREATION_TIME_SIZE = 23;

/**
 * Calculate the size of a document including system fields.
 *
 * If your value already has _id and _creationTime fields, this will count them
 * in the normal size calculation. Otherwise, it adds the constant overhead
 * for system fields.
 *
 * @param value - A Convex object (document body)
 * @param options - Options for size calculation
 * @returns The size in bytes
 *
 * @public
 */
export function getDocumentSize(
  value: Record<string, Value>,
  options?: {
    /**
     * @internal
     * Length of the _id field if it is missing. Defaults to standard length.
     */
    customIdLength?: number;
  },
): number {
  const baseSize = getConvexSize(value);

  // Check if system fields are already present
  const hasId = "_id" in value && value["_id"] !== undefined;
  const hasCreationTime =
    "_creationTime" in value && value["_creationTime"] !== undefined;

  if (hasId && hasCreationTime) {
    return baseSize;
  }

  // Add size for missing system fields
  let additionalSize = 0;

  if (!hasId) {
    if (options?.customIdLength) {
      // 4 bytes for _id field name + 2 bytes for string overhead.
      additionalSize += options.customIdLength + 6;
    } else {
      additionalSize += SYSTEM_FIELD_ID_ESTIMATE;
    }
  }

  if (!hasCreationTime) {
    additionalSize += SYSTEM_FIELD_CREATION_TIME_SIZE;
  }

  // The base size includes 2 bytes for object markers, but we need to account
  // for adding new fields to an existing object structure
  return baseSize + additionalSize;
}
