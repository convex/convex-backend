---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: components/username/convex.config
sidebar_label: convex.config
---

## Variables

### default

```ts
const default: ComponentDefinition<any, {
}>;
```

The username component.

The component owns the mapping between a username and an opaque app `userId`. It
stores one username for each user, and makes sure that no two users have the
same username.

The component is not a sign-in method. It knows nothing about credentials or
sessions. A sign-in method (for example `usernamePassword`) uses it to find the
user that a username identifies.

The component has no configuration.
