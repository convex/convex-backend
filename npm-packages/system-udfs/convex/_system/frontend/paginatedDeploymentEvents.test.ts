import { describe, expect, test } from "vitest";
import { minDateForAuditLogRetention } from "./paginatedDeploymentEvents";

describe("minDateForAuditLogRetention", () => {
  test("does not move over the course of a day", () => {
    const startOfDay = Date.UTC(2026, 1, 11, 0, 0, 0, 0);
    const endOfDay = Date.UTC(2026, 1, 11, 23, 59, 59, 999);
    expect(minDateForAuditLogRetention(30, startOfDay)).toEqual(
      minDateForAuditLogRetention(30, endOfDay),
    );
  });

  test("does not reach back further than the retention window", () => {
    const now = Date.UTC(2026, 1, 11, 12, 0, 0, 0);
    expect(now - minDateForAuditLogRetention(30, now)).toBeLessThanOrEqual(
      31 * 24 * 60 * 60 * 1000,
    );
  });
});
