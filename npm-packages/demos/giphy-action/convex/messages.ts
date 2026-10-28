import {
  query,
  mutation,
  action,
  env,
  internalMutation,
} from "./_generated/server";
import { internal } from "./_generated/api";
import { v } from "convex/values";

export const list = query({
  args: {},
  handler: async (ctx) => {
    return await ctx.db.query("messages").collect();
  },
});

export const send = mutation({
  args: {
    body: v.string(),
    author: v.string(),
  },
  handler: async (ctx, { body, author }) => {
    const message = { body, author, format: "text" };
    await ctx.db.insert("messages", message);
  },
});

function giphyUrl(queryString: string) {
  return (
    "https://api.giphy.com/v1/gifs/translate?api_key=" +
    env.GIPHY_KEY +
    "&s=" +
    encodeURIComponent(queryString)
  );
}

// Post a GIF chat message corresponding to the query string.
export const sendGif = action({
  args: { queryString: v.string(), author: v.string() },
  handler: async (ctx, { queryString, author }) => {
    // Fetch GIF url from GIPHY.
    const data = await fetch(giphyUrl(queryString));
    const json = await data.json();
    if (!data.ok) {
      throw new Error(`Giphy errored: ${JSON.stringify(json)}`);
    }
    const gifEmbedUrl = json.data.embed_url;

    // Write GIF url to Convex.
    await ctx.runMutation(internal.messages.sendGifMessage, {
      body: gifEmbedUrl,
      author,
    });
  },
});

export const sendGifMessage = internalMutation({
  args: {
    body: v.string(),
    author: v.string(),
  },
  handler: async (ctx, { body, author }) => {
    const message = { body, author, format: "giphy" };
    await ctx.db.insert("messages", message);
  },
});
