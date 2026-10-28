import { v } from "convex/values";
import { mutation, query } from "./_generated/server";

export const get = query({
  args: {
    counterName: v.string(),
  },
  handler: async ({ db }, { counterName }): Promise<number> => {
    const counterDoc = await db
      .query("counter_table")
      // eslint-disable-next-line @convex-dev/no-filter-in-query -- demo uses `.filter()` for simplicity
      .filter((q) => q.eq(q.field("name"), counterName))
      .first();
    console.log("Got stuff");
    if (counterDoc === null) {
      return 0;
    }
    return counterDoc.counter;
  },
});

export const increment = mutation({
  args: {
    counterName: v.string(),
    increment: v.number(),
  },
  handler: async ({ db, auth }, { counterName, increment }) => {
    const identity = await auth.getUserIdentity();
    if (!identity) {
      throw new Error("Unauthenticated call to incrementCounter");
    }
    const counterDoc = await db
      .query("counter_table")
      // eslint-disable-next-line @convex-dev/no-filter-in-query -- demo uses `.filter()` for simplicity
      .filter((q) => q.eq(q.field("name"), counterName))
      .first();
    if (counterDoc === null) {
      await db.insert("counter_table", {
        name: counterName,
        counter: increment,
      });
      // console.log messages appear in your browser's console and the Convex dashboard.
      console.log("Created counter.");
    } else {
      counterDoc.counter += increment;
      await db.replace(counterDoc._id, counterDoc);
      console.log(`Value of counter is now ${counterDoc.counter}.`);
    }
  },
});
