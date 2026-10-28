import { defineApp } from 'convex/server';
import { v } from 'convex/values';

const app = defineApp({
  env: {
    WORKOS_CLIENT_ID: v.string(),
  },
});

export default app;
