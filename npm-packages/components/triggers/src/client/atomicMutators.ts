import {
  FunctionReference,
  createFunctionHandle,
  internalMutationGeneric,
} from "convex/server";
import { v } from "convex/values";

export function atomicMutators(table: string) {
  return {
    atomicInsert: internalMutationGeneric({
      args: { value: v.any() },
      handler: async (ctx, { value }) => {
        const id = await ctx.db.insert(table, value);
        const newDoc = await ctx.db.get(table, id);
        return { newDoc };
      },
    }),
    atomicPatch: internalMutationGeneric({
      args: { id: v.id(table), value: v.any() },
      handler: async (ctx, { id, value }) => {
        const oldDoc = await ctx.db.get(table, id);
        await ctx.db.patch(table, id, value);
        const newDoc = await ctx.db.get(table, id);
        return { oldDoc, newDoc };
      },
    }),
    atomicReplace: internalMutationGeneric({
      args: { id: v.id(table), value: v.any() },
      handler: async (ctx, { id, value }) => {
        const oldDoc = await ctx.db.get(table, id);
        await ctx.db.replace(table, id, value);
        const newDoc = await ctx.db.get(table, id);
        return { oldDoc, newDoc };
      },
    }),
    atomicDelete: internalMutationGeneric({
      args: { id: v.id(table) },
      handler: async (ctx, { id }) => {
        const oldDoc = await ctx.db.get(table, id);
        await ctx.db.delete(table, id);
        return { oldDoc };
      },
    }),
  };
}

export type AtomicMutators = {
  [k in keyof ReturnType<typeof atomicMutators>]: FunctionReference<
    "mutation",
    "internal"
  >;
};

export async function watchTable(table: string, api: AtomicMutators) {
  return {
    table,
    insert: await createFunctionHandle(api.atomicInsert),
    patch: await createFunctionHandle(api.atomicPatch),
    replace: await createFunctionHandle(api.atomicReplace),
    delete: await createFunctionHandle(api.atomicDelete),
  };
}
