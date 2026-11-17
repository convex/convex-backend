---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/username-password/server
sidebar_label: server
---

import Link from "@docusaurus/Link";

## Type Aliases

### ChangePasswordResult

```ts
type ChangePasswordResult = Infer<typeof changePasswordResult>;
```

The result of `changePassword`.

`INVALID_CREDENTIALS` and `RATE_LIMITED` are about the current password. The
other errors are about the new password.

---

### SignInResult

```ts
type SignInResult = Infer<typeof signInResult>;
```

The result of `signInWithUsernamePassword`.

When complete the minted session tokens, otherwise a user-facing `userError`.

---

### SignUpResult

```ts
type SignUpResult = Infer<typeof signUpResult>;
```

The result of `signUpWithUsernamePassword`.

When complete the minted session tokens, otherwise a user-facing `userError`.

---

### UsernamePasswordOptions

```ts
type UsernamePasswordOptions = object;
```

Options for [setupUsernamePassword](#setupusernamepassword).

#### Properties

| Property                                                     | Type                   | Description                                                                                                                                                                            |
| ------------------------------------------------------------ | ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-component" /> `component`                 | `ComponentApi`         | The mounted password component (`components.authPassword`). The recipe drives its `_internal_dangerous_setPassword` / `verifyPassword` mutations.                                      |
| <Link id="property-usernamecomponent" /> `usernameComponent` | `UsernameComponentApi` | The mounted username component (`components.authUsername`). The recipe uses it to map a username onto the app user id: it stores the username at sign-up and reads it back at sign-in. |

## Functions

### setupUsernamePassword()

```ts
function setupUsernamePassword<UsersTable>(core, options): object;
```

The simplest password recipe: every account is a `(username, password)` pair,
with no email or email verification. Wire it up in `convex/auth.ts`:

```ts
const auth = setupConvexAuth({ component: components.auth });
export const { signOut, refreshSession, isAuthenticated } = auth;

export const {
  signUpWithUsernamePassword,
  signInWithUsernamePassword,
  changePassword,
} = setupUsernamePassword(auth, {
  component: components.authPassword,
  usernameComponent: components.authUsername,
}).attachUserCallbacks({ createUser: internal.users.createUserPassword });
```

The app re-exports the returned `signUpWithUsernamePassword` /
`signInWithUsernamePassword` / `changePassword` mutations so its clients can
call them.

Account resolution (username → app user id) is owned by the username component:
the method stores the username there at sign-up, and reads the user id back from
it at sign-in. The password component itself stores only
`{ userId, passwordHash }` and knows nothing about usernames.

#### Type Parameters

| Type Parameter                  |
| ------------------------------- |
| `UsersTable` _extends_ `string` |

#### Parameters

| Parameter | Type                                                            |
| --------- | --------------------------------------------------------------- |
| `core`    | [`AuthCore`](../../../server/index.md#authcore)\<`UsersTable`\> |
| `options` | [`UsernamePasswordOptions`](#usernamepasswordoptions)           |

#### Returns

##### attachUserCallbacks()

```ts
attachUserCallbacks(__namedParameters): object;
```

Supply the app's user callbacks (see UserCallbacks for how their args must be
declared) and get the functions of this method to export.

###### Parameters

| Parameter           | Type                                                                                 |
| ------------------- | ------------------------------------------------------------------------------------ |
| `__namedParameters` | `UserCallbacks`\<`"usernamePassword"`, `Record`\<`string`, `never`\>, `UsersTable`\> |

###### Returns

###### changePassword

```ts
changePassword: RegisteredMutation<
  "public",
  {
    currentPassword: string;
    newPassword: string;
    signOutOtherSessions?: boolean;
  },
  Promise<
    | {
        success: true;
      }
    | {
        success: false;
        userError:
          | {
              error: "PASSWORD_TOO_SHORT";
              minimumLength: number;
            }
          | {
              error: "PASSWORD_TOO_LONG";
              maximumLength: number;
            }
          | {
              error: "PASSWORD_HAS_SURROUNDING_WHITESPACE";
            }
          | {
              error: "PASSWORD_TOO_COMMON";
            }
          | {
              error: "NOT_LOGGED_IN";
            }
          | {
              error: "INVALID_CREDENTIALS";
            }
          | {
              error: "RATE_LIMITED";
              retryAfterMs: number;
            };
      }
  >
>;
```

Change the password of the signed-in user. The user must re-authenticate by
giving their current password. Signs out the user's other sessions if
`signOutOtherSessions` is `true`.

###### signInWithUsernamePassword

```ts
signInWithUsernamePassword: RegisteredMutation<
  "public",
  {
    password: string;
    username: string;
  },
  Promise<
    | {
        status: "complete";
        tokens: {
          accessToken: string;
          refreshToken: string;
          refreshTokenExpiresAt: number;
          userId: string;
        };
      }
    | {
        status: "error";
        userError:
          | {
              error: "PASSWORD_TOO_SHORT";
              minimumLength: number;
            }
          | {
              error: "PASSWORD_TOO_LONG";
              maximumLength: number;
            }
          | {
              error: "PASSWORD_HAS_SURROUNDING_WHITESPACE";
            }
          | {
              error: "INVALID_CREDENTIALS";
            }
          | {
              error: "RATE_LIMITED";
              retryAfterMs: number;
            }
          | {
              error: "USER_NOT_FOUND";
            };
      }
  >
>;
```

Verify an existing account's password and, on success, mint a session. Returns
`USER_NOT_FOUND` when the username has no account and `INVALID_CREDENTIALS` when
the password is wrong, so callers can tell the two apart. (Account existence is
already observable via sign-up's `USERNAME_TAKEN`, so distinguishing them here
leaks nothing new.)

###### signUpWithUsernamePassword

```ts
signUpWithUsernamePassword: RegisteredMutation<
  "public",
  {
    password: string;
    username: string;
  },
  Promise<
    | {
        status: "complete";
        tokens: {
          accessToken: string;
          refreshToken: string;
          refreshTokenExpiresAt: number;
          userId: string;
        };
      }
    | {
        status: "error";
        userError:
          | {
              error: "PASSWORD_TOO_SHORT";
              minimumLength: number;
            }
          | {
              error: "PASSWORD_TOO_LONG";
              maximumLength: number;
            }
          | {
              error: "PASSWORD_HAS_SURROUNDING_WHITESPACE";
            }
          | {
              error: "PASSWORD_TOO_COMMON";
            }
          | {
              error: "USERNAME_TOO_SHORT";
              minimumLength: number;
            }
          | {
              error: "USERNAME_TOO_LONG";
              maximumLength: number;
            }
          | {
              error: "USERNAME_HAS_SURROUNDING_WHITESPACE";
            }
          | {
              error: "USERNAME_HAS_INVALID_CHARACTERS";
            }
          | {
              error: "USERNAME_TAKEN";
            };
      }
  >
>;
```

Create a new account: reject a taken username or an invalid password, otherwise
create the user + session and store the username and the password.
