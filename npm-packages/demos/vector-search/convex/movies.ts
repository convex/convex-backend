import { v } from "convex/values";
import {
  query,
  action,
  env,
  internalMutation,
  mutation,
  internalAction,
} from "./_generated/server";
import { api, internal } from "./_generated/api";
import { EXAMPLE_MOVIES } from "./constants";
import { Doc, Id } from "./_generated/dataModel";

export type Result = Doc<"movies"> & { _score: number };
export type SearchResult = { _id: Id<"movieEmbeddings">; _score: number };

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
    for (const doc of EXAMPLE_MOVIES) {
      await ctx.runMutation(api.movies.insert, {
        title: doc.title,
        description: doc.description,
        genre: doc.genre,
      });
    }
  },
});

export const insert = mutation({
  args: { title: v.string(), description: v.string(), genre: v.string() },
  handler: async (ctx, args) => {
    const movieId = await ctx.db.insert("movies", {
      description: args.description,
      genre: args.genre,
      title: args.title,
      votes: 0,
    });
    // Kick off an action to generate an embedding for this movie
    await ctx.scheduler.runAfter(0, internal.movies.generateAndAddEmbedding, {
      movieId,
      description: args.description,
    });
  },
});

export const generateAndAddEmbedding = internalAction({
  args: { movieId: v.id("movies"), description: v.string() },
  handler: async (ctx, args) => {
    const embedding = await embed(args.description);
    await ctx.runMutation(internal.movies.addEmbedding, {
      movieId: args.movieId,
      embedding,
    });
  },
});

export const addEmbedding = internalMutation({
  args: { movieId: v.id("movies"), embedding: v.array(v.number()) },
  handler: async (ctx, args) => {
    const movie = await ctx.db.get("movies", args.movieId);
    if (movie === null) {
      // No movie to update
      return;
    }
    const movieEmbeddingId = await ctx.db.insert("movieEmbeddings", {
      embedding: args.embedding,
      genre: movie.genre,
    });
    await ctx.db.patch("movies", args.movieId, {
      embeddingId: movieEmbeddingId,
    });
  },
});

export const list = query({
  args: {},
  handler: async (ctx) => {
    return await ctx.db.query("movies").order("desc").take(10);
  },
});
// @snippet start fetchResults
export const fetchResults = query({
  args: {
    results: v.array(
      v.object({ _id: v.id("movieEmbeddings"), _score: v.float64() }),
    ),
  },
  handler: async (ctx, args) => {
    const out: Result[] = [];
    for (const result of args.results) {
      const doc = await ctx.db
        .query("movies")
        .withIndex("by_embedding", (q) => q.eq("embeddingId", result._id))
        .unique();
      if (doc === null) {
        continue;
      }
      out.push({
        ...doc,
        _score: result._score,
      });
    }
    return out;
  },
});
// @snippet end fetchResults
export const upvote = mutation({
  args: { id: v.id("movies") },
  handler: async (ctx, args) => {
    const movie = await ctx.db.get("movies", args.id);
    if (movie === null) {
      return;
    }
    await ctx.db.patch("movies", args.id, {
      votes: movie.votes + 1,
    });
  },
});

export const downvote = mutation({
  args: { id: v.id("movies") },
  handler: async (ctx, args) => {
    const movie = await ctx.db.get("movies", args.id);
    if (movie === null) {
      return;
    }
    await ctx.db.patch("movies", args.id, {
      votes: movie.votes - 1,
    });
  },
});
