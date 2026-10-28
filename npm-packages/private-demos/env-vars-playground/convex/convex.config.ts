import { defineApp } from "convex/server";
import { v } from "convex/values";
import fakeComponent from "../fakecomponent/convex.config";
import optionalComponent from "../optionalcomponent/convex.config";

const app = defineApp({
  env: {
    GREETING: v.optional(v.union(v.literal("yo"), v.literal("hey"))),
    MESSAGE: v.string(),
  },
});

// fakeComponent declares a required env var, so `env` must be passed.
app.use(fakeComponent, { env: { THING_I_NEED: "a string" } });

// optionalComponent declares only an optional env var, so `env` is optional.
app.use(optionalComponent);
// ...but it can still be passed. Use a distinct name to install it twice.
app.use(optionalComponent, {
  name: "optionalComponentWithEnv",
  env: { OPTIONAL_THING: "a string" },
});

export default app;
