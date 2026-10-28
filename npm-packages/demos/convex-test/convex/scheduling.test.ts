import { convexTest } from "convex-test";
import { expect, test, vi } from "vitest";
import { api } from "./_generated/api";
import schema from "./schema";

test("mutation scheduling action", async () => {
  // Enable fake timers
  vi.useFakeTimers();

  const t = convexTest(schema, modules);

  // Call a function that schedules a mutation or action
  const scheduledFunctionId = await t.mutation(
    api.scheduler.mutationSchedulingAction,
    { delayMs: 10000 },
  );

  // Advance the mocked time
  vi.advanceTimersByTime(5000);

  // Advance the mocked time past the scheduled time of the function
  vi.advanceTimersByTime(6000);

  // Or run all currently pending timers
  vi.runAllTimers();

  // At this point the scheduled function will be `inProgress`,
  // now wait for it to finish
  await t.finishInProgressScheduledFunctions();

  // Assert that the scheduled function succeeded or failed
  const scheduledFunctionStatus = await t.run(async (ctx) => {
    return await ctx.db.system.get("_scheduled_functions", scheduledFunctionId);
  });
  expect(scheduledFunctionStatus).toMatchObject({ state: { kind: "success" } });

  // Reset to normal `setTimeout` etc. implementation
  vi.useRealTimers();
});

const modules = import.meta.glob("./**/*.ts");
