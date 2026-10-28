# Convex guidelines
## Function guidelines
### Http endpoint syntax
- HTTP endpoints are defined in `convex/http.ts` and require an `httpAction` decorator. For example:
```typescript
import { httpRouter } from "convex/server";
import { httpAction } from "./_generated/server";
const http = httpRouter();
http.route({
    path: "/echo",
    method: "POST",
    handler: httpAction(async (ctx, req) => {
    const body = await req.bytes();
    return new Response(body, { status: 200 });
    }),
});
```
- HTTP endpoints are always registered at the exact path you specify in the `path` field. For example, if you specify `/api/someRoute`, the endpoint will be registered at `/api/someRoute`.

### Validators
- Below is an example of an array validator:
```typescript
import { mutation } from "./_generated/server";
import { v } from "convex/values";

export default mutation({
args: {
    simpleArray: v.array(v.union(v.string(), v.number())),
},
handler: async (ctx, args) => {
    //...
},
});
```
- Below is an example of a schema with validators that codify a discriminated union type:
```typescript
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
    results: defineTable(
        v.union(
            v.object({
                kind: v.literal("error"),
                errorMessage: v.string(),
            }),
            v.object({
                kind: v.literal("success"),
                value: v.number(),
            }),
        ),
    )
});
```
- Here are the valid Convex types along with their respective validators:
Convex Type  | TS/JS type  |  Example Usage         | Validator for argument validation and schemas  | Notes                                                                                                                                                                                                 |
| ----------- | ------------| -----------------------| -----------------------------------------------| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Id          | string      | `doc._id`              | `v.id(tableName)`                              |                                                                                                                                                                                                       |
| Null        | null        | `null`                 | `v.null()`                                     | JavaScript's `undefined` is not a valid Convex value. Functions the return `undefined` or do not return will return `null` when called from a client. Use `null` instead.                             |
| Int64       | bigint      | `3n`                   | `v.int64()`                                    | Int64s only support BigInts between -2^63 and 2^63-1. Convex supports `bigint`s in most modern browsers.                                                                                              |
| Float64     | number      | `3.1`                  | `v.number()`                                   | Convex supports all IEEE-754 double-precision floating point numbers (such as NaNs). Inf and NaN are JSON serialized as strings.                                                                      |
| Boolean     | boolean     | `true`                 | `v.boolean()`                                  |
| String      | string      | `"abc"`                | `v.string()`                                   | Strings are stored as UTF-8 and must be valid Unicode sequences. Strings must be smaller than the 1MB total size limit when encoded as UTF-8.                                                         |
| Bytes       | ArrayBuffer | `new ArrayBuffer(8)`   | `v.bytes()`                                    | Convex supports first class bytestrings, passed in as `ArrayBuffer`s. Bytestrings must be smaller than the 1MB total size limit for Convex types.                                                     |
| Array       | Array       | `[1, 3.2, "abc"]`      | `v.array(values)`                              | Arrays can have at most 8192 values.                                                                                                                                                                  |
| Object      | Object      | `{a: "abc"}`           | `v.object({property: value})`                  | Convex only supports "plain old JavaScript objects" (objects that do not have a custom prototype). Objects can have at most 1024 entries. Field names must be nonempty and not start with "$" or "_". |
| Record      | Record      | `{"a": "1", "b": "2"}` | `v.record(keys, values)`                       | Records are objects at runtime, but can have dynamic keys. Keys must be only ASCII characters, nonempty, and not start with "$" or "_".                                                               |

### Function registration
- Use `internalQuery`, `internalMutation`, and `internalAction` to register internal functions. These functions are private and aren't part of an app's API. They can only be called by other Convex functions. These functions are always imported from `./_generated/server`.
- Use `query`, `mutation`, and `action` to register public functions. These functions are part of the public API and are exposed to the public Internet. Do NOT use `query`, `mutation`, or `action` to register sensitive internal functions that should be kept private.
- You CANNOT register a function through the `api` or `internal` objects.
- ALWAYS include argument validators for all Convex functions. This includes all of `query`, `internalQuery`, `mutation`, `internalMutation`, `action`, and `internalAction`.

