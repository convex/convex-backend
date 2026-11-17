---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/username-password
sidebar_label: username-password
---

The values of the username + password method that the backend and the clients
share, exported at `@convex-dev/auth/methods/username-password`. Any runtime can
import this entry point.

## Variables

### DEFAULT_MIN_PASSWORD_LENGTH

```ts
const DEFAULT_MIN_PASSWORD_LENGTH: 10 = 10;
```

The minimum length of a new password in the password methods. The methods export
it (`@convex-dev/auth/methods/username-password`), so that a sign-up form can
show the same limit as the backend.

The password component accepts shorter passwords (`MIN_PASSWORD_LENGTH` of
`components/password/validation.ts`). Thus the methods give this value to
`validateNewPassword`.
