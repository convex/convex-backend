import { defineApp } from "convex/server";
import { v } from "convex/values";

const app = defineApp({
  env: {
    // Origin of the website allowed to call the HTTP actions.
    CLIENT_ORIGIN: v.string(),
  },
});

export default app;