### Function calling
- Use `ctx.runQuery` to call a query from a query, mutation, or action.
- Use `ctx.runMutation` to call a mutation from a mutation or action.
- Use `ctx.runAction` to call an action from an action.
- ONLY call an action from another action if you need to cross runtimes (e.g. from V8 to Node). Otherwise, pull out the shared code into a helper async function and call that directly instead.
- Try to use as few calls from actions to queries and mutations as possible. Queries and mutations are transactions, so splitting logic up into multiple calls introduces the risk of race conditions.
- All of these calls take in a `FunctionReference`. Do NOT try to pass the callee function directly into one of these calls.
- When using `ctx.runQuery`, `ctx.runMutation`, or `ctx.runAction` to call a function in the same file, specify a type annotation on the return value to work around TypeScript circularity limitations. For example,
```
export const f = query({
  args: { name: v.string() },
  handler: async (ctx, args) => {
    return "Hello " + args.name;
  },
});

export const g = query({
  args: {},
  handler: async (ctx, args) => {
    const result: string = await ctx.runQuery(api.example.f, { name: "Bob" });
    return null;
  },
});
```

### Function references
- Use the `api` object defined by the framework in `convex/_generated/api.ts` to call public functions registered with `query`, `mutation`, or `action`.
- Use the `internal` object defined by the framework in `convex/_generated/api.ts` to call internal (or private) functions registered with `internalQuery`, `internalMutation`, or `internalAction`.
- Convex uses file-based routing, so a public function defined in `convex/example.ts` named `f` has a function reference of `api.example.f`.
- A private function defined in `convex/example.ts` named `g` has a function reference of `internal.example.g`.
- Functions can also registered within directories nested within the `convex/` folder. For example, a public function `h` defined in `convex/messages/access.ts` has a function reference of `api.messages.access.h`.

### Pagination
- Define pagination using the following syntax:

```ts
import { v } from "convex/values";
import { query, mutation } from "./_generated/server";
import { paginationOptsValidator } from "convex/server";
export const listWithExtraArg = query({
    args: { paginationOpts: paginationOptsValidator, author: v.string() },
    handler: async (ctx, args) => {
        return await ctx.db
        .query("messages")
        .withIndex("by_author", (q) => q.eq("author", args.author))
        .order("desc")
        .paginate(args.paginationOpts);
    },
});
```
Note: `paginationOpts` is an object with the following properties:
- `numItems`: the maximum number of documents to return (the validator is `v.number()`)
- `cursor`: the cursor to use to fetch the next page of documents (the validator is `v.union(v.string(), v.null())`)
- A query that ends in `.paginate()` returns an object that has the following properties:
- page (contains an array of documents that you fetches)
- isDone (a boolean that represents whether or not this is the last page of documents)
- continueCursor (a string that represents the cursor to use to fetch the next page of documents)


## Schema guidelines
- Always define your schema in `convex/schema.ts`.
- Always import the schema definition functions from `convex/server`.
- System fields are automatically added to all documents and are prefixed with an underscore. The two system fields that are automatically added to all documents are `_creationTime` which has the validator `v.number()` and `_id` which has the validator `v.id(tableName)`.
- Always include all index fields in the index name. For example, if an index is defined as `["field1", "field2"]`, the index name should be "by_field1_and_field2".
- Index fields must be queried in the same order they are defined. If you want to be able to query by "field1" then "field2" and by "field2" then "field1", you must create separate indexes.
- Do not store unbounded lists as an array field inside a document (e.g. `v.array(v.object({...}))`). As the array grows it will hit the 1MB document size limit, and every update rewrites the entire document. Instead, create a separate table for the child items with a foreign key back to the parent.
- Separate high-churn operational data (e.g. heartbeats, online status, typing indicators) from stable profile data. Storing frequently updated fields on a shared document forces every write to contend with reads of the entire document. Instead, create a dedicated table for the high-churn data with a foreign key back to the parent record.

