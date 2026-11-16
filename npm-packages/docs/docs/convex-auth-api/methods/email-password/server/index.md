---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/email-password/server
sidebar_label: server
---

import Link from "@docusaurus/Link";

The `emailPassword` method: accounts have an email address and a password, and
the email must be validated before the first sign-in.

## Type Aliases

### ChangePasswordResult

```ts
type ChangePasswordResult = Infer<typeof changePasswordResult>;
```

The result of `changePassword`.

`INVALID_CREDENTIALS` and `RATE_LIMITED` are about the current password. The
other errors are about the new password.

---

### CheckPasswordRecoveryResult

```ts
type CheckPasswordRecoveryResult = Infer<typeof checkPasswordRecoveryResult>;
```

The result of `checkPasswordRecovery`.

---

### CompleteChangeEmailResult

```ts
type CompleteChangeEmailResult = Infer<typeof completeChangeEmailResult>;
```

The result of `completeChangeEmail`.

---

### CompletePasswordRecoveryResult

```ts
type CompletePasswordRecoveryResult = Infer<
  typeof completePasswordRecoveryResult
>;
```

The result of `completePasswordRecovery`: the minted session tokens, or an
error.

---

### CompleteSignUpResult

```ts
type CompleteSignUpResult = Infer<typeof completeSignUpResult>;
```

The result of `completeSignUpWithEmailPassword`: the minted session tokens, or
an error.

---

### EmailPasswordOptions

```ts
type EmailPasswordOptions = object;
```

