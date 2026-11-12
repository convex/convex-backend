---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/email-password/react
sidebar_label: react
---

import Link from "@docusaurus/Link";

React client for the `emailPassword` method, exported at
`@convex-dev/auth/methods/email-password/react`.

## Type Aliases

### CompleteChangeEmailState

```ts
type CompleteChangeEmailState = LinkFlowState<
  ExtractError<CompleteChangeEmailResult> | MissingSecretError | OtherError
>;
```

The state of [useCompleteChangeEmail](#usecompletechangeemail).

---

### CompletePasswordRecoveryResult

```ts
type CompletePasswordRecoveryResult =
  | ClientView<CompletePasswordRecoveryResult>
  | SignInUnexpectedFailure;
```

The result of the `completePasswordRecovery` callback of the `ready` state of
[useCompletePasswordRecovery](#usecompletepasswordrecovery).

---

### CompletePasswordRecoveryState

```ts
type CompletePasswordRecoveryState =
  | {
      status: "loading";
    }
  | {
      completePasswordRecovery: (
        args,
      ) => Promise<CompletePasswordRecoveryResult>;
      pending: boolean;
      status: "ready";
    }
  | {
      status: "complete";
    }
  | {
      status: "error";
      userError: PasswordRecoveryLinkError | MissingSecretError;
    };
```

The state of [useCompletePasswordRecovery](#usecompletepasswordrecovery). Unlike
the other landing page hooks, the flow needs input from the user (the new
password) before it can complete, so a `ready` state carries the function to
submit it.

#### Union Members

##### Type Literal

```ts
{
  status: "loading";
}
```

---

##### Type Literal

```ts
{
  completePasswordRecovery: (args) => Promise<CompletePasswordRecoveryResult>;
  pending: boolean;
  status: "ready";
}
```

###### completePasswordRecovery

```ts
completePasswordRecovery: (args) => Promise<CompletePasswordRecoveryResult>;
```

Set the new password and sign the user in. Errors about the new password come
back in the result and leave the state `ready`, so the user can try another
password with the same link.

###### Parameters

| Parameter          | Type                           |
| ------------------ | ------------------------------ |
| `args`             | \{ `newPassword`: `string`; \} |
| `args.newPassword` | `string`                       |

###### Returns

`Promise`\<[`CompletePasswordRecoveryResult`](#completepasswordrecoveryresult)\>

###### pending

```ts
pending: boolean;
```

`true` while `completePasswordRecovery` is in flight.

###### status

```ts
status: "ready";
```

---

##### Type Literal

```ts
{
  status: "complete";
}
```

---

##### Type Literal

```ts
{
  status: "error";
  userError: PasswordRecoveryLinkError | MissingSecretError;
}
```

---

### CompleteSignUpState

```ts
type CompleteSignUpState = LinkFlowState<
  | SignInError<ClientView<CompleteSignUpResult>>
  | MissingSecretError
  | OtherError
>;
```

The state of
[useCompleteSignUpWithEmailPassword](#usecompletesignupwithemailpassword).

---

### LinkFlowState

```ts
type LinkFlowState<UserError> =
  | {
      status: "loading";
    }
  | {
      status: "complete";
    }
  | {
      status: "error";
      userError: UserError;
    };
```

The state of a landing page hook: `loading` until the link has been presented,
then `complete` or `error`.

#### Type Parameters

| Type Parameter |
| -------------- |
| `UserError`    |

---

### PasswordRecoveryApi

```ts
type PasswordRecoveryApi = object;
```

The functions that [useCompletePasswordRecovery](#usecompletepasswordrecovery)
calls. The app's `api.auth` has both of them.

#### Properties

| Property                                                                   | Type                               |
| -------------------------------------------------------------------------- | ---------------------------------- |
| <Link id="property-checkpasswordrecovery" /> `checkPasswordRecovery`       | `CheckPasswordRecoveryQuery`       |
| <Link id="property-completepasswordrecovery" /> `completePasswordRecovery` | `CompletePasswordRecoveryMutation` |

---

### SignInResult

```ts
type SignInResult = ClientView<SignInResult> | SignInUnexpectedFailure;
```

The result of the `signIn` callback from
[useSignInWithEmailPassword](#usesigninwithemailpassword).

---

### SignUpResult

```ts
type SignUpResult = WithoutSecret<ClientView<SignUpResult>> | UnexpectedFailure;
```

The result of the `signUp` callback from
[useSignUpWithEmailPassword](#usesignupwithemailpassword).

---

### StartChangeEmailResult

```ts
type StartChangeEmailResult =
  | WithoutSecret<StartChangeEmailResult>
  | UnexpectedFailure;
```

The result of the `startChangeEmail` callback from
[useStartChangeEmail](#usestartchangeemail).

---

### StartPasswordRecoveryResult

```ts
type StartPasswordRecoveryResult =
  | WithoutSecret<StartPasswordRecoveryResult>
  | UnexpectedFailure;
```

The result of the `startPasswordRecovery` callback from
[useStartPasswordRecovery](#usestartpasswordrecovery).

## Functions

### useCompleteChangeEmail()

```ts
function useCompleteChangeEmail(api, emailCode): CompleteChangeEmailState;
```

Client for the landing page of the email-change confirmation link: as soon as
the page opens, present the code from the link with the secret stored by
[useStartChangeEmail](#usestartchangeemail). No session is adopted: the user
already has one, and the backend requires it.

#### Parameters

| Parameter                 | Type                                                        | Description                                      |
| ------------------------- | ----------------------------------------------------------- | ------------------------------------------------ |
| `api`                     | \{ `completeChangeEmail`: `CompleteChangeEmailMutation`; \} | The functions of the method, usually `api.auth`. |
| `api.completeChangeEmail` | `CompleteChangeEmailMutation`                               | -                                                |
| `emailCode`               | \{ `emailCode`: `string`; \}                                | The `code` query parameter of the link.          |
| `emailCode.emailCode`     | `string`                                                    | -                                                |

#### Returns

[`CompleteChangeEmailState`](#completechangeemailstate)

---

### useCompletePasswordRecovery()

```ts
function useCompletePasswordRecovery(
  recoveryApi,
  emailCode,
): CompletePasswordRecoveryState;
```

Client for the landing page of the password-reset link. As soon as the page
opens, it reads the secret stored by
[useStartPasswordRecovery](#usestartpasswordrecovery) and subscribes to
`checkPasswordRecovery` with it and the code from the link. When the link can be
used, the state is `ready`, with the function that sets the new password and
adopts the minted session. When the link cannot be used, the state is `error`,
so the page does not ask for a password.

The check is a subscription, thus the state moves to `error` on its own when the
link dies while the page is open: another tab claims it, or it expires and the
server erases it.

The hook does not complete the flow by itself: the flow needs the new password,
so call `completePasswordRecovery` when the user submits the form. An error
about the password keeps the state `ready`, so show it in the form. An error
about the link moves the state to `error`.

```tsx
function ResetPassword({ emailCode }: { emailCode: string }) {
  const state = useCompletePasswordRecovery(api.auth, { emailCode });
  const [newPassword, setNewPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  switch (state.status) {
    case "loading":
      return <p>Checking the link…</p>;
    case "complete":
      return <Navigate to="/" replace />;
    case "error":
      // map state.userError (MISSING_SECRET, INVALID_CHALLENGE, …) to a message
      return <p role="alert">This link cannot be used.</p>;
  }

  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        const result = await state.completePasswordRecovery({ newPassword });
        // map result.userError (PASSWORD_TOO_SHORT, …) to a message
        setError(result.status === "error" ? result.userError.error : null);
      }}
    >
      <label>
        New password
        <input
          type="password"
          autoComplete="new-password"
          required
          value={newPassword}
          onChange={(e) => setNewPassword(e.target.value)}
          disabled={state.pending}
        />
      </label>
      {error !== null && <p role="alert">{error}</p>}
      <button type="submit" disabled={state.pending}>
        Reset password and sign in
      </button>
    </form>
  );
}
```

#### Parameters

| Parameter             | Type                                          | Description                                                                                                         |
| --------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `recoveryApi`         | [`PasswordRecoveryApi`](#passwordrecoveryapi) | The app's `checkPasswordRecovery` query and `completePasswordRecovery` mutation references, for example `api.auth`. |
| `emailCode`           | \{ `emailCode`: `string`; \}                  | The `code` query parameter of the link.                                                                             |
| `emailCode.emailCode` | `string`                                      | -                                                                                                                   |

#### Returns

[`CompletePasswordRecoveryState`](#completepasswordrecoverystate)

---

### useCompleteSignUpWithEmailPassword()

```ts
function useCompleteSignUpWithEmailPassword(
  api,
  emailCode,
): CompleteSignUpState;
```

Client for the landing page of the validation link: as soon as the page opens,
present the code from the link with the secret stored by
[useSignUpWithEmailPassword](#usesignupwithemailpassword), and adopt the minted
session.

```tsx
function ValidateEmail({ emailCode }: { emailCode: string }) {
  const state = useCompleteSignUpWithEmailPassword(api.auth, { emailCode });
  switch (state.status) {
    case "loading": // validating…
    case "complete": // signed in
    case "error": // map state.userError to a message
  }
}
```

#### Parameters

| Parameter                             | Type                                                               | Description                                      |
| ------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------ |
| `api`                                 | \{ `completeSignUpWithEmailPassword`: `CompleteSignUpMutation`; \} | The functions of the method, usually `api.auth`. |
| `api.completeSignUpWithEmailPassword` | `CompleteSignUpMutation`                                           | -                                                |
| `emailCode`                           | \{ `emailCode`: `string`; \}                                       | The `code` query parameter of the link.          |
| `emailCode.emailCode`                 | `string`                                                           | -                                                |

#### Returns

[`CompleteSignUpState`](#completesignupstate)

---

### useSignInWithEmailPassword()

```ts
function useSignInWithEmailPassword(api): object;
```

Client for the sign-in flow: run the `signInWithEmailPassword` mutation and, on
success, establish an authenticated session.

```tsx
function LogIn() {
  const { signIn, pending } = useSignInWithEmailPassword(api.auth);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        const result = await signIn({ email, password });
        // map result.userError (INVALID_CREDENTIALS, …) to a message
        setError(result.status === "error" ? result.userError.error : null);
      }}
    >
      <label>
        Email
        <input
          type="email"
          autoComplete="username"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          disabled={pending}
        />
      </label>
      <label>
        Password
        <input
          type="password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          disabled={pending}
        />
      </label>
      {error !== null && <p role="alert">{error}</p>}
      <button type="submit" disabled={pending}>
        Log in
      </button>
    </form>
  );
}
```

#### Parameters

| Parameter                     | Type                                               | Description                                      |
| ----------------------------- | -------------------------------------------------- | ------------------------------------------------ |
| `api`                         | \{ `signInWithEmailPassword`: `SignInMutation`; \} | The functions of the method, usually `api.auth`. |
| `api.signInWithEmailPassword` | `SignInMutation`                                   | -                                                |

#### Returns

`object`

##### pending

```ts
pending: boolean;
```

##### signIn

```ts
signIn: (credentials) => Promise<SignInResult>;
```

###### Parameters

| Parameter              | Type                                           |
| ---------------------- | ---------------------------------------------- |
| `credentials`          | \{ `email`: `string`; `password`: `string`; \} |
| `credentials.email`    | `string`                                       |
| `credentials.password` | `string`                                       |

###### Returns

`Promise`\<[`SignInResult`](#signinresult)\>

---

### useSignUpWithEmailPassword()

```ts
function useSignUpWithEmailPassword(api): object;
```

Client for the sign-up flow: run the `signUpWithEmailPassword` mutation and keep
the returned secret for
[useCompleteSignUpWithEmailPassword](#usecompletesignupwithemailpassword).

A successful sign-up does _not_ sign the user in: it sends the validation email.
Tell the user to open the link (in this same browser).

```tsx
function SignUp() {
  const { signUp, pending } = useSignUpWithEmailPassword(api.auth);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [sentTo, setSentTo] = useState<string | null>(null);

  if (sentTo !== null) {
    return <p>We sent a link to {sentTo}. Open it in this browser.</p>;
  }

  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        const result = await signUp({ email, password });
        if (result.success) {
          setSentTo(email);
        } else {
          // map result.userError (EMAIL_TAKEN, PASSWORD_TOO_SHORT, …) to a message
          setError(result.userError.error);
        }
      }}
    >
      <label>
        Email
        <input
          type="email"
          autoComplete="username"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          disabled={pending}
        />
      </label>
      <label>
        Password
        <input
          type="password"
          autoComplete="new-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          disabled={pending}
        />
      </label>
      {error !== null && <p role="alert">{error}</p>}
      <button type="submit" disabled={pending}>
        Create account
      </button>
    </form>
  );
}
```

#### Parameters

| Parameter                     | Type                                               | Description                                      |
| ----------------------------- | -------------------------------------------------- | ------------------------------------------------ |
| `api`                         | \{ `signUpWithEmailPassword`: `SignUpMutation`; \} | The functions of the method, usually `api.auth`. |
| `api.signUpWithEmailPassword` | `SignUpMutation`                                   | -                                                |

#### Returns

`object`

##### pending

```ts
pending: boolean;
```

##### signUp

```ts
signUp: (credentials) => Promise<SignUpResult>;
```

###### Parameters

| Parameter              | Type                                           |
| ---------------------- | ---------------------------------------------- |
| `credentials`          | \{ `email`: `string`; `password`: `string`; \} |
| `credentials.email`    | `string`                                       |
| `credentials.password` | `string`                                       |

###### Returns

`Promise`\<[`SignUpResult`](#signupresult)\>

---

### useStartChangeEmail()

```ts
function useStartChangeEmail(api): object;
```

Client for starting an email change: run the backend's `startChangeEmail`
mutation and keep the returned secret for
[useCompleteChangeEmail](#usecompletechangeemail).

#### Parameters

| Parameter              | Type                                                  | Description                                      |
| ---------------------- | ----------------------------------------------------- | ------------------------------------------------ |
| `api`                  | \{ `startChangeEmail`: `StartChangeEmailMutation`; \} | The functions of the method, usually `api.auth`. |
| `api.startChangeEmail` | `StartChangeEmailMutation`                            | -                                                |

#### Returns

`object`

##### pending

```ts
pending: boolean;
```

##### startChangeEmail

```ts
startChangeEmail: (args) => Promise<StartChangeEmailResult>;
```

###### Parameters

| Parameter              | Type                                                     |
| ---------------------- | -------------------------------------------------------- |
| `args`                 | \{ `currentPassword`: `string`; `newEmail`: `string`; \} |
| `args.currentPassword` | `string`                                                 |
| `args.newEmail`        | `string`                                                 |

###### Returns

`Promise`\<[`StartChangeEmailResult`](#startchangeemailresult)\>

---

### useStartPasswordRecovery()

```ts
function useStartPasswordRecovery(api): object;
```

Client for starting a password recovery: run the backend's
`startPasswordRecovery` mutation and keep the returned secret for
[useCompletePasswordRecovery](#usecompletepasswordrecovery). On success,
`sentTo` gives the address that received the link. Show this address to the
user, not the typed one: the two can differ in case or in Unicode form.

#### Parameters

| Parameter                   | Type                                                            | Description                                      |
| --------------------------- | --------------------------------------------------------------- | ------------------------------------------------ |
| `api`                       | \{ `startPasswordRecovery`: `StartPasswordRecoveryMutation`; \} | The functions of the method, usually `api.auth`. |
| `api.startPasswordRecovery` | `StartPasswordRecoveryMutation`                                 | -                                                |

#### Returns

`object`

##### pending

```ts
pending: boolean;
```

##### startPasswordRecovery

```ts
startPasswordRecovery: (args) => Promise<StartPasswordRecoveryResult>;
```

###### Parameters

| Parameter    | Type                     |
| ------------ | ------------------------ |
| `args`       | \{ `email`: `string`; \} |
| `args.email` | `string`                 |

###### Returns

`Promise`\<[`StartPasswordRecoveryResult`](#startpasswordrecoveryresult)\>