## Authentication guidelines
- Convex supports JWT-based authentication through `convex/auth.config.ts`. ALWAYS create this file when using authentication. Without it, `ctx.auth.getUserIdentity()` will always return `null`.
- Example `convex/auth.config.ts`:
```typescript
export default {
  providers: [
    {
      domain: "https://your-auth-provider.com",
      applicationID: "convex",
    },
  ],
};
```
The `domain` must be the issuer URL of the JWT provider. Convex fetches `{domain}/.well-known/openid-configuration` to discover the JWKS endpoint. The `applicationID` is checked against the JWT `aud` (audience) claim.
- Use `ctx.auth.getUserIdentity()` to get the authenticated user's identity in any query, mutation, or action. This returns `null` if the user is not authenticated, or a `UserIdentity` object with fields like `subject`, `issuer`, `name`, `email`, etc. The `subject` field is the unique user identifier.
- In Convex `UserIdentity`, `tokenIdentifier` is guaranteed and is the canonical stable identifier for the authenticated identity. For any auth-linked database lookup or ownership check, prefer `identity.tokenIdentifier` over `identity.subject`. Do NOT use `identity.subject` alone as a global identity key.
- NEVER accept a `userId` or any user identifier as a function argument for authorization purposes. Always derive the user identity server-side via `ctx.auth.getUserIdentity()`.
- When using an external auth provider with Convex on the client, use `ConvexProviderWithAuth` instead of `ConvexProvider`:
```tsx
import { ConvexProviderWithAuth, ConvexReactClient } from "convex/react";

const convex = new ConvexReactClient(process.env.NEXT_PUBLIC_CONVEX_URL!);

function App({ children }: { children: React.ReactNode }) {
  return (
    <ConvexProviderWithAuth client={convex} useAuth={useYourAuthHook}>
      {children}
    </ConvexProviderWithAuth>
  );
}
```
The `useAuth` prop must return `{ isLoading, isAuthenticated, fetchAccessToken }`. Do NOT use plain `ConvexProvider` when authentication is needed — it will not send tokens with requests.

## Typescript guidelines
- You can use the helper typescript type `Id` imported from './_generated/dataModel' to get the type of the id for a given table. For example if there is a table called 'users' you can use `Id<'users'>` to get the type of the id for that table.
- Use `Doc<"tableName">` from `./_generated/dataModel` to get the full document type for a table.
- Use `QueryCtx`, `MutationCtx`, `ActionCtx` from `./_generated/server` for typing function contexts. NEVER use `any` for ctx parameters — always use the proper context type.
- If you need to define a `Record` make sure that you correctly provide the type of the key and value in the type. For example a validator `v.record(v.id('users'), v.string())` would have the type `Record<Id<'users'>, string>`. Below is an example of using `Record` with an `Id` type in a query:
```ts
import { query } from "./_generated/server";
import { Doc, Id } from "./_generated/dataModel";

export const exampleQuery = query({
    args: { userIds: v.array(v.id("users")) },
    handler: async (ctx, args) => {
        const idToUsername: Record<Id<"users">, string> = {};
        for (const userId of args.userIds) {
            const user = await ctx.db.get("users", userId);
            if (user) {
                idToUsername[user._id] = user.username;
            }
        }

        return idToUsername;
    },
});
```
- Be strict with types, particularly around id's of documents. For example, if a function takes in an id for a document in the 'users' table, take in `Id<'users'>` rather than `string`.

## Full text search guidelines
- A query for "10 messages in channel '#general' that best match the query 'hello hi' in their body" would look like:

const messages = await ctx.db
  .query("messages")
  .withSearchIndex("search_body", (q) =>
    q.search("body", "hello hi").eq("channel", "#general"),
  )
  .take(10);

## Query guidelines
- Do NOT use `filter` in queries. Instead, define an index in the schema and use `withIndex` instead.
- If the user does not explicitly tell you to return all results from a query you should ALWAYS return a bounded collection instead. So that is instead of using `.collect()` you should use `.take()` or paginate on database queries. This prevents future performance issues when tables grow in an unbounded way.
- Never use `.collect().length` to count rows. Convex has no built-in count operator, so if you need a count that stays efficient at scale, maintain a denormalized counter in a separate document and update it in your mutations.
- Convex queries do NOT support `.delete()`. If you need to delete all documents matching a query, use `.take(n)` to read them in batches, iterate over each batch calling `ctx.db.delete(row._id)`, and repeat until no more results are returned.
- Convex mutations are transactions with limits on the number of documents read and written. If a mutation needs to process more documents than fit in a single transaction (e.g. bulk deletion on a large table), process a batch with `.take(n)` and then call `ctx.scheduler.runAfter(0, api.myModule.myMutation, args)` to schedule itself to continue. This way each invocation stays within transaction limits.
- Use `.unique()` to get a single document from a query. This method will throw an error if there are multiple documents that match the query.
- When using async iteration, don't use `.collect()` or `.take(n)` on the result of a query. Instead, use the `for await (const row of query)` syntax.
### Ordering
- By default Convex always returns documents in ascending `_creationTime` order.
- You can use `.order('asc')` or `.order('desc')` to pick whether a query is in ascending or descending order. If the order isn't specified, it defaults to ascending.
- Document queries that use indexes will be ordered based on the columns in the index and can avoid slow table scans.


