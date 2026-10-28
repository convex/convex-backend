type ParsedExpirationSuccess =
  | { kind: "none" }
  | { kind: "absolute"; timestampMs: number }
  | { kind: "relative"; amount: number; unit: "minute" | "hour" | "day" };

type ParsedExpiration =
  | ParsedExpirationSuccess
  | { kind: "error"; message: string };

const PARSE_ERROR_MESSAGE =
  `Supported formats:\n` +
  `  "none"                          — no expiration\n` +
  `  "in 7 days"                     — relative (minutes, hours, days)\n` +
  `  "2026-04-01T00:00:00Z"          — UTC datetime\n` +
  `  "1711828382"                    — Unix timestamp (seconds)\n` +
  `  "1711828382000"                 — Unix timestamp (milliseconds)`;

const UNIT_MS = {
  minute: 60 * 1000,
  hour: 60 * 60 * 1000,
  day: 24 * 60 * 60 * 1000,
} as const;

/**
 * Parse an expiration input string into a structured representation.
 * Does not depend on the current time.
 */
export function parseExpiration(input: string): ParsedExpiration {
  const trimmed = input.trim();

  if (trimmed.toLowerCase() === "none") {
    return { kind: "none" };
  }

  // All digits → Unix timestamp
  if (/^\d+$/.test(trimmed)) {
    const n = Number(trimmed);

    return {
      kind: "absolute",
      timestampMs:
        n < 1e12 // 1e12 milliseconds is a date in 2001 → unambiguous
          ? n * 1000 // seconds → convert to ms
          : n, // already milliseconds
    };
  }

  // UTC datetime: "2026-04-01T00:00:00Z"
  if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(trimmed)) {
    const date = new Date(trimmed);
    if (isNaN(date.getTime())) {
      return {
        kind: "error",
        message: `Invalid UTC datetime: "${trimmed}". ${PARSE_ERROR_MESSAGE}`,
      };
    }
    return { kind: "absolute", timestampMs: date.getTime() };
  }

  // Relative: "in 3 hours", "in 1 day", "in 45 minutes"
  const relativeMatch = trimmed.match(/^in\s+(\d+)\s+(minute|hour|day)s?$/i);
  if (relativeMatch) {
    const amount = Number(relativeMatch[1]);
    const unit = relativeMatch[2].toLowerCase() as "minute" | "hour" | "day";
    return { kind: "relative", amount, unit };
  }

  return {
    kind: "error",
    message: `Invalid expiration format: "${trimmed}". ${PARSE_ERROR_MESSAGE}`,
  };
}

/**
 * Resolve a parsed expiration into a timestamp in milliseconds, or null for "none".
 */
export function resolveExpiration(
  parsed: ParsedExpirationSuccess,
  now?: number,
): number | null {
  switch (parsed.kind) {
    case "none":
      return null;
    case "absolute":
      return parsed.timestampMs;
    case "relative": {
      const base = now ?? Date.now();
      return base + parsed.amount * UNIT_MS[parsed.unit];
    }
  }
}

type ValidationResult =
  | { kind: "success" }
  | { kind: "error"; message: string };

/**
 * Validate that a resolved expiration timestamp is acceptable.
 */
export function validateExpiration(
  timestampMs: number,
  now?: number,
): ValidationResult {
  const base = now ?? Date.now();
  const thirtyMinutes = 30 * 60 * 1000;
  const oneYear = 365 * 24 * 60 * 60 * 1000;

  if (timestampMs <= base) {
    return { kind: "error", message: "Expiration must be in the future." };
  }
  if (timestampMs - base < thirtyMinutes) {
    return {
      kind: "error",
      message: "Expiration must be at least 30 minutes from now.",
    };
  }
  if (timestampMs - base > oneYear) {
    return {
      kind: "error",
      message: "Expiration must be at most 1 year from now.",
    };
  }
  return { kind: "success" };
}
