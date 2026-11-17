---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/username-passkey/server
sidebar_label: server
---

import Link from "@docusaurus/Link";

## Type Aliases

### FinishSignInResult

```ts
type FinishSignInResult = Infer<typeof finishSignInResult>;
```

The result of `finishSignInWithUsernamePasskey`.

When complete the minted session tokens and the username of the account,
otherwise a user-facing `userError`.

---

### FinishSignUpResult

```ts
type FinishSignUpResult = Infer<typeof finishSignUpResult>;
```

The result of `finishSignUpWithUsernamePasskey`.

When complete the minted session tokens, otherwise a user-facing `userError`.

---

### StartAutofillSignInResult

```ts
type StartAutofillSignInResult = Infer<typeof startAutofillSignInResult>;
```

The result of `startAutofillSignInWithUsernamePasskey`: an unbound challenge for
a conditional-mediation (passkey autofill) `navigator.credentials.get()` call.

---

### StartSignInResult

```ts
type StartSignInResult = Infer<typeof startSignInResult>;
```

The result of `startSignInWithUsernamePasskey`: which ceremony the client must
run, with the data for the WebAuthn call, or a user-facing `userError`.

---

### UsernamePasskeyConfig

```ts
type UsernamePasskeyConfig = Required<UsernamePasskeyOptions> & object;
```

