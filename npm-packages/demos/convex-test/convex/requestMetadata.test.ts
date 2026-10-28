import { convexTest } from "convex-test";
import { expect, test } from "vitest";
import { api } from "./_generated/api";
import schema from "./schema";

test("request metadata", async () => {
  const t = convexTest(schema, modules);

  const fromBrowser = t.withRequestMetadata({
    ip: "203.0.113.7",
    userAgent: "Mozilla/5.0",
  });
  const clientInfo = await fromBrowser.mutation(api.requestMetadata.clientInfo);
  expect(clientInfo).toEqual({ ip: "203.0.113.7", userAgent: "Mozilla/5.0" });
});

const modules = import.meta.glob("./**/*.ts");
