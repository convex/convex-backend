import { defineComponent } from "convex/server";
import { v } from "convex/values";

const component = defineComponent("optionalComponent", {
  env: {
    OPTIONAL_THING: v.optional(v.string()),
  },
});

export default component;