## Mutation guidelines
- Use `ctx.db.replace` to fully replace an existing document. This method will throw an error if the document does not exist. Syntax: `await ctx.db.replace('tasks', taskId, { name: 'Buy milk', completed: false })`
- Use `ctx.db.patch` to shallow merge updates into an existing document. This method will throw an error if the document does not exist. Syntax: `await ctx.db.patch('tasks', taskId, { completed: true })`

## Action guidelines
- Always add `"use node";` to the top of files containing actions that use Node.js built-in modules.
- Never add `"use node";` to a file that also exports queries or mutations. Only actions can run in the Node.js runtime; queries and mutations must stay in the default Convex runtime. If you need Node.js built-ins alongside queries or mutations, put the action in a separate file.
- `fetch()` is available in the default Convex runtime. You do NOT need `"use node";` just to use `fetch()`.
- Never use `ctx.db` inside of an action. Actions don't have access to the database.
- Below is an example of the syntax for an action:
```ts
import { action } from "./_generated/server";

export const exampleAction = action({
    args: {},
    handler: async (ctx, args) => {
        console.log("This action does not return anything");
        return null;
    },
});
```

## Scheduling guidelines
### Cron guidelines
- Only use the `crons.interval` or `crons.cron` methods to schedule cron jobs. Do NOT use the `crons.hourly`, `crons.daily`, or `crons.weekly` helpers.
- Both cron methods take in a FunctionReference. Do NOT try to pass the function directly into one of these methods.
- Define crons by declaring the top-level `crons` object, calling some methods on it, and then exporting it as default. For example,
```ts
import { cronJobs } from "convex/server";
import { internal } from "./_generated/api";
import { internalAction } from "./_generated/server";

const empty = internalAction({
  args: {},
  handler: async (ctx, args) => {
    console.log("empty");
  },
});

const crons = cronJobs();

// Run `internal.crons.empty` every two hours.
crons.interval("delete inactive users", { hours: 2 }, internal.crons.empty, {});

export default crons;
```
- You can register Convex functions within `crons.ts` just like any other file.
- If a cron calls an internal function, always import the `internal` object from '_generated/api', even if the internal function is registered in the same file.


## Testing guidelines
- Use `convex-test` with `vitest` and `@edge-runtime/vm` to test Convex functions. Always install the latest versions of these packages. Configure vitest with `environment: "edge-runtime"` in `vitest.config.ts`.

Test files go inside the `convex/` directory. You must pass a module map from `import.meta.glob` to `convexTest`:
```typescript
/// <reference types="vite/client" />
import { convexTest } from "convex-test";
import { expect, test } from "vitest";
import { api } from "./_generated/api";
import schema from "./schema";

const modules = import.meta.glob("./**/*.ts");

test("some behavior", async () => {
  const t = convexTest(schema, modules);
  await t.mutation(api.messages.send, { body: "Hi!", author: "Sarah" });
  const messages = await t.query(api.messages.list);
  expect(messages).toMatchObject([{ body: "Hi!", author: "Sarah" }]);
});
```
The `modules` argument is required so convex-test can discover and load function files. The `/// <reference types="vite/client" />` directive is needed for TypeScript to recognize `import.meta.glob`.

## File storage guidelines
- The `ctx.storage.getUrl()` method returns a signed URL for a given file. It returns `null` if the file doesn't exist.
- Do NOT use the deprecated `ctx.storage.getMetadata` call for loading a file's metadata.

Instead, query the `_storage` system table. For example, you can use `ctx.db.system.get` to get an `Id<"_storage">`.
```
import { query } from "./_generated/server";
import { Id } from "./_generated/dataModel";

type FileMetadata = {
    _id: Id<"_storage">;
    _creationTime: number;
    contentType?: string;
    sha256: string;
    size: number;
}

export const exampleQuery = query({
    args: { fileId: v.id("_storage") },
    handler: async (ctx, args) => {
        const metadata: FileMetadata | null = await ctx.db.system.get("_storage", args.fileId);
        console.log(metadata);
        return null;
    },
});
```
- Convex storage stores items as `Blob` objects. You must convert all items to/from a `Blob` when using Convex storage.


