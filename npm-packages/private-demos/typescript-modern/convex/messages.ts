import { mutation } from "./_generated/server.js";
import { query } from "./_generated/server.js";
import { Doc } from "./_generated/dataModel.js";
import schema, { messageValidator } from "./schema.js";
import { v } from "convex/values";

export const list = query({
  args: {},
  handler: async (ctx): Promise<Doc<"messages">[]> => {
    return await ctx.db.query("messages").collect();
  },
});

export const send = mutation({
  args: messageValidator,
  handler: async (ctx, { body, author }) => {
    await ctx.db.insert("messages", { body, author });
  },
});

export const update = mutation({
  args: messageValidator
    .omit("author")
    .partial()
    .extend({
      id: schema.id("messages"),
    }),
  handler: async (ctx, args) => {
    await ctx.db.patch("messages", args.id, args);
  },
});

export const get = query({
  args: { id: schema.id("messages") },
  returns: v.nullable(schema.doc("messages")),
  handler: async (ctx, { id }) => {
    return await ctx.db.get("messages", id);
  },
});

export const typeTest = query({
  args: {},
  handler: async (ctx) => {
    const stuff = await ctx.db.query("typeTestMessages").collect();

    // (noUncheckedIndexedAccess)
    const doc = stuff[0]!;

    // exactOptionalPropertyTypes isn't any different when you access this
    const optionalField: undefined | string = doc.optionalString;
    console.log(optionalField);

    const {
      _id,
      _creationTime,
      body: _body,
      author: _author,
      objectWithOptionalString,
      ...justOptional
    } = doc;

    if ("optionalString" in justOptional) {
      // @ts-expect-error since exactOptionalPropertyTypes is not set, this should fail
      const exists: string = justOptional.optionalString;
      console.log(exists);
    } else {
      // @ts-expect-error since exactOptionalPropertyTypes is not set, this should fail
      const dne: undefined = justOptional.optionalString;
      // @ts-expect-error undefined is not assignable to string
      const exists: string = justOptional.optionalString;
      console.log(dne, exists);
    }

    if ("optionalString" in objectWithOptionalString) {
      // @ts-expect-error since exactOptionalPropertyTypes is not set, this should fail
      const exists: string = justOptional.optionalString;
      console.log(exists);
    } else {
      // @ts-expect-error since exactOptionalPropertyTypes is not set, this should fail
      const dne: undefined = justOptional.optionalString;
      // @ts-expect-error since exactOptionalPropertyTypes is not set, this should fail
      const exists: string = justOptional.optionalString;
      console.log(dne, exists);
    }
  },
});
