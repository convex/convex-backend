import { v, VString } from "convex/values";
import {
  action,
  internalAction,
  internalMutation,
  internalQuery,
  mutation,
  query,
} from "./_generated/server.js";
import { primitiveTypes, primitiveTypesByName } from "./schema.js";

type Branded = string & { __brand: "branded" };
const vBranded = v.string() as VString<Branded>;

export const q = query({
  args: {
    branded: vBranded,
  },
  returns: vBranded,
  handler: async (_ctx, args) => {
    return args.branded;
  },
});

export const iq = internalQuery({
  args: primitiveTypesByName,
  returns: v.union(...primitiveTypes),
  handler: async (_ctx, args) => {
    const key = "str" as keyof typeof primitiveTypesByName;
    return args[key];
  },
});

export const m = mutation({
  args: {
    branded: vBranded,
  },
  returns: v.union(
    v.object({
      _id: v.id("empty"),
      _creationTime: v.number(),
    }),
    v.null(),
  ),
  handler: async (ctx, args) => {
    const normalizedId = ctx.db.normalizeId("empty", args.branded);
    if (normalizedId === null) {
      return await ctx.db.get("empty", await ctx.db.insert("empty", {}));
    }
    return await ctx.db.get("empty", normalizedId);
  },
});

export const im = internalMutation({
  args: {
    branded: vBranded,
  },
  returns: v.null(),
  handler: async () => {},
});

export const a = action({
  args: {
    id: v.id("empty"),
    branded: vBranded,
  },
  returns: vBranded,
  handler: async () => {
    return "" as Branded;
  },
});

export const ia = internalAction({
  args: {},
  handler: async () => {},
});
