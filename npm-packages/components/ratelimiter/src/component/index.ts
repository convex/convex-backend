import { ConvexError, type Infer, v } from "convex/values";
import { mutation, query, type DatabaseReader } from "./_generated/server.js";

const rateLimitArgs = {
  name: v.string(),
  key: v.optional(v.string()),
  count: v.optional(v.number()),
  reserve: v.optional(v.boolean()),
  throws: v.optional(v.boolean()),
};
const _rateLimitArgsObject = v.object(rateLimitArgs);

const config = {
  kind: "token bucket",
  rate: 3,
  period: 1000 * 60,
  capacity: undefined,
  maxReserved: undefined,
  start: undefined,
};

export const rateLimit = mutation({
  args: rateLimitArgs,
  returns: v.object({
    ok: v.boolean(),
    retryAt: v.optional(v.number()),
  }),
  handler: async (ctx, args) => {
    const status = await checkRateLimitInternal(ctx.db, args);
    const { ok, retryAt } = status;
    if (ok) {
      const { ts, value } = status;
      const existing = await getExisting(ctx.db, args.name, args.key);
      if (existing) {
        await ctx.db.patch("rateLimits", existing._id, { ts, value });
      } else {
        const { name, key } = args;
        await ctx.db.insert("rateLimits", {
          name,
          key,
          ts: ts!,
          value: value!,
        });
      }
    }
    return { ok, retryAt };
  },
});

export const checkRateLimit = query({
  args: rateLimitArgs,
  returns: v.object({
    ok: v.boolean(),
    retryAt: v.optional(v.number()),
    ts: v.optional(v.number()),
    value: v.optional(v.number()),
  }),
  handler: async (ctx, args) => {
    // FIXME: a rate-limit check in a query is a footgun: it reads the clock, so
    // its cached result can be stale. The published rate-limiter component has
    // the same issue in `lib.checkRateLimit`; remove or document it there too.
    // eslint-disable-next-line @convex-dev/no-time-or-random-in-queries -- See FIXME.
    return await checkRateLimitInternal(ctx.db, args);
  },
});

export const resetRateLimit = mutation({
  args: {
    name: v.string(),
    key: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    const existing = await getExisting(ctx.db, args.name, args.key);
    if (existing) {
      await ctx.db.delete("rateLimits", existing._id);
    }
  },
});

async function checkRateLimitInternal(
  db: DatabaseReader,
  args: Infer<typeof _rateLimitArgsObject>,
) {
  const now = Date.now();
  const existing = await getExisting(db, args.name, args.key);
  const max = config.capacity ?? config.rate;
  const consuming = args.count ?? 1;
  if (args.reserve) {
    if (config.maxReserved && consuming > max + config.maxReserved) {
      throw new Error(
        `Rate limit ${args.name} count ${consuming} exceeds ${max + config.maxReserved}.`,
      );
    }
  } else if (consuming > max) {
    throw new Error(
      `Rate limit ${args.name} count ${consuming} exceeds ${max}.`,
    );
  }
  const state = existing ?? {
    value: max,
    ts:
      config.kind === "fixed window"
        ? (config.start ?? Math.floor(Math.random() * config.period))
        : now,
  };
  let ts,
    value,
    retryAt: number | undefined = undefined;
  if (config.kind === "token bucket") {
    const elapsed = now - state.ts;
    const rate = config.rate / config.period;
    value = Math.min(state.value + elapsed * rate, max) - consuming;
    ts = now;
    if (value < 0) {
      retryAt = now + -value / rate;
    }
  } else {
    const elapsedWindows = Math.floor((Date.now() - state.ts) / config.period);
    value =
      Math.min(state.value + config.rate * elapsedWindows, max) - consuming;
    ts = state.ts + elapsedWindows * config.period;
    if (value < 0) {
      const windowsNeeded = Math.ceil(-value / config.rate);
      retryAt = ts + config.period * windowsNeeded;
    }
  }
  if (value < 0) {
    if (!args.reserve || (config.maxReserved && -value > config.maxReserved)) {
      if (args.throws) {
        throw new ConvexError({
          kind: "RateLimited",
          name: args.name,
          retryAt,
        });
      }
      return { ok: false, retryAt } as const;
    }
  }
  return { ok: true, retryAt, ts, value } as const;
}

async function getExisting(
  db: DatabaseReader,
  name: string,
  key: string | undefined,
) {
  return await db
    .query("rateLimits")
    .withIndex("name", (q: any) => q.eq("name", name).eq("key", key))
    .unique();
}
