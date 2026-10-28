---
title: "api.js"
sidebar_position: 2
description:
  "Generated API references for your Convex functions and internal calls"
---

<Admonition type="caution" title="This code is generated">

These exports are not directly available in the `convex` package!

Instead you need to run `npx convex dev` to create `convex/_generated/api.js`
and `convex/_generated/api.d.ts`.

</Admonition>

These types require running code generation because they are specific to the
Convex functions you define for your app.

If you aren't using code generation, you can use
[`makeFunctionReference`](/api/modules/server#makefunctionreference) instead.

### api

An object of type `API` describing your app's public Convex API.

Its `API` type includes information about the arguments and return types of your
app's Convex functions.

The api object is used by client-side React hooks and Convex functions that run
or schedule other functions.

```javascript title="src/App.tsx"
import { api } from "../convex/_generated/api";
import { useQuery } from "convex/react";

const data = useQuery(api.messages.list);
```

### internal

Another object of type `API` describing your app's internal Convex API.

```js title="convex/upgrade.ts"
import { action } from "../_generated/server";
import { internal } from "../_generated/api";

export default action({
  handler: async ({ runMutation }, { planId, ... }) => {
    // Call out to payment provider (e.g. Stripe) to charge customer
    const response = await fetch(...);
    if (response.ok) {
      // Mark the plan as "professional" in the Convex DB
      await runMutation(internal.plans.markPlanAsProfessional, { planId });
    }
  },
});
```
