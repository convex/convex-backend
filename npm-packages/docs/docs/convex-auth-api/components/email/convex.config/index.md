---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: components/email/convex.config
sidebar_label: convex.config
---

## Variables

### default

```ts
const default: ComponentDefinition<any, {
}>;
```

The email component.

Tracks the verified email addresses of users, keyed only by an opaque `userId`.
An email address goes into this component only after the user proves ownership
of it through the challenge. The app owns the users table and maps its own
identifiers onto the `userId` it passes in.

Mounts the rate-limiter component to throttle the `start` mutations per
destination address and per client IP, and the batch worker component, which
runs the background loop that erases the expired challenges (see cleanup.ts).
