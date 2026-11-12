---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/username-passkey/react
sidebar_label: react
---

import Link from "@docusaurus/Link";

React client for the `usernamePasskey` method, exported at
`@convex-dev/auth/methods/username-passkey/react`.

[useSignInWithUsernamePasskey](#usesigninwithusernamepasskey) is the
batteries-included hook for the username + passkey login form. It drives the
browser-side WebAuthn ceremonies (through the internal client module built on
`@simplewebauthn/browser`) against the mutations of a passkey recipe, and it
handles both the identifier-first modal flow and passkey autofill (conditional
mediation), where the user selects an account directly in the autocompletion
list.

[useAddPasskey](#useaddpasskey) and [useRemovePasskey](#useremovepasskey) (from
`management/react.tsx`) drive the passkey settings page of a signed-in user.

## Interfaces

- [AuthenticationResponseJSON](interfaces/AuthenticationResponseJSON.md)
- [PublicKeyCredentialCreationOptionsJSON](interfaces/PublicKeyCredentialCreationOptionsJSON.md)
- [PublicKeyCredentialRequestOptionsJSON](interfaces/PublicKeyCredentialRequestOptionsJSON.md)
- [RegistrationResponseJSON](interfaces/RegistrationResponseJSON.md)

## Type Aliases

### AddPasskeyApi

```ts
type AddPasskeyApi = object;
```

#### Properties

| Property                                                   | Type                       |
| ---------------------------------------------------------- | -------------------------- |
| <Link id="property-finishaddpasskey" /> `finishAddPasskey` | `FinishAddPasskeyMutation` |
| <Link id="property-startaddpasskey" /> `startAddPasskey`   | `StartAddPasskeyMutation`  |
| <Link id="property-verifyaddpasskey" /> `verifyAddPasskey` | `VerifyAddPasskeyMutation` |

---

### AddPasskeyResult

```ts
type AddPasskeyResult = AddPasskeyFlowResult | AlreadyPendingFailure;
```

---

### AlreadyPendingFailure

```ts
type AlreadyPendingFailure = object;
```

Failure shape for AlreadyPendingError.

#### Properties

| Property                                     | Type                  |
| -------------------------------------------- | --------------------- |
| <Link id="property-success" /> `success`     | `false`               |
| <Link id="property-usererror" /> `userError` | `AlreadyPendingError` |

---

### PasskeyAutofillStatus

```ts
type PasskeyAutofillStatus =
  | "idle"
  | "waiting"
  | "signingIn"
  | "signedIn"
  | "stopped";
```

The states of usePasskeyAutofill:

- `"idle"`: the flow has not started yet, or autofill is off.
- `"waiting"`: the autofill request is pending in the browser.
- `"signingIn"`: a picked assertion is being handled (`onAssertion` is running,
  usually a server verification).
- `"signedIn"`: `onAssertion` reported success and the flow is done.
- `"stopped"`: the flow is done without a success (failure, or no browser
  support).

---

### PasskeyClientError

```ts
type PasskeyClientError =
  | {
      error: "CEREMONY_ABORTED";
    }
  | {
      error: "PASSKEY_ALREADY_REGISTERED";
    }
  | {
      error: "WEBAUTHN_UNSUPPORTED";
    }
  | {
      cause: unknown;
      error: "OTHER_ERROR";
    };
```

The failures the client side produces (the server never returns these):

- `CEREMONY_ABORTED`: the user dismissed the passkey dialog, the platform
  refused the ceremony (`NotAllowedError`), or another ceremony displaced this
  one. This is the most common failure; show a calm "sign-in was cancelled"
  message.
- `PASSKEY_ALREADY_REGISTERED`: the authenticator refused a registration because
  it already holds a passkey for this account (`InvalidStateError`, usually via
  `excludeCredentials`). Tell the user they can sign in with the passkey they
  already have.
- `WEBAUTHN_UNSUPPORTED`: this runtime cannot run passkey ceremonies.
- `OTHER_ERROR`: everything else thrown (a network blip, a bug, an unexpected
  server error). The thrown value is preserved on `cause` for callers that want
  to inspect or log it.

---

### PasskeyClientFailure

```ts
type PasskeyClientFailure = object;
```

A failed result with a client-side `userError`. The failure arm all the ceremony
functions and hooks share; it composes with the server's own
`{ success: false, userError }` results into one `userError` switch.

#### Properties

| Property                                       | Type                                        |
| ---------------------------------------------- | ------------------------------------------- |
| <Link id="property-success-1" /> `success`     | `false`                                     |
| <Link id="property-usererror-1" /> `userError` | [`PasskeyClientError`](#passkeyclienterror) |

---

### RemovePasskeyApi

```ts
type RemovePasskeyApi = object;
```

#### Properties

| Property                                                         | Type                          |
| ---------------------------------------------------------------- | ----------------------------- |
| <Link id="property-finishremovepasskey" /> `finishRemovePasskey` | `FinishRemovePasskeyMutation` |
| <Link id="property-startremovepasskey" /> `startRemovePasskey`   | `StartRemovePasskeyMutation`  |

---

### RemovePasskeyResult

```ts
type RemovePasskeyResult = RemovePasskeyFlowResult | AlreadyPendingFailure;
```

---

### RenamePasskeyResult

```ts
type RenamePasskeyResult = Infer<typeof renamePasskeyResult>;
```

---

### SignInWithUsernamePasskeyResult

```ts
type SignInWithUsernamePasskeyResult =
  | SignInFlowResult
  | SignInError<PasskeyClientError | AlreadyPendingError>;
```

The result of the `signIn` callback from `useSignInWithUsernamePasskey`.

A completed sign-in carries a `flow` discriminant: `"signUp"` when the ceremony
created a new account, `"signIn"` when it authenticated an existing one.
`ALREADY_PENDING` comes back when a `signIn` call runs while the previous one
still does, or while an autofill sign-in is verified.

---

### UsernamePasskeyApi

```ts
type UsernamePasskeyApi = object;
```

The mutation references the sign-in flows drive.

#### Properties

| Property                                                                                               | Type                          |
| ------------------------------------------------------------------------------------------------------ | ----------------------------- |
| <Link id="property-finishsigninwithusernamepasskey" /> `finishSignInWithUsernamePasskey`               | `FinishSignInMutation`        |
| <Link id="property-finishsignupwithusernamepasskey" /> `finishSignUpWithUsernamePasskey`               | `FinishSignUpMutation`        |
| <Link id="property-startautofillsigninwithusernamepasskey" /> `startAutofillSignInWithUsernamePasskey` | `StartAutofillSignInMutation` |
| <Link id="property-startsigninwithusernamepasskey" /> `startSignInWithUsernamePasskey`                 | `StartSignInMutation`         |

---

### UsernamePasskeyAutofillError

```ts
type UsernamePasskeyAutofillError =
  | Extract<
      FinishSignInResult,
      {
        status: "error";
      }
    >["userError"]
  | PasskeyClientError;
```

The errors the autofill sign-in flow reports.

---

### WireAuthenticationResponse

```ts
type WireAuthenticationResponse = Infer<typeof vAuthenticationResponseJSON>;
```

---

### WireCreationOptions

```ts
type WireCreationOptions = Infer<
  typeof vPublicKeyCredentialCreationOptionsJSON
>;
```

---

### WireRegistrationResponse

```ts
type WireRegistrationResponse = Infer<typeof vRegistrationResponseJSON>;
```

---

### WireRequestOptions

```ts
type WireRequestOptions = Infer<typeof vPublicKeyCredentialRequestOptionsJSON>;
```

## Functions

### useAddPasskey()

```ts
function useAddPasskey(managementApi): object;
```

Hook for the "Add a passkey" button of a settings page.

```tsx
import { useAddPasskey } from "@convex-dev/auth/methods/username-passkey/react";
import { api } from "../convex/_generated/api";

function AddPasskeyButton() {
  const { addPasskey, pending } = useAddPasskey(api.auth);
  return (
    <button
      disabled={pending}
      onClick={async () => {
        const result = await addPasskey();
        if (!result.success) {
          // map result.userError to a message
        }
      }}
    >
      Add a passkey
    </button>
  );
}
```

First asks the user to authenticate with an existing passkey to prove their
identity, then registers a new passkey.

#### Parameters

| Parameter       | Type                              | Description                                                                                            |
| --------------- | --------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `managementApi` | [`AddPasskeyApi`](#addpasskeyapi) | The app module that re-exports the passkey-management functions of the method, for example `api.auth`. |

#### Returns

##### addPasskey

```ts
addPasskey: () => Promise<AddPasskeyResult>;
```

Adds a passkey to the account of the signed-in user.

Returns an object with a `success` boolean flag. If it is `false`, the object
has a `userError` field that tells why.

###### Returns

`Promise`\<[`AddPasskeyResult`](#addpasskeyresult)\>

##### pending

```ts
pending: boolean;
```

`true` while an `addPasskey` call is running.

---

### useRemovePasskey()

```ts
function useRemovePasskey(managementApi): object;
```

Hook for the "Remove" button of a passkey list.

```tsx
import { useRemovePasskey } from "@convex-dev/auth/methods/username-passkey/react";
import { api } from "../convex/_generated/api";

function RemovePasskeyButton({ passkeyId }: { passkeyId: string }) {
  const { removePasskey, pending } = useRemovePasskey(api.auth);
  return (
    <button
      disabled={pending}
      onClick={async () => {
        const result = await removePasskey(passkeyId);
        if (!result.success) {
          // map result.userError to a message
        }
      }}
    >
      Remove
    </button>
  );
}
```

This asks the user to reauthenticate with another passkey before deleting the
targeted passkey.

#### Parameters

| Parameter       | Type                                    | Description                                                                                            |
| --------------- | --------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `managementApi` | [`RemovePasskeyApi`](#removepasskeyapi) | The app module that re-exports the passkey-management functions of the method, for example `api.auth`. |

#### Returns

##### pending

```ts
pending: boolean;
```

`true` while a `removePasskey` call is running.

##### removePasskey

```ts
removePasskey: (passkeyId) => Promise<RemovePasskeyResult>;
```

Removes the given passkey from the account of the signed-in user.

Returns an object with a `success` boolean flag. If it is `false`, the object
has a `userError` field that tells why.

###### Parameters

| Parameter   | Type     |
| ----------- | -------- |
| `passkeyId` | `string` |

###### Returns

`Promise`\<[`RemovePasskeyResult`](#removepasskeyresult)\>

---

### useSignInWithUsernamePasskey()

```ts
function useSignInWithUsernamePasskey(usernamePasskeyApi): object;
```

Client for the log in page in the “username + passkey” auth flow.

This client handles both:

- Manually logging in by providing an identifier (e.g. username) then
  registering a new passkey or logging in
- Passkey autofill (conditional mediation) where the user selects an account
  directly in the autocompletion list.

```tsx
import { useSignInWithUsernamePasskey } from "@convex-dev/auth/methods/username-passkey/react";
import { useState } from "react";
import { api } from "../convex/_generated/api";

function LogIn() {
  // While this hook is mounted, the browser also shows the stored passkeys
  // in the autocompletion list of the username field.
  const { signIn, pending, autofill } = useSignInWithUsernamePasskey(api.auth);
  const [username, setUsername] = useState("");
  const [error, setError] = useState<string | null>(null);

  // The alerts show the error codes. Map each code to a message for the user.
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        setError(null);
        const result = await signIn({ username });
        if (result.status === "error") {
          setError(result.userError.error);
        }
      }}
    >
      <input
        type="text"
        value={username}
        onChange={(e) => setUsername(e.target.value)}
        autoComplete="username webauthn"
        required
        disabled={pending}
      />
      {error !== null && <p role="alert">{error}</p>}
      {autofill.lastError !== null && (
        <p role="alert">{autofill.lastError.error}</p>
      )}
      <button type="submit" disabled={pending}>
        {pending ? "Waiting for your passkey…" : "Continue with a passkey"}
      </button>
    </form>
  );
}
```

#### Parameters

| Parameter            | Type                                        | Description                                      |
| -------------------- | ------------------------------------------- | ------------------------------------------------ |
| `usernamePasskeyApi` | [`UsernamePasskeyApi`](#usernamepasskeyapi) | The functions of the method, usually `api.auth`. |

#### Returns

##### autofill

```ts
autofill: object;
```

The state of the autofill flow:

- `available`: whether the platform can offer passkeys in the autocompletion
  list; `null` while the detection runs.
- `status`: the state of the request; see
  [PasskeyAutofillStatus](#passkeyautofillstatus).
- `lastError`: the most recent failure, for display or logging.

###### autofill.available

```ts
available: boolean | null;
```

###### autofill.lastError

```ts
lastError:
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
  error: "CEREMONY_ABORTED";
}
  | {
  error: "PASSKEY_ALREADY_REGISTERED";
}
  | {
  error: "WEBAUTHN_UNSUPPORTED";
}
  | {
  cause: unknown;
  error: "OTHER_ERROR";
}
  | null;
```

###### autofill.status

```ts
status: PasskeyAutofillStatus;
```

##### pending

```ts
pending: boolean;
```

`true` while a sign-in runs: a `signIn` call, or an autofill sign-in that the
server verifies (`autofill.status` is `"signingIn"`). A `signIn` call in this
time returns `ALREADY_PENDING`.

##### signIn

```ts
signIn: (__namedParameters) => Promise<SignInWithUsernamePasskeyResult>;
```

Runs the identifier-first passkey flow for the given username.

Returns an object with a `status` field.

If it is `"complete"` the sign-in (or the account creation) was successful and
the client will establish an authenticated session with the Convex backend
server. The `flow` field tells which one it was: `"signUp"` created a new
account, `"signIn"` authenticated an existing one.

If it is `"error"` the returned object will have a `userError` field with
additional details about why sign-in failed.

###### Parameters

| Parameter                    | Type                        |
| ---------------------------- | --------------------------- |
| `__namedParameters`          | \{ `username`: `string`; \} |
| `__namedParameters.username` | `string`                    |

###### Returns

`Promise`\<[`SignInWithUsernamePasskeyResult`](#signinwithusernamepasskeyresult)\>
