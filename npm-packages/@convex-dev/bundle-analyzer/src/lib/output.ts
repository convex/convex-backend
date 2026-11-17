import { InvalidArgumentError } from "@commander-js/extra-typings";

/** A failure the CLI reports as a message and exit code 1, without a stack. */
export class AnalyzerError extends Error {}

/** Writes a report or JSON to stdout. Progress and warnings go to stderr. */
export function logOutput(text: string) {
  process.stdout.write(text + "\n");
}

export function logWarning(text: string) {
  process.stderr.write(text + "\n");
}

export function parsePositiveInteger(value: string) {
  const parsedValue = +value;
  if (!Number.isSafeInteger(parsedValue) || parsedValue <= 0) {
    throw new InvalidArgumentError("Not a positive integer.");
  }
  return parsedValue;
}

export function printJson(value: unknown) {
  logOutput(JSON.stringify(value, jsonReplacer, 2));
}

function jsonReplacer(_key: string, value: unknown) {
  if (value instanceof Map) return Object.fromEntries(value);
  if (value instanceof Set) return [...value];
  return value;
}
