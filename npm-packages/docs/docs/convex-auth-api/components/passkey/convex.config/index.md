---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: components/passkey/convex.config
sidebar_label: convex.config
---

## Variables

### default

```ts
const default: ComponentDefinition<any, {
}>;
```

The passkey (WebAuthn) component.

The component owns the `passkeys` credential table and the short-lived
`challenges` table. It examines registrations and authentications on the server.
It stores credentials with an opaque `userId` only: the app owns the users
table.

The component mounts the batch worker component. It runs the background loop
that erases the expired challenges (see cleanup.ts).

The component has no configuration. The app passes the relying party ID and the
origin as arguments to the finish functions.
