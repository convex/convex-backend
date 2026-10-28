# @convex-dev/react-query

Instead of polling you subscribe to receive update from server-side query
functions in a Convex deployment. Convex is a database with server-side
(db-side? like stored procedures) functions that update reactively.

New results for all relevant subscriptions are pushed to the client where they
update at the same time so data is never stale and there's no need to call
`queryClient.invalidateQueries()`.

## Setup

See [./src/example.tsx](./src/example.tsx) for a real example. The general
pattern:

1. Create a ConvexClient and ConvexQueryClient. Set the global default
   `queryKeyHashFn` to `convexQueryClient.hashFn()` and `queryFn` to
   `convexQueryClient.queryFn()`. Connect the ConvexQueryClient to the React
   Query QueryClient.

```ts
const convexClient = new ConvexReactClient(import.meta.env.VITE_CONVEX_URL);
const convexQueryClient = new ConvexQueryClient(convexClient);
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      queryKeyHashFn: convexQueryClient.hashFn(),
      queryFn: convexQueryClient.queryFn(),
    },
  },
});
convexQueryClient.connect(queryClient);
```

2. Use `useQuery()` with the `convexQuery` options factory function called with
   an `api` object imported from `../convex/_generated/server` and the arguments
   for this query function. These two form the query key.

```ts
const { isPending, error, data } = useQuery({
  ...convexQuery(api.repos.get, { repo: "made/up" }),
  gcTime: 10000, // unsubscribe after 10s of no use
});
```

`staleTime` is set to `Infinity` beacuse this data is never stale; it's
proactively updated whenever the query result updates on the server. (see
[tkdodo's post](https://tkdodo.eu/blog/using-web-sockets-with-react-query#increasing-staletime)
for more about this) If you like, customize the `gcTime` to the length of time a
query subscription should remain active after all `useQuery()` hooks using it
have unmounted.

If you need to use a Convex Action as a query, it won't be reactive; you'll get
all the normal tools from React Query to refetch it.

# Differences from using TanStack Query with `fetch`

New query results are pushed from the server, so a `staleTime` of `Infinity`
should be used.

Your app will remain subscribed to a query until the `gcTime` has elapsed. Tune
this for your app: it's usually a good tradeoff to use a value of at least a
couple seconds.

# Example

To run this example:

- `npm install`
- `npm run dev`

# Mutations and Actions

If you wrap your app in a `ConvexProvider` you'll be able to use convex hooks
like `useConvexMutation` and `useConvexAction`.:

```tsx
<ConvexProvider client={convex}>
  <QueryClientProvider client={queryClient}>
    <App />
  </QueryClientProvider>
</ConvexProvider>
```

You can use this mutation function directly or wrap it in a TanStack Query
`useMutation`:

```ts
const mutationFn = useConvexMutation(api.board.createColumn);
const { mutate } = useMutation({ mutationFn });
```

```ts
const { mutate } = useMutation({
  mutationFn: useConvexAction(api.time.getTotal),
});
```

# Authentication

**Note:** The example app includes a basic Convex Auth implementation for
reference.

TanStack Query isn't opionated about auth; an auth code might be a an element of
a query key like any other. With Convex it's not necessary to add an additional
key for an auth code; auth is an implicit argument to all Convex queries and
these queries will be retried when authentication info changes.

Convex auth is typically done via JWT: some query functions will fail if
requested before calling `convexReactClinet.setAuth()` with a function that
provides the token.

Auth setup looks just like it's recommended in
[Convex docs](https://docs.convex.dev/auth), which make use of components that
use native convex hooks. For Clerk, this might look like this: a `ClerkProvider`
for auth, a `ConvexProviderWithClerk` for the convex client, and a
`QueryClient`.

```
<ClerkProvider publishableKey="pk_test_...">
  <ConvexProviderWithClerk client={convex} useAuth={useAuth}>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </ConvexProviderWithClerk>
</ClerkProvider>
```

See the [Convex Auth docs](https://docs.convex.dev/auth) for setup instructions.

# TODO

- auth
- paginated queries
- cleanup / unsubscribe in useEffect; something with hot reloading may not be
  working right

# Contributing

After cloning this repo run `npm i` to install dependencies. This package uses
[tshy](https://github.com/isaacs/tshy) to publish an ESM. If there's ever demand
for a CJS build we can add "cjs" to the "dialects" section of "tshy" config in
the package.json.

To publish an alpha release, update the version in package.json to something
like `0.0.0-alpha.1` and run `just pnpm publish --tag alpha`.

To publish a regular release, update the version in package.json to something
like `0.1.2` and run `just pnpm publish`.