[UsernamePasskeyOptions](#usernamepasskeyoptions) with the defaults applied,
plus the core component (for `getAuthUserId`). The functions of the method,
including the passkey-management ones, close over this value.

#### Type Declaration

##### coreComponent

```ts
coreComponent: CoreComponentApi;
```

---

### UsernamePasskeyOptions

```ts
type UsernamePasskeyOptions = object;
```

Options for [setupUsernamePasskey](#setupusernamepasskey).

#### Properties

| Property                                                     | Type                   | Description                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------------------------------------------------------ | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| <Link id="property-component" /> `component`                 | `ComponentApi`         | The mounted passkey component (`components.authPasskey`). The recipe drives its registration and authentication ceremonies.                                                                                                                                                                                                                                                                                                                |
| <Link id="property-origin" /> `origin`                       | `string` \| `string`[] | The exact origin of the ceremonies (for example, "https://app.example.com" or "http://localhost:5173"), or a list of the accepted origins. A list lets one deployment serve several clients of the same `rpId`: a web page reports the origin it is served from, an iOS app reports `https://<rpId>`, and an Android app reports `android:apk-key-hash:<hash>`, the unpadded base64url SHA-256 hash of the certificate that signs the app. |
| <Link id="property-rpid" /> `rpId`                           | `string`               | The relying party ID: usually the registrable domain at which the app is served (for example, "example.com" or "localhost"). Only web pages on the same domain (or its subdomains) can use the passkeys. See https://web.dev/articles/webauthn-rp-id                                                                                                                                                                                       |
| <Link id="property-rpname" /> `rpName?`                      | `string`               | The human-readable relying party name that the browser shows in the passkey dialog. WebAuthn requires it on the client. When not set, the `rpId` is used.                                                                                                                                                                                                                                                                                  |
| <Link id="property-usernamecomponent" /> `usernameComponent` | `UsernameComponentApi` | The mounted username component (`components.authUsername`). The recipe uses it to map a username onto the app user id: it stores the username at sign-up and reads it back at sign-in.                                                                                                                                                                                                                                                     |

## Functions

### setupUsernamePasskey()

```ts
function setupUsernamePasskey<UsersTable>(core, options): object;
```

An identifier-first username + passkey recipe: the user types a username; when
the username has an account, the user authenticates with a passkey of that
account; when the username is free, the user registers a new account
immediately.

Logged in users can add or remove passkeys.

Wire it up in `convex/auth.ts`:

```ts
const auth = setupConvexAuth({ component: components.auth });
export const { signOut, refreshSession, isAuthenticated } = auth;

const {
  startSignInWithUsernamePasskey,
  startAutofillSignInWithUsernamePasskey,
  finishSignUpWithUsernamePasskey,
  finishSignInWithUsernamePasskey,

  listPasskeys,
  renamePasskey,

  startAddPasskey,
  verifyAddPasskey,
  finishAddPasskey,

  startRemovePasskey,
  finishRemovePasskey,
} = setupUsernamePasskey(auth, {
  component: components.authPasskey,
  usernameComponent: components.authUsername,
  rpId: "localhost",
  origin: "http://localhost:5173",
}).attachUserCallbacks({ createUser: internal.users.createUserPasskey });
```

#### Type Parameters

| Type Parameter                  |
| ------------------------------- |
| `UsersTable` _extends_ `string` |

#### Parameters

| Parameter | Type                                                            |
| --------- | --------------------------------------------------------------- |
| `core`    | [`AuthCore`](../../../server/index.md#authcore)\<`UsersTable`\> |
| `options` | [`UsernamePasskeyOptions`](#usernamepasskeyoptions)             |

#### Returns

##### attachUserCallbacks()

```ts
attachUserCallbacks(__namedParameters): object;
```

Supply the app's user callbacks (see UserCallbacks for how their args must be
declared) and get the functions of this method to export.

###### Parameters

| Parameter           | Type                                                                                |
| ------------------- | ----------------------------------------------------------------------------------- |
| `__namedParameters` | `UserCallbacks`\<`"usernamePasskey"`, `Record`\<`string`, `never`\>, `UsersTable`\> |

###### Returns

###### finishAddPasskey

```ts
finishAddPasskey: RegisteredMutation<
  "public",
  {
    response: {
      clientExtensionResults: {};
      id: string;
      rawId: string;
      response: {
        attestationObject: string;
        clientDataJSON: string;
        transports?: string[];
      };
      type: "public-key";
    };
  },
  Promise<
    | {
        passkeyId: string;
        success: true;
      }
    | {
        success: false;
        userError:
          | {
              error: "CHALLENGE_EXPIRED";
            }
          | {
              error: "PROTOCOL_ERROR";
            }
          | {
              error: "NOT_LOGGED_IN";
            }
          | {
              error: "TOO_MANY_PASSKEYS";
            };
      }
  >
>;
```

###### finishRemovePasskey

```ts
finishRemovePasskey: RegisteredMutation<
  "public",
  {
    passkeyId: string;
    response: {
      clientExtensionResults: {};
      id: string;
      rawId: string;
      response: {
        authenticatorData: string;
        clientDataJSON: string;
        signature: string;
        userHandle?: string;
      };
      type: "public-key";
    };
  },
  Promise<
    | {
        success: true;
      }
    | {
        success: false;
        userError:
          | {
              error: "PROTOCOL_ERROR";
            }
          | {
              error: "UNKNOWN_CREDENTIAL";
            }
          | {
              error: "CHALLENGE_EXPIRED";
            }
          | {
              error: "NOT_LOGGED_IN";
            }
          | {
              error: "PASSKEY_NOT_FOUND";
            };
      }
  >
>;
```

###### finishSignInWithUsernamePasskey

```ts
finishSignInWithUsernamePasskey: RegisteredMutation<
  "public",
  {
    response: {
      clientExtensionResults: {};
      id: string;
      rawId: string;
      response: {
        authenticatorData: string;
        clientDataJSON: string;
        signature: string;
        userHandle?: string;
      };
      type: "public-key";
    };
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
        username: string | null;
      }
    | {
        status: "error";
        userError:
          | {
              error: "PROTOCOL_ERROR";
            }
          | {
              error: "UNKNOWN_CREDENTIAL";
            }
          | {
              error: "CHALLENGE_EXPIRED";
            };
      }
  >
>;
```

Verify a passkey assertion and, on success, mint a session.

The function serves both ceremonies that produce an assertion: the
username-first branch (`startSignInWithUsernamePasskey` →
`step: "authenticate"`, a challenge bound to the user) and passkey autofill
(`startAutofillSignInWithUsernamePasskey`, an unbound challenge where the
credential identifies the user).

###### finishSignUpWithUsernamePasskey

```ts
finishSignUpWithUsernamePasskey: RegisteredMutation<
  "public",
  {
    response: {
      clientExtensionResults: {};
      id: string;
      rawId: string;
      response: {
        attestationObject: string;
        clientDataJSON: string;
        transports?: string[];
      };
      type: "public-key";
    };
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
            }
          | {
              error: "CHALLENGE_EXPIRED";
            }
          | {
              error: "PROTOCOL_ERROR";
            };
      }
  >
>;
```

Create a new account: the user, the username, the account, the session, and the
passkey, all in ONE mutation. When any step fails, nothing is committed.

The order of the steps gives that guarantee:

1. Checks that write nothing: the username format, the username conflict, and
   `checkRegistrationForNewUser` (the full WebAuthn verification as a query).
   Each failure, correctable or not, returns a `userError` here, before anything
   exists.
2. Writes that cannot fail after the checks: `completeSignUp` mints the user,
   the account, and the session, `setUsername` stores the username, and
   `_internal_dangerous_finishRegistrationForNewUser` stores the passkey. None
   of them can fail after step 1 inside the same transaction, so a failure
   throws and rolls everything back.

###### listPasskeys

```ts
listPasskeys: RegisteredQuery<
  "public",
  {},
  Promise<
    | {
        passkeys: object[];
        success: true;
      }
    | {
        success: false;
        userError: {
          error: "NOT_LOGGED_IN";
        };
      }
  >
>;
```

###### renamePasskey

```ts
renamePasskey: RegisteredMutation<
  "public",
  {
    name: string;
    passkeyId: string;
  },
  Promise<
    | {
        success: true;
      }
    | {
        success: false;
        userError:
          | {
              error: "NOT_LOGGED_IN";
            }
          | {
              error: "PASSKEY_NOT_FOUND";
            }
          | {
              error: "INVALID_NAME";
            };
      }
  >
>;
```

###### startAddPasskey

```ts
startAddPasskey: RegisteredMutation<
  "public",
  {},
  Promise<
    | {
        options: {
          allowCredentials: object[];
          challenge: string;
          rpId: string;
          timeout: number;
          userVerification: "required";
        };
        success: true;
      }
    | {
        success: false;
        userError:
          | {
              error: "NOT_LOGGED_IN";
            }
          | {
              error: "TOO_MANY_PASSKEYS";
            };
      }
  >
>;
```

###### startAutofillSignInWithUsernamePasskey

```ts
startAutofillSignInWithUsernamePasskey: RegisteredMutation<
  "public",
  {},
  Promise<{
    options: {
      allowCredentials: object[];
      challenge: string;
      rpId: string;
      timeout: number;
      userVerification: "required";
    };
  }>
>;
```

Start a conditional-mediation (passkey autofill) ceremony: an authentication
challenge that is not bound to a user. The passkey that the user selects
identifies the account.

###### startRemovePasskey

```ts
startRemovePasskey: RegisteredMutation<
  "public",
  {
    passkeyId: string;
  },
  Promise<
    | {
        options: {
          allowCredentials: object[];
          challenge: string;
          rpId: string;
          timeout: number;
          userVerification: "required";
        };
        success: true;
      }
    | {
        success: false;
        userError:
          | {
              error: "NOT_LOGGED_IN";
            }
          | {
              error: "PASSKEY_NOT_FOUND";
            }
          | {
              error: "LAST_PASSKEY";
            };
      }
  >
>;
```

###### startSignInWithUsernamePasskey

```ts
startSignInWithUsernamePasskey: RegisteredMutation<
  "public",
  {
    username: string;
  },
  Promise<
    | {
        options: {
          allowCredentials: object[];
          challenge: string;
          rpId: string;
          timeout: number;
          userVerification: "required";
        };
        step: "authenticate";
        success: true;
      }
    | {
        options: {
          attestation: "none";
          authenticatorSelection: {
            requireResidentKey: true;
            residentKey: "required";
            userVerification: "required";
          };
          challenge: string;
          excludeCredentials: object[];
          extensions: {};
          pubKeyCredParams: object[];
          rp: {
            id: string;
            name: string;
          };
          timeout: number;
          user: {
            displayName: string;
            id: string;
            name: string;
          };
        };
        step: "register";
        success: true;
      }
    | {
        success: false;
        userError:
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

The identifier-first entry point: look the username up and start the matching
ceremony.

- The username has an account: start an authentication ceremony that is bound to
  that user (`step: "authenticate"`).
- The username is free: start a registration ceremony for a new account
  (`step: "register"`). The account is not created here;
  `finishSignUpWithUsernamePasskey` creates everything in one transaction.

Note: this flow lets an attacker enumerate which usernames exist. That is
intrinsic to identifier-first passkey sign-in (and account existence is also
observable through sign-up), so it is accepted.

###### verifyAddPasskey

```ts
verifyAddPasskey: RegisteredMutation<
  "public",
  {
    response: {
      clientExtensionResults: {};
      id: string;
      rawId: string;
      response: {
        authenticatorData: string;
        clientDataJSON: string;
        signature: string;
        userHandle?: string;
      };
      type: "public-key";
    };
  },
  Promise<
    | {
        options: {
          attestation: "none";
          authenticatorSelection: {
            requireResidentKey: true;
            residentKey: "required";
            userVerification: "required";
          };
          challenge: string;
          excludeCredentials: object[];
          extensions: {};
          pubKeyCredParams: object[];
          rp: {
            id: string;
            name: string;
          };
          timeout: number;
          user: {
            displayName: string;
            id: string;
            name: string;
          };
        };
        success: true;
      }
    | {
        success: false;
        userError:
          | {
              error: "PROTOCOL_ERROR";
            }
          | {
              error: "UNKNOWN_CREDENTIAL";
            }
          | {
              error: "CHALLENGE_EXPIRED";
            }
          | {
              error: "NOT_LOGGED_IN";
            };
      }
  >
>;
```
