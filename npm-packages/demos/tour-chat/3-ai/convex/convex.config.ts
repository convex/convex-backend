import { defineApp } from "convex/server";
import { v } from "convex/values";

const app = defineApp({
  env: {
    TOGETHER_API_KEY: v.string(),
  },
});

export default app;