Options for [setupEmailPassword](#setupemailpassword).

#### Properties

| Property                                                     | Type                                        | Description                                                                                                                                       |
| ------------------------------------------------------------ | ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-component" /> `component`                 | `ComponentApi`                              | The mounted email component (`components.authEmail`). The method uses it to track verified emails and to run the challenges.                      |
| <Link id="property-emailsender" /> `emailSender`             | [`EmailSenderOptions`](#emailsenderoptions) | How the method sends emails.                                                                                                                      |
| <Link id="property-passwordcomponent" /> `passwordComponent` | `PasswordComponentApi`                      | The mounted password component (`components.authPassword`). The method drives its `_internal_dangerous_setPassword` / `verifyPassword` mutations. |
| <Link id="property-urls" /> `urls`                           | [`EmailPasswordUrls`](#emailpasswordurls)   | The landing pages the emailed links point at.                                                                                                     |

---

### EmailPasswordProfile

```ts
type EmailPasswordProfile = Record<string, never>;
```

---

### EmailPasswordUrls

```ts
type EmailPasswordUrls = object;
```

The landing pages the emailed links point at. The link's `code` query parameter
will be appended to these URLs.

#### Properties

| Property                                         | Type     | Description                                       |
| ------------------------------------------------ | -------- | ------------------------------------------------- |
| <Link id="property-changeemail" /> `changeEmail` | `string` | Landing page for the change-email challenge link. |
| <Link id="property-recovery" /> `recovery`       | `string` | Landing page for the password-recovery link.      |
| <Link id="property-signup" /> `signUp`           | `string` | Landing page for the sign-up challenge link.      |

---

### EmailSenderOptions

```ts
type EmailSenderOptions = object;
```

How the method sends emails: challenge links (through the email component) and
security notifications (directly).

Only Resend is supported for now, through the `@convex-dev/resend` component.
Mount that component in the app and pass its `sendEmail` reference and your
Resend API key here.

TODO: support other email providers. TODO: offer a first-party
zero-configuration email service. TODO: let applications customize the email
templates.

#### Properties

| Property                                     | Type           | Description                                                                                                                                  |
| -------------------------------------------- | -------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-apikey" /> `apiKey`       | `string`       | The Resend API key. Read it from an environment variable of the deployment (e.g. `env.RESEND_API_KEY`) rather than writing it in the source. |
| <Link id="property-from" /> `from`           | `string`       | The From address, e.g. `"My App <auth@example.com>"`. The address must be on a domain you verified with Resend.                              |
| <Link id="property-kind" /> `kind`           | `"resend"`     | -                                                                                                                                            |
| <Link id="property-sendemail" /> `sendEmail` | `SendEmailRef` | The mounted Resend component's `lib.sendEmail` reference.                                                                                    |

---

### SignInResult

```ts
type SignInResult = Infer<typeof signInResult>;
```

The result of `signInWithEmailPassword`: the minted session tokens, or an error.

---

### SignUpResult

```ts
type SignUpResult = Infer<typeof signUpResult>;
```

The result of `signUpWithEmailPassword`. On success, the secret the browser must
keep for `completeSignUpWithEmailPassword`; the session arrives only after the
email is validated.

---

### StartChangeEmailResult

```ts
type StartChangeEmailResult = Infer<typeof startChangeEmailResult>;
```

The result of `startChangeEmail`.

---

### StartPasswordRecoveryResult

```ts
type StartPasswordRecoveryResult = Infer<typeof startPasswordRecoveryResult>;
```

The result of `startPasswordRecovery`.

## Functions

### setupEmailPassword()

```ts
function setupEmailPassword<UsersTable>(core, options): object;
```

A password sign-in method where accounts have a validated email address and a
password.

- Sign-up creates the user immediately but without a session; the user signs in
  only after they open the challenge link. Wire it up in `convex/auth.ts`:

```ts
import { components, internal } from "./_generated/api";
import { env } from "./_generated/server";
import { setupConvexAuth } from "@convex-dev/auth/server";
import { setupEmailPassword } from "@convex-dev/auth/methods/email-password/server";

// The frontend origin the emailed links point at.
const SITE_URL = env.SITE_URL ?? "http://localhost:5173";

const auth = setupConvexAuth({ component: components.auth });
export const { signOut, refreshSession, isAuthenticated } = auth;

export const {
  signUpWithEmailPassword,
  completeSignUpWithEmailPassword,
  signInWithEmailPassword,
  changePassword,
  startPasswordRecovery,
  checkPasswordRecovery,
  completePasswordRecovery,
  startChangeEmail,
  completeChangeEmail,
} = setupEmailPassword(auth, {
  component: components.authEmail,
  passwordComponent: components.authPassword,
  emailSender: {
    kind: "resend",
    sendEmail: components.resend.lib.sendEmail,
    apiKey: env.RESEND_API_KEY,
    from: "My App <auth@example.com>",
  },
  urls: {
    signUp: `${SITE_URL}/validate-email`,
    changeEmail: `${SITE_URL}/confirm-email-change`,
    recovery: `${SITE_URL}/reset-password`,
  },
}).attachUserCallbacks({
  createUser: internal.users.createUserEmailPassword,
});
```

- Sign-in accepts any verified email of the account.
- Change-password and change-email require the session _and_ the current
  password (OWASP ASVS v5 6.2.3), and send a security notification to the
  affected address (ASVS 6.3.7).
- Recovery proves ownership of a verified email through a 10-minute link, then
  sets the new password and signs the user in.

Account resolution (email → app user id) is owned by the email component; the
password component stores only `{ userId, passwordHash }`.

#### Type Parameters

| Type Parameter                  |
| ------------------------------- |
| `UsersTable` _extends_ `string` |

#### Parameters

| Parameter | Type                                                            |
| --------- | --------------------------------------------------------------- |
| `core`    | [`AuthCore`](../../../server/index.md#authcore)\<`UsersTable`\> |
| `options` | [`EmailPasswordOptions`](#emailpasswordoptions)                 |

#### Returns

##### attachUserCallbacks()

```ts
attachUserCallbacks(__namedParameters): object;
```

Supply the app's user callbacks (see UserCallbacks for how their args must be
declared) and get the functions of this method to export.

###### Parameters

| Parameter           | Type                                                                                                |
| ------------------- | --------------------------------------------------------------------------------------------------- |
| `__namedParameters` | `UserCallbacks`\<`"emailPassword"`, [`EmailPasswordProfile`](#emailpasswordprofile), `UsersTable`\> |

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

Change the signed-in user's password. Requires the session _and_ the current
password (OWASP ASVS v5 6.2.3), and notifies the primary email address (ASVS
6.3.7). Signs out the user's other sessions if `signOutOtherSessions` is `true`
(ASVS v5 7.4.3).

###### checkPasswordRecovery

```ts
checkPasswordRecovery: RegisteredQuery<
  "public",
  {
    browserSecret: string;
    emailCode: string;
  },
  Promise<
    | {
        success: true;
      }
    | {
        success: false;
        userError:
          | {
              error: "INVALID_CHALLENGE";
            }
          | {
              error: "INCORRECT_CODE";
            };
      }
  >
>;
```

Check a password recovery link before the user types a new password: tell
whether `completePasswordRecovery` would accept the link code + browser secret.
It does not consume the link.

###### completeChangeEmail

```ts
completeChangeEmail: RegisteredMutation<
  "public",
  {
    browserSecret: string;
    emailCode: string;
  },
  Promise<
    | {
        success: true;
      }
    | {
        success: false;
        userError:
          | {
              error: "EMAIL_TAKEN";
            }
          | {
              error: "INVALID_CHALLENGE";
            }
          | {
              error: "INCORRECT_CODE";
            }
          | {
              error: "NOT_LOGGED_IN";
            }
          | {
              error: "WRONG_USER";
            };
      }
  >
>;
```

Complete an email change. The old primary address is removed and replaced with
the new address. This also notifies old address (OWASP ASVS 6.3.7).

No session is minted — the user already has one.

###### completePasswordRecovery

```ts
completePasswordRecovery: RegisteredMutation<
  "public",
  {
    browserSecret: string;
    emailCode: string;
    newPassword: string;
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
              error: "INVALID_CHALLENGE";
            }
          | {
              error: "INCORRECT_CODE";
            };
      }
  >
>;
```

Complete a password recovery: the link code + browser secret prove ownership of
the email, so set the new password, sign out every existing session of the user,
and sign the user in, in one transaction. Notifies the primary email (ASVS
6.3.7).

###### completeSignUpWithEmailPassword

```ts
completeSignUpWithEmailPassword: RegisteredMutation<
  "public",
  {
    browserSecret: string;
    emailCode: string;
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
              error: "EMAIL_TAKEN";
            }
          | {
              error: "INVALID_CHALLENGE";
            }
          | {
              error: "INCORRECT_CODE";
            };
      }
  >
>;
```

Complete a sign-up: validate the email with the code from the link and the
secret from the starting browser, then sign the user in. Validation and sign-in
happen in one transaction.

###### signInWithEmailPassword

```ts
signInWithEmailPassword: RegisteredMutation<
  "public",
  {
    email: string;
    password: string;
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

Verify an existing account's password and, on success, mint a session. Any
verified email of the account works. Returns `USER_NOT_FOUND` when no account
has verified the email and `INVALID_CREDENTIALS` when the password is wrong.
(Address existence is already observable via sign-up's `EMAIL_TAKEN`, so
distinguishing them here leaks nothing new.) The lookup of the address has a
per-IP limit, and returns `RATE_LIMITED` when the client has done too many
lookups.

###### signUpWithEmailPassword

```ts
signUpWithEmailPassword: RegisteredMutation<
  "public",
  {
    email: string;
    password: string;
  },
  Promise<
    | {
        browserSecret: string;
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
              error: "INVALID_EMAIL";
            }
          | {
              error: "RATE_LIMITED";
              retryAfterMs: number;
            }
          | {
              error: "EMAIL_TAKEN";
            };
      }
  >
>;
```

Create the app user and password for a new account, without a session. The user
signs in for the first time through `completeSignUpWithEmailPassword`, after
opening the challenge link.

Repeated sign-ups with one unvalidated address create separate users; the first
completed validation wins, and the others can never sign in.

###### startChangeEmail

```ts
startChangeEmail: RegisteredMutation<
  "public",
  {
    currentPassword: string;
    newEmail: string;
  },
  Promise<
    | {
        browserSecret: string;
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
              error: "INVALID_CREDENTIALS";
            }
          | {
              error: "RATE_LIMITED";
              retryAfterMs: number;
            }
          | {
              error: "INVALID_EMAIL";
            }
          | {
              error: "RATE_LIMITED";
              retryAfterMs: number;
            }
          | {
              error: "EMAIL_TAKEN";
            }
          | {
              error: "NOT_LOGGED_IN";
            };
      }
  >
>;
```

Start changing the signed-in user's primary email address. The new address
replaces the old one: after the change, the account no longer has the old
address. Requires the session _and_ the current password (OWASP ASVS v5 6.2.3).
Sends a challenge link to the new address; the change happens in
`completeChangeEmail`.

###### startPasswordRecovery

```ts
startPasswordRecovery: RegisteredMutation<
  "public",
  {
    email: string;
  },
  Promise<
    | {
        browserSecret: string;
        sentTo: string;
        success: true;
      }
    | {
        success: false;
        userError:
          | {
              error: "INVALID_EMAIL";
            }
          | {
              error: "RATE_LIMITED";
              retryAfterMs: number;
            }
          | {
              error: "EMAIL_NOT_FOUND";
            };
      }
  >
>;
```

Start a password recovery: send a reset link (valid 10 minutes) to a verified
email address. Any verified address of the account works, not only the primary
one.

`EMAIL_NOT_FOUND` is surfaced to the caller. This reveals whether an address has
an account, which sign-up's `EMAIL_TAKEN` reveals anyway; the recipe accepts
that trade-off for a clearer flow. The lookup has a per-IP limit, so a client
cannot probe many addresses.
