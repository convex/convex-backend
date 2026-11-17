---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: components/google/convex.config
sidebar_label: convex.config
---

## Variables

### default

```ts
const default: ComponentDefinition<any, {
  CLIENT_ID: VString<string, "required">;
  CLIENT_SECRET: VString<string, "required">;
}>;
```

The Google oauth component. Install it once, with the `httpPrefix` its callback
is served under, and bind the OAuth client's credentials:

```ts
app.use(authGoogle, {
  httpPrefix: "/oauth/google",
  env: {
    CLIENT_ID: app.env.AUTH_GOOGLE_CLIENT_ID,
    CLIENT_SECRET: app.env.AUTH_GOOGLE_CLIENT_SECRET,
  },
});
```
