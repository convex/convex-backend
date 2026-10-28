import { defineComponent } from "convex/server";
import { v } from "convex/values";

const component = defineComponent("fakeComponent", {
  env: {
    THING_I_NEED: v.string(),
  },
});

export default component;
