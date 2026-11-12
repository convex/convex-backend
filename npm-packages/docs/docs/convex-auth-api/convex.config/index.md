---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: convex.config
---

## Variables

### default

```ts
const default: ComponentDefinition<any, {
}>;
```

The core auth component.

Owns session lifetimes (`sessions`: refresh tokens, rotation, expiry). The
mapping from a provider account to an app user lives in the component of each
sign-in method, not here.

Access tokens are signed with the deployment's own key (`ctx.auth.signJwt`), so
the component carries no signing material of its own. The public keys live at
`${CONVEX_CLOUD_URL}/.well-known/jwks.json`, and tokens minted here carry
`iss = ${CONVEX_CLOUD_URL}/components/auth`.

It does NOT know about any specific sign-in method. Methods hand it identity
claims (see ../lib/types) and the core turns that into a session + JWT.

Apps install it from `@convex-dev/auth/convex.config.js`.
