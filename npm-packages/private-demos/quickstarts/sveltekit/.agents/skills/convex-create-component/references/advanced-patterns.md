# Advanced Component Patterns

Additional patterns for Convex components that go beyond the basics covered in the main skill file.

## Function Handles for callbacks

When the app needs to pass a callback function to the component, use function handles. This is common for components that run app-defined logic on a schedule or in a workflow.

```ts
// App side: create a handle and pass it to the component
import { createFunctionHandle } from "convex/server";

export const startJob = mutation({
  handler: async (ctx) => {
    const handle = await createFunctionHandle(internal.myModule.processItem);
    await ctx.runMutation(components.workpool.enqueue, {
      callback: handle,
    });
  },
});
```

```ts
// Component side: accept and invoke the handle
import { v } from "convex/values";
import type { FunctionHandle } from "convex/server";
import { mutation } from "./_generated/server.js";

export const enqueue = mutation({
  args: { callback: v.string() },
  handler: async (ctx, args) => {
    const handle = args.callback as FunctionHandle<"mutation">;
    await ctx.scheduler.runAfter(0, handle, {});
  },
});
```

## Deriving validators from schema

Instead of manually repeating field types in return validators, use
`schema.doc(tableName)` — the table's validator with the `_id` and
`_creationTime` system fields added — and `schema.id(tableName)`:

```ts
import { v } from "convex/values";
import schema from "./schema.js";

export const getLatest = query({
  args: {},
  returns: v.nullable(schema.doc("notifications")),
  handler: async (ctx) => {
    return await ctx.db.query("notifications").order("desc").first();
  },
});
```

## Static configuration with a globals table

A common pattern for component configuration is a single-document "globals" table:

```ts
// schema.ts
export default defineSchema({
  globals: defineTable({
    maxRetries: v.number(),
    webhookUrl: v.optional(v.string()),
  }),
  // ... other tables
});
```

```ts
// lib.ts
export const configure = mutation({
  args: { maxRetries: v.number(), webhookUrl: v.optional(v.string()) },
  returns: v.null(),
  handler: async (ctx, args) => {
    const existing = await ctx.db.query("globals").first();
    if (existing) {
      await ctx.db.patch(existing._id, args);
    } else {
      await ctx.db.insert("globals", args);
    }
    return null;
  },
});
```

## Class-based client wrappers

For components with many functions or configuration options, a class-based client provides a cleaner API. This pattern is common in published components.

```ts
// src/client/index.ts
import type { GenericMutationCtx, GenericDataModel } from "convex/server";
import type { ComponentApi } from "../component/_generated/component.js";

type MutationCtx = Pick<GenericMutationCtx<GenericDataModel>, "runMutation">;

export class Notifications {
  constructor(
    private component: ComponentApi,
    private options?: { defaultChannel?: string },
  ) {}

  async send(ctx: MutationCtx, args: { userId: string; message: string }) {
    return await ctx.runMutation(this.component.lib.send, {
      ...args,
      channel: this.options?.defaultChannel ?? "default",
    });
  }
}
```

```ts
// App usage
import { Notifications } from "@convex-dev/notifications";
import { components } from "./_generated/api";

const notifications = new Notifications(components.notifications, {
  defaultChannel: "alerts",
});

export const send = mutation({
  args: { message: v.string() },
  handler: async (ctx, args) => {
    const userId = await getAuthUserId(ctx);
    await notifications.send(ctx, { userId, message: args.message });
  },
});
```
