import { defineApp } from "convex/server";
import { v } from "convex/values";

const app = defineApp({
  env: {
    OPENAI_KEY: v.string(),
  },
});

export default app;
