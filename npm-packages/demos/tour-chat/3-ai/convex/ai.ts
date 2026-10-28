// @snippet start openai
import { action, env } from "./_generated/server";
import { api } from "./_generated/api";
import { v } from "convex/values";

const TOGETHER_API_KEY = env.TOGETHER_API_KEY;

export const chat = action({
  args: {
    messageBody: v.string(),
  },
  handler: async (ctx, args) => {
    const res = await fetch("https://api.together.xyz/v1/chat/completions", {
      method: "POST",
      headers: {
        Authorization: `Bearer ${TOGETHER_API_KEY}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        model: "meta-llama/Llama-3-8b-chat-hf",
        messages: [
          {
            // Provide a 'system' message to add context about how to respond
            // (feel free to change this to give your AI agent personality!)
            role: "system",
            content:
              "You are a terse bot in a group chat responding to questions with 1-sentence answers.",
          },
          {
            // Pass on the chat user's message to the AI agent
            role: "user",
            content: args.messageBody,
          },
        ],
      }),
    });

    const json = await res.json();
    // Pull the message content out of the response
    const messageContent = json.choices[0].message?.content;

    // highlight-start
    // Send AI's response as a new message
    await ctx.runMutation(api.messages.send, {
      author: "AI Agent",
      body: messageContent || "Sorry, I don't have an answer for that.",
    });
    // highlight-end
  },
});
// @snippet end openai
