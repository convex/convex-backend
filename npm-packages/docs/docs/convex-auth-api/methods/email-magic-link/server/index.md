---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/email-magic-link/server
sidebar_label: server
---

import Link from "@docusaurus/Link";

The `emailMagicLink` method: a person signs in with a link that they receive by
email. There is no password, and one form serves both sign-up and sign-in.

## Type Aliases

### EmailMagicLinkOptions

```ts
type EmailMagicLinkOptions = object;
```

Options for [setupEmailMagicLink](#setupemailmagiclink).

#### Properties

| Property                                         | Type                                        | Description                                                                                                                  |
| ------------------------------------------------ | ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-component" /> `component`     | `ComponentApi`                              | The mounted email component (`components.authEmail`). The method uses it to track verified emails and to run the challenges. |
| <Link id="property-emailsender" /> `emailSender` | [`EmailSenderOptions`](#emailsenderoptions) | How the method sends emails.                                                                                                 |
| <Link id="property-url" /> `url`                 | `string`                                    | The landing page that the emailed link points at. The link's `code` query parameter will be appended to this URL.            |

---

### EmailMagicLinkProfile

```ts
type EmailMagicLinkProfile = Record<string, never>;
```

---

### EmailSenderOptions

```ts
type EmailSenderOptions = object;
```

How the method sends the sign-in emails.

Only Resend is supported for now, through the `@convex-dev/resend` component.
Mount that component in the app and pass its `sendEmail` reference and your
Resend API key here.

#### Properties

| Property                                     | Type           | Description                                                                                                                                  |
| -------------------------------------------- | -------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-apikey" /> `apiKey`       | `string`       | The Resend API key. Read it from an environment variable of the deployment (e.g. `env.RESEND_API_KEY`) rather than writing it in the source. |
| <Link id="property-from" /> `from`           | `string`       | The From address, e.g. `"My App <auth@example.com>"`. The address must be on a domain you verified with Resend.                              |
| <Link id="property-kind" /> `kind`           | `"resend"`     | -                                                                                                                                            |
| <Link id="property-sendemail" /> `sendEmail` | `SendEmailRef` | The mounted Resend component's `lib.sendEmail` reference.                                                                                    |

---

### SendResult

```ts
type SendResult = Infer<typeof sendResult>;
```

The result of `sendEmailMagicLink`. On success, the secret the browser must keep
for `signInWithEmailMagicLink`. The result does not tell whether the address has
an account.

---

### SignInResult

```ts
type SignInResult = Infer<typeof signInResult>;
```

The result of `signInWithEmailMagicLink`: the minted session tokens, or an
error.

## Functions

### setupEmailMagicLink()

```ts
function setupEmailMagicLink<UsersTable>(core, options): object;
```

A sign-in method where a person signs in with a link received by email. One form
serves both a person who has an account and a person who has none. Wire it up in
`convex/auth.ts`:

```ts
import { components, internal } from "./_generated/api";
import { env } from "./_generated/server";
import { setupConvexAuth } from "@convex-dev/auth/server";
import { setupEmailMagicLink } from "@convex-dev/auth/methods/email-magic-link/server";

// The frontend origin the emailed links point at.
const SITE_URL = env.SITE_URL ?? "http://localhost:5173";

const auth = setupConvexAuth({ component: components.auth });
export const { signOut, refreshSession, isAuthenticated } = auth;

export const { sendEmailMagicLink, signInWithEmailMagicLink } =
  setupEmailMagicLink(auth, {
    component: components.authEmail,
    emailSender: {
      kind: "resend",
      sendEmail: components.resend.lib.sendEmail,
      apiKey: env.RESEND_API_KEY,
      from: "My App <auth@example.com>",
    },
    url: `${SITE_URL}/sign-in-with-link`,
  }).attachUserCallbacks({
    createUser: internal.users.createUserEmailMagicLink,
  });
```

- `sendEmailMagicLink` sends the link. It creates no user, and it gives the same
  result whether the address has an account or not.
- `signInWithEmailMagicLink` signs in the user that has verified the address.
  When no user has, it creates one with the app's `createUser`, and records the
  address as its primary email address.
- A link works once, for 10 minutes, and only in the browser that asked for it.
  Opening the link does not use it: the browser must also send its secret. Thus
  a mail scanner that opens the link cannot use it or make it fail.

#### Type Parameters

| Type Parameter                  |
| ------------------------------- |
| `UsersTable` _extends_ `string` |

#### Parameters

| Parameter | Type                                                            |
| --------- | --------------------------------------------------------------- |
| `core`    | [`AuthCore`](../../../server/index.md#authcore)\<`UsersTable`\> |
| `options` | [`EmailMagicLinkOptions`](#emailmagiclinkoptions)               |

#### Returns

##### attachUserCallbacks()

```ts
attachUserCallbacks(__namedParameters): object;
```

Supply the app's user callbacks (see UserCallbacks for how their args must be
declared) and get the functions of this method to export.

###### Parameters

| Parameter           | Type                                                                                                   |
| ------------------- | ------------------------------------------------------------------------------------------------------ |
| `__namedParameters` | `UserCallbacks`\<`"emailMagicLink"`, [`EmailMagicLinkProfile`](#emailmagiclinkprofile), `UsersTable`\> |

###### Returns

###### sendEmailMagicLink

```ts
sendEmailMagicLink: RegisteredMutation<
  "public",
  {
    email: string;
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
              error: "INVALID_EMAIL";
            }
          | {
              error: "RATE_LIMITED";
              retryAfterMs: number;
            };
      }
  >
>;
```

Send a sign-in link (valid 10 minutes) to the address. Creates nothing but the
challenge: no user exists for a new address until the person opens the link.

The result is the same whether or not a user has verified the address. It fails
only for a malformed address, or when the address or the client has asked for
too many emails.

###### signInWithEmailMagicLink

```ts
signInWithEmailMagicLink: RegisteredMutation<
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
              error: "INVALID_CHALLENGE";
            }
          | {
              error: "INCORRECT_CODE";
            };
      }
  >
>;
```

Sign in with the code from the link and the secret from the starting browser.
The user that has verified the address signs in. When no user has, the app's
`createUser` creates one, and the address becomes its primary email address.
Everything happens in one transaction.
