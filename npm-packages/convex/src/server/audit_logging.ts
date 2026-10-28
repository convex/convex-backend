import { version } from "../index.js";
import { performAsyncSyscall } from "./impl/syscall.js";
import { LogVar, varNames } from "./logVars.js";

export type AuditLogBody = { [key: string]: AuditLogValue };
export type AuditLogValue =
  | null
  | undefined
  | boolean
  | number
  | string
  | LogVar
  | AuditLogValue[]
  | { [key: string]: AuditLogValue };

type JsonValue =
  | null
  | undefined
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

function validateKey(key: string) {
  if (key.startsWith("$")) {
    throw new Error(`Audit log body keys must not start with "$": "${key}"`);
  }
}

function cloneValue(value: AuditLogValue): JsonValue {
  if (typeof value === "symbol") {
    if (!(value in varNames)) {
      throw new Error(`Unknown audit var symbol: ${String(value)}.`);
    }
    return { $var: varNames[value] };
  }
  if (value === null || value === undefined || typeof value !== "object") {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map(cloneValue);
  }
  const result: { [key: string]: JsonValue } = {};
  for (const [key, val] of Object.entries(value)) {
    validateKey(key);
    result[key] = cloneValue(val);
  }
  return result;
}

/**
 * Deep-clone the body, replacing audit var symbols with sentinel objects
 * like `{ $var: "ip" }`.
 */
export function cloneWithSentinels(body: AuditLogBody): {
  [key: string]: JsonValue;
} {
  const result: { [key: string]: JsonValue } = {};
  for (const [key, val] of Object.entries(body)) {
    validateKey(key);
    result[key] = cloneValue(val);
  }
  return result;
}

export const audit = async (body: AuditLogBody): Promise<void> => {
  await performAsyncSyscall("1.0/auditLog", {
    body: cloneWithSentinels(body),
    version,
  });
};
