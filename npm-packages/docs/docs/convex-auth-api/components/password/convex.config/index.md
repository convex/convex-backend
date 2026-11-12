---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: components/password/convex.config
sidebar_label: convex.config
---

## Variables

### default

```ts
const default: ComponentDefinition<any, {
}>;
```

The password component.

Securely stores and verifies passwords, keyed only by an opaque `userId`. It
holds no email/username and knows nothing about the identity behind a user id —
intentionally reusable in any context. The app owns the users table and maps its
own identifiers onto the `userId` it passes in.

Mounts the rate-limiter component to throttle `verifyPassword` per user id.
