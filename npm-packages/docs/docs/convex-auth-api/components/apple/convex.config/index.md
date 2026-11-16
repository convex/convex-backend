---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: components/apple/convex.config
sidebar_label: convex.config
---

## Variables

### default

```ts
const default: ComponentDefinition<any, {
  CLIENT_ID: VString<string, "required">;
  KEY_ID: VString<string, "required">;
  PRIVATE_KEY: VString<string, "required">;
  TEAM_ID: VString<string, "required">;
}>;
```

The Apple oauth component. Install it once, with the `httpPrefix` its callback
is served under, and bind the four values from your Apple developer account:

```ts
app.use(authApple, {
  httpPrefix: "/oauth/apple",
  env: {
    CLIENT_ID: app.env.AUTH_APPLE_CLIENT_ID,
    TEAM_ID: app.env.AUTH_APPLE_TEAM_ID,
    KEY_ID: app.env.AUTH_APPLE_KEY_ID,
    PRIVATE_KEY: app.env.AUTH_APPLE_PRIVATE_KEY,
  },
});
```

Apple has no static client secret - instead, it takes the pieces needed to sign
a fresh one for every token exchange:

- `CLIENT_ID` is the Services ID registered for your website, e.g.
  `com.example.app.web`.
- `TEAM_ID` is your 10-character Apple developer team id.
- `KEY_ID` is the 10-character id of the Sign in with Apple key.
- `PRIVATE_KEY` is the contents of that key's `AuthKey_<KEY_ID>.p8` file.
