---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/username-passkey/react-custom
sidebar_label: react-custom
---

import Link from "@docusaurus/Link";

The passkey hooks on ceremonies that the app provides, for example on a React
Native passkey library. Exported at
`@convex-dev/auth/methods/username-passkey/react-custom`.

## Type Aliases

### PasskeyAuthenticationResponse

```ts
type PasskeyAuthenticationResponse = object;
```

An authentication response in the WebAuthn JSON encoding, as the passkey
libraries return it.

#### Properties

| Property                                   | Type               |
| ------------------------------------------ | ------------------ |
| <Link id="property-id" /> `id`             | `string`           |
| <Link id="property-rawid" /> `rawId`       | `string`           |
| <Link id="property-response" /> `response` | `object`           |
| `response.authenticatorData`               | `string`           |
| `response.clientDataJSON`                  | `string`           |
| `response.signature`                       | `string`           |
| `response.userHandle?`                     | `string` \| `null` |
| <Link id="property-type" /> `type`         | `"public-key"`     |

---

### PasskeyCeremonies

```ts
type PasskeyCeremonies = object;
```

The passkey ceremonies of a platform, which an app implements to run the passkey
hooks outside a browser. `createUsernamePasskeyHooks` takes one.

`register` and `authenticate` return the response of the platform as it is: the
hooks keep only the fields the server accepts. For a failure they recognize,
they return the matching
[PasskeyClientError](../react/index.md#passkeyclienterror). A thrown error named
`NotAllowedError` or `AbortError` becomes `CEREMONY_ABORTED`, and any other
thrown value becomes `OTHER_ERROR`.

#### Properties

| Property                                           | Type                                                                                                                                                                                                                  | Description                                                        |
| -------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| <Link id="property-authenticate" /> `authenticate` | (`options`) => `Promise`\< \| \{ `response`: [`PasskeyAuthenticationResponse`](#passkeyauthenticationresponse); `success`: `true`; \} \| [`PasskeyClientFailure`](../react/index.md#passkeyclientfailure)\>           | -                                                                  |
| <Link id="property-autofill" /> `autofill?`        | `object`                                                                                                                                                                                                              | Passkey autofill. Without it, autofill reports `available: false`. |
| `autofill.authenticate`                            | (`options`, `signal`) => `Promise`\< \| \{ `response`: [`PasskeyAuthenticationResponse`](#passkeyauthenticationresponse); `success`: `true`; \} \| [`PasskeyClientFailure`](../react/index.md#passkeyclientfailure)\> | -                                                                  |
| `autofill.isAvailable`                             | () => `Promise`\<`boolean`\>                                                                                                                                                                                          | -                                                                  |
| `autofill.onResume?`                               | (`listener`) => () => `void`                                                                                                                                                                                          | -                                                                  |
| <Link id="property-issupported" /> `isSupported`   | () => `boolean`                                                                                                                                                                                                       | Whether this device can run passkey ceremonies.                    |
| <Link id="property-register" /> `register`         | (`options`) => `Promise`\< \| \{ `response`: [`PasskeyRegistrationResponse`](#passkeyregistrationresponse); `success`: `true`; \} \| [`PasskeyClientFailure`](../react/index.md#passkeyclientfailure)\>               | -                                                                  |

---

### PasskeyRegistrationResponse

```ts
type PasskeyRegistrationResponse = object;
```

A registration response in the WebAuthn JSON encoding, as the passkey libraries
return it.

#### Properties

| Property                                     | Type                 |
| -------------------------------------------- | -------------------- |
| <Link id="property-id-1" /> `id`             | `string`             |
| <Link id="property-rawid-1" /> `rawId`       | `string`             |
| <Link id="property-response-1" /> `response` | `object`             |
| `response.attestationObject`                 | `string`             |
| `response.clientDataJSON`                    | `string`             |
| `response.transports?`                       | `string`[] \| `null` |
| <Link id="property-type-1" /> `type`         | `"public-key"`       |

## Functions

### createUsernamePasskeyHooks()

```ts
function createUsernamePasskeyHooks(ceremonies): object;
```

Make the passkey hooks on the given ceremonies. They take the same arguments and
return the same results as the hooks of
`@convex-dev/auth/methods/username-passkey/react`.

Call it once, at the top level of a module:

```ts skip-check
export const { useSignInWithUsernamePasskey, useAddPasskey, useRemovePasskey } =
  createUsernamePasskeyHooks(ceremonies);
```

#### Parameters

| Parameter    | Type                                      |
| ------------ | ----------------------------------------- |
| `ceremonies` | [`PasskeyCeremonies`](#passkeyceremonies) |

#### Returns

`object`

##### useAddPasskey

```ts
useAddPasskey: (api) => object;
```

###### Parameters

| Parameter | Type                                               |
| --------- | -------------------------------------------------- |
| `api`     | [`AddPasskeyApi`](../react/index.md#addpasskeyapi) |

###### Returns

###### addPasskey

```ts
addPasskey: () => Promise<AddPasskeyResult>;
```

Adds a passkey to the account of the signed-in user.

Returns an object with a `success` boolean flag. If it is `false`, the object
has a `userError` field that tells why.

###### Returns

`Promise`\<[`AddPasskeyResult`](../react/index.md#addpasskeyresult)\>

###### pending

```ts
pending: boolean;
```

`true` while an `addPasskey` call is running.

##### useRemovePasskey

```ts
useRemovePasskey: (api) => object;
```

###### Parameters

| Parameter | Type                                                     |
| --------- | -------------------------------------------------------- |
| `api`     | [`RemovePasskeyApi`](../react/index.md#removepasskeyapi) |

###### Returns

###### pending

```ts
pending: boolean;
```

`true` while a `removePasskey` call is running.

###### removePasskey

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

`Promise`\<[`RemovePasskeyResult`](../react/index.md#removepasskeyresult)\>

##### useSignInWithUsernamePasskey

```ts
useSignInWithUsernamePasskey: (api) => object;
```

###### Parameters

| Parameter | Type                                                         |
| --------- | ------------------------------------------------------------ |
| `api`     | [`UsernamePasskeyApi`](../react/index.md#usernamepasskeyapi) |

###### Returns

###### autofill

```ts
autofill: object;
```

The state of the autofill flow:

- `available`: whether the platform can offer passkeys in the autocompletion
  list; `null` while the detection runs.
- `status`: the state of the request; see
  [PasskeyAutofillStatus](../react/index.md#passkeyautofillstatus).
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

###### pending

```ts
pending: boolean;
```

`true` while a sign-in runs: a `signIn` call, or an autofill sign-in that the
server verifies (`autofill.status` is `"signingIn"`). A `signIn` call in this
time returns `ALREADY_PENDING`.

###### signIn

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

`Promise`\<[`SignInWithUsernamePasskeyResult`](../react/index.md#signinwithusernamepasskeyresult)\>

## References

### AddPasskeyApi

Re-exports [AddPasskeyApi](../react/index.md#addpasskeyapi)

---

### AddPasskeyResult

Re-exports [AddPasskeyResult](../react/index.md#addpasskeyresult)

---

### AlreadyPendingFailure

Re-exports [AlreadyPendingFailure](../react/index.md#alreadypendingfailure)

---

### PasskeyAutofillStatus

Re-exports [PasskeyAutofillStatus](../react/index.md#passkeyautofillstatus)

---

### PasskeyClientError

Re-exports [PasskeyClientError](../react/index.md#passkeyclienterror)

---

### PasskeyClientFailure

Re-exports [PasskeyClientFailure](../react/index.md#passkeyclientfailure)

---

### RemovePasskeyApi

Re-exports [RemovePasskeyApi](../react/index.md#removepasskeyapi)

---

### RemovePasskeyResult

Re-exports [RemovePasskeyResult](../react/index.md#removepasskeyresult)

---

### RenamePasskeyResult

Re-exports [RenamePasskeyResult](../react/index.md#renamepasskeyresult)

---

### SignInWithUsernamePasskeyResult

Re-exports
[SignInWithUsernamePasskeyResult](../react/index.md#signinwithusernamepasskeyresult)

---

### UsernamePasskeyApi

Re-exports [UsernamePasskeyApi](../react/index.md#usernamepasskeyapi)

---

### UsernamePasskeyAutofillError

Re-exports
[UsernamePasskeyAutofillError](../react/index.md#usernamepasskeyautofillerror)

---

### WireCreationOptions

Re-exports [WireCreationOptions](../react/index.md#wirecreationoptions)

---

### WireRequestOptions

Re-exports [WireRequestOptions](../react/index.md#wirerequestoptions)
