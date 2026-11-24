---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/email-magic-link/react
sidebar_label: react
---

React client for the `emailMagicLink` method, exported at
`@convex-dev/auth/methods/email-magic-link/react`.

## Type Aliases

### SendResult

```ts
type SendResult =
  | {
      success: true;
    }
  | Extract<
      SendResult,
      {
        success: false;
      }
    >
  | {
      success: false;
      userError: OtherError;
    };
```

The result of the `sendMagicLink` callback from
[useSendEmailMagicLink](#usesendemailmagiclink).

---

### SignInState

```ts
type SignInState =
  | {
      status: "loading";
    }
  | {
      status: "complete";
    }
  | {
      status: "error";
      userError:
        | Extract<
            ClientView<SignInResult>,
            {
              status: "error";
            }
          >["userError"]
        | MissingSecretError
        | OtherError;
    };
```

The state of [useSignInWithEmailMagicLink](#usesigninwithemailmagiclink).

## Functions

### useSendEmailMagicLink()

```ts
function useSendEmailMagicLink(api): object;
```

Client for the one form of the method: send a sign-in link to the address, and
keep the returned secret for
[useSignInWithEmailMagicLink](#usesigninwithemailmagiclink). The same call
serves a person who has an account and a person who has none.

```tsx
function SignIn() {
  const { sendMagicLink, pending } = useSendEmailMagicLink(api.auth);
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (sent) {
    return <p>We sent a sign-in link to {email}.</p>;
  }
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        setError(null);
        const result = await sendMagicLink({ email });
        if (result.success) {
          setSent(true);
        } else {
          setError(result.userError.error);
        }
      }}
    >
      <input
        type="email"
        value={email}
        onChange={(e) => setEmail(e.target.value)}
      />
      <button type="submit" disabled={pending}>
        Continue with email
      </button>
      {error !== null && <p>Could not send the link: {error}</p>}
    </form>
  );
}
```

#### Parameters

| Parameter                | Type                                        | Description                                      |
| ------------------------ | ------------------------------------------- | ------------------------------------------------ |
| `api`                    | \{ `sendEmailMagicLink`: `SendMutation`; \} | The functions of the method, usually `api.auth`. |
| `api.sendEmailMagicLink` | `SendMutation`                              | -                                                |

#### Returns

`object`

##### pending

```ts
pending: boolean;
```

##### sendMagicLink

```ts
sendMagicLink: (args) => Promise<SendResult>;
```

###### Parameters

| Parameter    | Type                     |
| ------------ | ------------------------ |
| `args`       | \{ `email`: `string`; \} |
| `args.email` | `string`                 |

###### Returns

`Promise`\<[`SendResult`](#sendresult)\>

---

### useSignInWithEmailMagicLink()

```ts
function useSignInWithEmailMagicLink(api, emailCode): SignInState;
```

Client for the landing page of the link: as soon as the page opens, present the
code from the link with the secret stored by
[useSendEmailMagicLink](#usesendemailmagiclink), and adopt the minted session.

```tsx
function SignInWithLink({ emailCode }: { emailCode: string }) {
  const state = useSignInWithEmailMagicLink(api.auth, { emailCode });
  switch (state.status) {
    case "loading": // signing in…
    case "complete": // signed in
    case "error": // map state.userError to a message
  }
}
```

#### Parameters

| Parameter                      | Type                                                | Description                                      |
| ------------------------------ | --------------------------------------------------- | ------------------------------------------------ |
| `api`                          | \{ `signInWithEmailMagicLink`: `SignInMutation`; \} | The functions of the method, usually `api.auth`. |
| `api.signInWithEmailMagicLink` | `SignInMutation`                                    | -                                                |
| `emailCode`                    | \{ `emailCode`: `string`; \}                        | The `code` query parameter of the link.          |
| `emailCode.emailCode`          | `string`                                            | -                                                |

#### Returns

[`SignInState`](#signinstate)
