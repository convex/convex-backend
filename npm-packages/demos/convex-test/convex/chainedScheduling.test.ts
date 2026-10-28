import { convexTest } from "convex-test";
import { expect, test, vi } from "vitest";
import { api } from "./_generated/api";
import schema from "./schema";

test("mutation scheduling action scheduling action", async () => {
  // Enable fake timers
  vi.useFakeTimers();

  const t = convexTest(schema, modules);

  // Call a function that schedules a mutation or action
  await t.mutation(api.scheduler.mutationSchedulingActionSchedulingAction);

  // Wait for all scheduled functions, repeatedly
  // advancing time and waiting for currently in-progress
  // functions to finish
  await t.finishAllScheduledFunctions(vi.runAllTimers);

  // Assert the resulting state after all scheduled functions finished
  const createdTask = await t.run(async (ctx) => {
    return await ctx.db.query("tasks").first();
  });
  expect(createdTask).toMatchObject({ author: "AI" });

  // Reset to normal `setTimeout` etc. implementation
  vi.useRealTimers();
});

const modules = import.meta.glob("./**/*.ts");
