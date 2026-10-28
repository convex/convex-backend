"use node";

import OpenAI from "openai";
import { action, env } from "./_generated/server";
import { internal } from "./_generated/api";
import { v } from "convex/values";

export const send = action({
  args: {
    prompt: v.string(),
    author: v.string(),
  },
  handler: async (ctx, { prompt, author }) => {
    const openai = new OpenAI({ apiKey: env.OPENAI_API_KEY });

    // Check if the prompt is offensive.
    const modResponse = await openai.moderations.create({
      input: prompt,
    });
    const modResult = modResponse.results[0];
    if (modResult.flagged) {
      throw new Error(
        `Your prompt was flagged: ${JSON.stringify(modResult.categories)}`,
      );
    }

    // Query OpenAI for the image.
    const openaiResponse = await openai.images.generate({
      prompt,
      size: "256x256",
    });
    const dallEImageUrl = openaiResponse.data![0]["url"]!;

    // @snippet start storeImage
    // Download the image
    const imageResponse = await fetch(dallEImageUrl);
    if (!imageResponse.ok) {
      throw new Error(`failed to download: ${imageResponse.statusText}`);
    }

    // Store the image to Convex storage.
    const image = await imageResponse.blob();
    // TODO update storage.store to accept whatever kind of Blob is returned from fetch
    const storageId = await ctx.storage.store(image as Blob);

    // Write storageId as the body of the message to the Convex database.
    await ctx.runMutation(internal.messages.sendDallEMessage, {
      body: storageId,
      author,
      prompt,
    });
    // @snippet end storeImage
  },
});
