import { v } from "convex/values";
import {
  query,
  action,
  env,
  internalMutation,
  internalQuery,
} from "./_generated/server";
import { internal } from "./_generated/api";
import { CUISINES, EXAMPLE_DATA } from "./constants";

export type SearchResult = {
  _id: string;
  description: string;
  cuisine: string;
  _score: number;
};

export async function embed(text: string): Promise<number[]> {
  const key = env.OPENAI_KEY;
  const req = { input: text, model: "text-embedding-ada-002" };
  const resp = await fetch("https://api.openai.com/v1/embeddings", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${key}`,
    },
    body: JSON.stringify(req),
  });
  if (!resp.ok) {
    const msg = await resp.text();
    throw new Error(`OpenAI API error: ${msg}`);
  }
  const json = await resp.json();
  const vector = json["data"][0]["embedding"];
  console.log(`Computed embedding of "${text}": ${vector.length} dimensions`);
  return vector;
}

export const populate = action({
  args: {},
  handler: async (ctx) => {
    for (const doc of EXAMPLE_DATA) {
      const embedding = await embed(doc.description);
      await ctx.runMutation(internal.vectorDemo.insertRow, {
        cuisine: doc.cuisine,
        description: doc.description,
        embedding,
      });
    }
  },
});

export const insert = action({
  args: { cuisine: v.string(), description: v.string() },
  handler: async (ctx, args) => {
    const embedding = await embed(args.description);
    const doc = {
      cuisine: args.cuisine,
      description: args.description,
      embedding,
    };
    await ctx.runMutation(internal.vectorDemo.insertRow, doc);
  },
});

export const insertRow = internalMutation({
  args: {
    description: v.string(),
    cuisine: v.string(),
    embedding: v.array(v.float64()),
  },
  handler: async (ctx, args) => {
    if (!Object.prototype.hasOwnProperty.call(CUISINES, args.cuisine)) {
      throw new Error(`Invalid cuisine: ${args.cuisine}`);
    }
    await ctx.db.insert("foods", args);
  },
});

export const list = query({
  args: {},
  handler: async (ctx) => {
    const docs = await ctx.db.query("foods").order("desc").take(10);
    return docs.map((doc) => {
      return {
        _id: doc._id,
        description: doc.description,
        cuisine: doc.cuisine,
      };
    });
  },
});

export const fetchResults = internalQuery({
  args: {
    results: v.array(v.object({ _id: v.id("foods"), _score: v.float64() })),
  },
  handler: async (ctx, args) => {
    const out: SearchResult[] = [];
    for (const result of args.results) {
      const doc = await ctx.db.get("foods", result._id);
      if (!doc) {
        continue;
      }
      out.push({
        _id: doc._id,
        description: doc.description,
        cuisine: doc.cuisine,
        _score: result._score,
      });
    }
    return out;
  },
});

export const fullTextSearch = query({
  args: {
    query: v.string(),
    cuisine: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    return await ctx.db
      .query("foods")
      .withSearchIndex("by_description", (q) => {
        const result = q.search("description", args.query);
        if (args.cuisine) {
          return result.eq("cuisine", args.cuisine);
        } else {
          return result;
        }
      })
      .collect();
  },
});
