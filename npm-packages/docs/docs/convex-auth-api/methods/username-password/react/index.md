---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/username-password/react
sidebar_label: react
---

import Link from "@docusaurus/Link";

React client for the `usernamePassword` method, exported at
`@convex-dev/auth/methods/username-password/react`.

A method's job on the client is to run its own sign-in flow and hand the
resulting TokenBundle to the core client's `setSession` (see useAuthActions).

The `usernamePassword` method has two flows and provides a hook for each:

1.  signing in to an existing account
    ([useSignInWithUsernamePassword](#usesigninwithusernamepassword))
2.  signing up a new one
    ([useSignUpWithUsernamePassword](#usesignupwithusernamepassword))

Each hook returns a function for sending up the credentials and a `pending`
value that is flipped to `true` while the credentials are being validated.

## Type Aliases

### Credentials

```ts
type Credentials = object;
```

The `(username, password)` pair both flows accept.

#### Properties

| Property                                   | Type     |
| ------------------------------------------ | -------- |
| <Link id="property-password" /> `password` | `string` |
| <Link id="property-username" /> `username` | `string` |

---

### SignInWithUsernamePasswordResult

```ts
type SignInWithUsernamePasswordResult =
  | ClientView<SignInResult>
  | UnexpectedFailure;
```

The result of the `signIn` callback from
[useSignInWithUsernamePassword](#usesigninwithusernamepassword).

---

### SignUpWithUsernamePasswordResult

```ts
type SignUpWithUsernamePasswordResult =
  | ClientView<SignUpResult>
  | UnexpectedFailure;
```

The result of the `signUp` callback from
[useSignUpWithUsernamePassword](#usesignupwithusernamepassword).

## Functions

### useSignInWithUsernamePassword()

```ts
function useSignInWithUsernamePassword(api): object;
```

Client for the sign-in flow of the `usernamePassword` method: wire the backend's
`signInWithUsernamePassword` mutation to the core client.

The returned `signIn` runs the mutation with the given credentials and, on
success, establishes an authenticated session with your Convex backend.

The `pending` flag returned will let you know if the credentials are currently
being validated.

After calling `signIn`, check the `status` field on the return value to see
whether the sign-in was successful or if you need to handle an error.

```tsx
import { useSignInWithUsernamePassword } from "@convex-dev/auth/methods/username-password/react";
import { api } from "../convex/_generated/api";

function LogIn() {
  const { signIn, pending } = useSignInWithUsernamePassword(api.auth);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        const result = await signIn({ username, password });
        if (result.status !== "complete") {
          // map result.userError to a message
        }
      }}
    >
      <label htmlFor="username">
        Username
        <input
          // A static id helps password managers
          id="username"
          name="username"
          type="text"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          autoComplete="username"
          required
          disabled={pending}
        />
      </label>
      <label htmlFor="password">
        Password
        <input
          // A static id helps password managers
          id="password"
          name="password"
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          autoComplete="current-password"
          required
          disabled={pending}
        />
      </label>
      <button type="submit" disabled={pending}>
        Log in
      </button>
    </form>
  );
}
```

#### Parameters

| Parameter                        | Type                                                                      | Description                                      |
| -------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------ |
| `api`                            | \{ `signInWithUsernamePassword`: `SignInWithUsernamePasswordMutation`; \} | The functions of the method, usually `api.auth`. |
| `api.signInWithUsernamePassword` | `SignInWithUsernamePasswordMutation`                                      | -                                                |

#### Returns

##### pending

```ts
pending: boolean;
```

`true` if the sign-in attempt is being validated.

##### signIn

```ts
signIn: (credentials) =>
  (Promise<
    | ClientView<
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
    | UnexpectedFailure
  > = run);
```

Passes up the given credentials to perform a username/password sign in.

Returns an object with a `status` field.

If it is `"complete"` the sign-in was successful and the client will establish
an authenticated session with the Convex backend server.

If it is `"error"` the returned object will have a `userError` field with
additional details about why sign-in failed.

###### Parameters

| Parameter     | Type                          |
| ------------- | ----------------------------- |
| `credentials` | [`Credentials`](#credentials) |

###### Returns

`Promise`\< \| [`ClientView`](../../../ssr/index.md#clientview)\< \| \{
`status`: `"complete"`; `tokens`: \{ `accessToken`: `string`; `refreshToken`:
`string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`; \}; \} \| \{
`status`: `"error"`; `userError`: \| \{ `error`: `"PASSWORD_TOO_SHORT"`;
`minimumLength`: `number`; \} \| \{ `error`: `"PASSWORD_TOO_LONG"`;
`maximumLength`: `number`; \} \| \{ `error`:
`"PASSWORD_HAS_SURROUNDING_WHITESPACE"`; \} \| \{ `error`:
`"INVALID_CREDENTIALS"`; \} \| \{ `error`: `"RATE_LIMITED"`; `retryAfterMs`:
`number`; \} \| \{ `error`: `"USER_NOT_FOUND"`; \}; \}\> \|
`UnexpectedFailure`\>

---

### useSignUpWithUsernamePassword()

```ts
function useSignUpWithUsernamePassword(api): object;
```

Client for the sign-up flow of the `usernamePassword` method: wire the backend's
`signUpWithUsernamePassword` mutation to the core client.

The returned `signUp` runs the mutation with the given credentials and, on
success, establishes an authenticated session with your Convex backend.

The `pending` flag returned will let you know if the credentials are currently
being validated.

After calling `signUp`, check the `status` field on the return value to see
whether the sign-up was successful or if you need to handle an error.

```tsx
import { useSignUpWithUsernamePassword } from "@convex-dev/auth/methods/username-password/react";
import { api } from "../convex/_generated/api";

function SignUp() {
  const { signUp, pending } = useSignUpWithUsernamePassword(api.auth);
  // ...same shape as useSignInWithUsernamePassword
}
```

#### Parameters

| Parameter                        | Type                                                                      | Description                                      |
| -------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------ |
| `api`                            | \{ `signUpWithUsernamePassword`: `SignUpWithUsernamePasswordMutation`; \} | The functions of the method, usually `api.auth`. |
| `api.signUpWithUsernamePassword` | `SignUpWithUsernamePasswordMutation`                                      | -                                                |

#### Returns

##### pending

```ts
pending: boolean;
```

`true` if the sign-up attempt is being validated.

##### signUp

```ts
signUp: (credentials) =>
  (Promise<
    | UnexpectedFailure
    | ClientView<
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
  > = run);
```

Passes up the given credentials to perform a username/password sign up.

Returns an object with a `status` field.

If it is `"complete"` the sign-up was successful and the client will establish
an authenticated session with the Convex backend server.

If it is `"error"` the returned object will have a `userError` field with
additional details about why sign-up failed.

###### Parameters

| Parameter     | Type                          |
| ------------- | ----------------------------- |
| `credentials` | [`Credentials`](#credentials) |

###### Returns

`Promise`\< \| `UnexpectedFailure` \|
[`ClientView`](../../../ssr/index.md#clientview)\< \| \{ `status`: `"complete"`;
`tokens`: \{ `accessToken`: `string`; `refreshToken`: `string`;
`refreshTokenExpiresAt`: `number`; `userId`: `string`; \}; \} \| \{ `status`:
`"error"`; `userError`: \| \{ `error`: `"PASSWORD_TOO_SHORT"`; `minimumLength`:
`number`; \} \| \{ `error`: `"PASSWORD_TOO_LONG"`; `maximumLength`: `number`; \}
\| \{ `error`: `"PASSWORD_HAS_SURROUNDING_WHITESPACE"`; \} \| \{ `error`:
`"PASSWORD_TOO_COMMON"`; \} \| \{ `error`: `"USERNAME_TOO_SHORT"`;
`minimumLength`: `number`; \} \| \{ `error`: `"USERNAME_TOO_LONG"`;
`maximumLength`: `number`; \} \| \{ `error`:
`"USERNAME_HAS_SURROUNDING_WHITESPACE"`; \} \| \{ `error`:
`"USERNAME_HAS_INVALID_CHARACTERS"`; \} \| \{ `error`: `"USERNAME_TAKEN"`; \};
\}\>\>
