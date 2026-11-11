# Multiple Convex accounts on one machine

The CLI can keep several Convex accounts logged in at the same time and use a
different one in each project directory. A developer who works for several
clients, or keeps work and personal projects apart, can run `npx convex dev` for
project A and project B side by side without `logout` and `login` in between.
This is the change requested in
[get-convex/convex-backend#326](https://github.com/get-convex/convex-backend/issues/326).

This document covers how the feature works, every place it changes what the CLI
does, and what stays exactly as before.

## Nothing changes until a second account is added

A machine starts with the same `~/.convex/config.json` as today: one top-level
`accessToken`. As long as nobody runs `npx convex login --account`, the CLI
reads and writes that file the same way as the released CLI:

- `npx convex login` saves one top-level token.
- `npx convex logout` deletes the file.
- Every command uses that one token.
- No token is ever revoked.

A handful of fixes and warnings apply to everyone. They are listed in
[What changes for every user](#what-changes-for-every-user). None of them change
which credentials a command uses.

The config becomes multi-account only when an account is added with
`npx convex login --account`. The existing token is then kept as an entry called
`unidentified` (see
[Tokens saved before member ids](#tokens-saved-before-member-ids)).

## The config file

```json
{
  "accessToken": "<the default account's token, for older CLI versions>",
  "accounts": {
    "230405": {
      "accessToken": "…",
      "email": "dev@client-a.com",
      "description": "client-a",
      "isDefault": true
    },
    "482122": {
      "accessToken": "…",
      "email": "dev@client-b.com",
      "description": "client-b"
    }
  },
  "instances": {
    "/Users/dev/code/client-b-app": { "account": "482122" }
  },
  "settings": { "revokeReplacedTokens": true }
}
```

- **`accounts`** is keyed by Convex member id. A member id is unique per Convex
  member. An email is not: several members can share one, for example when the
  same address signs in through different providers. `email` and `description`
  are for display only.
- **`isDefault`** marks the default account, used by directories that aren't
  bound to an account. Exactly one account holds it while any exist.
- **`instances`** binds a directory to an account. It applies to that directory
  and everything below it.
- **The top-level `accessToken`** always mirrors the default account's token.
  See [Older CLI versions](#older-cli-versions).
- **`settings`** holds machine-wide options. See
  [Revoking replaced tokens](#revoking-replaced-tokens).

Properties this CLI doesn't recognize are kept when it rewrites the file.

## Which credentials a command uses

For every command, the CLI picks credentials in this order. The first match
wins:

1. `CONVEX_OVERRIDE_ACCESS_TOKEN` in the environment.
2. A project or deployment key from `CONVEX_DEPLOY_KEY` or
   `CONVEX_DEPLOYMENT_TOKEN`, in the shell, `.env.local` or `.env`.
3. The account bound to the current directory, or the closest parent directory
   that has a binding.
4. The default account.
5. The top-level `accessToken`, for a config with a single token.
6. A preview deploy key.

Steps 3 and 4 are new. Deploy keys (step 2) keep priority so that CI setups and
existing projects behave exactly as they do today. That is also why the CLI now
warns loudly when a deploy key is in effect (see
[Deploy keys override accounts](#deploy-keys-override-accounts)).

If a directory is bound to an account that isn't saved (for example after the
config was edited by hand), the CLI reports it and treats the directory as
logged out. It never falls back to the default account, so a command can't
silently run as the wrong member.

Directory matching uses path segments, so a binding for `/code/app` applies to
`/code/app/convex` but not to `/code/app-old`.

## Commands

### `npx convex login`

The plain command keeps its behavior: it checks the token this directory would
use and logs in only if that token is missing or rejected.

- `-y, --yes` accepts the default answer to every prompt, including the device
  name and opening the browser.
- With several accounts saved, a new login for this directory refreshes the
  account the directory uses, but only if the login is the same member. A login
  as a different member is saved as a separate account and never overwrites
  another member's token.

### `npx convex login --account [member-id]`

Adds an account without logging out of the others.

- **With no id, in a terminal:** shows a picker with each saved account and
  whether its token works, plus "Log in to another account". Picking an account
  binds this directory to it.
- **With no id and `--yes`:** logs in to a new account straight away.
- **With no id and no terminal:** prints the saved accounts and asks for
  `--account <member-id>` or `--account --yes`.
- **With an id:** prints one line about that account only (not saved yet, or
  token revoked). If its token works, it says so and doesn't log in again.
- **`--account` takes a member id** (or `unidentified`). Anything else, such as
  `--account list`, is rejected before any login starts.
- `--description <text>` sets the note shown next to the account. Without it, a
  new account is described by its team slugs, with a prompt to change it
  (skipped by `--yes`).

After the browser login, the CLI reads the member from the Convex profile and
saves the new token under that member id:

- **Same member as a saved account:** that account's token is refreshed, and a
  warning explains how to add a different account (log out in the browser
  first).
- **`--account X` but the login is a different member:** the login is saved
  under its own id and X is left untouched.
- **Binding this directory:** offered after a login. The suggested answer is yes
  only if the directory looks like a project (`convex.json` or `package.json`),
  isn't already bound to another account, and the account can reach this
  directory's deployment. With `--yes` or without a terminal, the suggested
  answer is taken, so an existing binding is never moved silently.

The description prompt, and the "Replace it?" prompt for old tokens, come before
the Convex token is created. Quitting at either doesn't leave an unsaved token
on the account.

### `npx convex login status`

Shows the deploy key banner if a key is set, then the default account and the
account this directory uses, then the login status and teams as before.

### `npx convex account`

| Command                               | What it does                                                                                                                                                                                                                                     |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `account list`                        | The default account, the account this directory uses, every saved account with a live token check, and the directories bound to each. Also warns if this directory's account can't reach its deployment, and identifies old tokens where it can. |
| `account use <member-id> [--force]`   | Binds the current directory to an account, after checking that the account can reach this directory's deployment. `--force` skips the check.                                                                                                     |
| `account unbind`                      | Removes the binding that applies here. The directory falls back to the default account.                                                                                                                                                          |
| `account default <member-id>`         | Moves the default role to another account and updates the top-level token for older CLIs.                                                                                                                                                        |
| `account describe <member-id> <text>` | Sets the note shown next to an account.                                                                                                                                                                                                          |
| `account settings [name] [value]`     | Lists or changes settings.                                                                                                                                                                                                                       |

Token state in `account list` is checked live when it runs. A token revoked
later makes commands fail until that account logs in again.

### `npx convex logout`

- **Single token:** deletes the config, as before.
- **Several accounts, in a terminal:** asks which account to log out of, or all
  of them.
- **Several accounts, no terminal:** asks for `--account <member-id>` or
  `--all`.
- **`--account <member-id>`:** removes that account and every directory binding
  to it. If it was the default, another account becomes the default. Removing
  the last account deletes the file.
- **`--all`:** deletes the file.

Logging out doesn't revoke tokens on Convex, as before.

## What changes for every user

These apply with or without a second account.

### Login no longer loops when a deploy key is set

Before, `npx convex login` decided whether you were logged in by testing the
credential the directory resolved to. With `CONVEX_DEPLOY_KEY` in `.env.local`,
that was the deploy key. The auth check rejected it in that form, so every
`login` ran a new browser login, created a new personal access token, and saved
a token the next run ignored again. Login now tests the account token in
`~/.convex/config.json`, whatever deploy key is set.

### Deploy keys override accounts

A deploy key outranks every account, which is the easiest way to end up acting
on the wrong deployment. For example, a `prod:` key in `.env.local` makes
`npx convex run` in a dev checkout act on production. The CLI now says so in two
ways.

A banner is printed by `login`, `login --account`, `login status` and
`account list`:

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
WARNING: CONVEX_DEPLOY_KEY OVERRIDES YOUR CONVEX LOGIN IN THIS DIRECTORY
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Source:   .env.local
Key:      PRODUCTION deployment "oh-dear-123" (valid)
Effect:   `dev`, `deploy`, `run`, `env`, `logs`, `data`, `import` and `export` act on this deployment only. `npx convex dev` and `npx convex run` change production.
Ignored:  the account you are logged in with, and any account set for
          this directory with `npx convex account use`.
To use your account instead, remove CONVEX_DEPLOY_KEY from .env.local.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

A one-line warning is printed by every other command that picks a deployment,
including `dev`, `deploy`, `run`, `env`, `logs`, `data`, `import` and `export`.
It comes before the command does anything, so an invalid key is flagged too:

```
WARNING: CONVEX_DEPLOY_KEY from .env.local overrides your Convex login: this command uses the PRODUCTION deployment "oh-dear-123". Run `npx convex login status` for details.
```

The one-line warning only appears when the machine also has a Convex login. A
machine that only has a deploy key, such as a CI runner, sees no new output. The
banner and the warning are red for production keys and yellow otherwise, and
both start with `WARNING:` so that agents reading the output see them.

### `npx convex dev` when the configured project belongs to another account

Before, when the logged-in account couldn't access the project in `.env.local`,
`dev` printed "You don't have access to the selected project" and went straight
to "create a new project / choose an existing project". Either choice rewrote
`CONVEX_DEPLOYMENT` and lost the pointer to the real project. Now `dev` explains
the situation:

```
This directory is set up for the deployment "robust-ferret-84" (team: cwc, project: cwc-website) by CONVEX_DEPLOYMENT in .env.local,
but member 230405 <dev@client-a.com> can't access it. It probably belongs to another Convex account.
? What would you like to do?
❯ Use member 482122 <dev@client-b.com>, which can access it
  Log in to another Convex account
  Set up a different project here (replaces CONVEX_DEPLOYMENT in .env.local)
```

- **Use a saved account:** only offered for accounts that pass the deployment
  access check. It binds the directory and continues.
- **Log in to another account:** logs in, saves and binds the new account, then
  checks access again. If that account can't reach the project either, the same
  choices come back.
- **Set up a different project:** the previous flow, and the only choice that
  rewrites `CONVEX_DEPLOYMENT`.

With `--team` and `--project`, `dev` goes straight to the previous flow, because
the project is already chosen. Without a terminal, `dev` stops with the same
explanation and the commands that fix it. It used to fail while trying to show
the project picker.

### New output in `login`

When plain `login` finds the device already authorized, it adds one line
pointing to `npx convex login --account`. With several accounts, it also names
the default account and the account this directory uses.

## Tokens saved before member ids

A token written by `npx convex login`, or by an older CLI, has no member id
attached. When the config becomes multi-account, that token is kept as the
account `unidentified`, and it stays the default account, so nothing stops
working. The Convex profile endpoint only accepts the token from the browser
login, not a saved personal access token, so the CLI can't simply look the
member up.

Instead, the CLI identifies it by comparing token lists. Any personal access
token can list all personal access tokens of its own member
(`GET /v1/list_personal_access_tokens`). Two different tokens of the same member
get back the same list, which contains both of them. Tokens of different members
get back lists with nothing in common.

The comparison runs in two places:

- **After every login**, against the new token.
- **In `account list` and the `login --account` picker**, against every account
  already saved by member id. This needs no login.

When they match, the old entry is filed under the member id. Its default role
and its directory bindings move with it, and the account keeps the newer token:

```
Identified the "unidentified" token: it belongs to member 230405 <dev@client-a.com>. It is now listed under that member, which stays the default account.
```

Special cases:

- **`npx convex login --account unidentified`** always runs the browser login,
  even when the old token works. That login is how the member gets recorded.
- **The old token no longer works,** so it can't be compared. After a login, a
  terminal asks whether to replace it with the member who just logged in. That
  hands over its default role and directories. With `--yes` or without a
  terminal, it's kept as a separate entry, which
  `npx convex logout --account unidentified` removes.
- **The token belongs to nobody who is saved.** It stays `unidentified` until
  its member logs in once.

## Revoking replaced tokens

When a newer token of the same member replaces an older one on this machine, the
CLI revokes the older one on Convex (`POST /v1/delete_personal_access_token`).
That covers two cases:

- logging in again to a saved account;
- identifying an old token.

Rules:

- **Setting:** it's controlled by `settings.revokeReplacedTokens`, which
  defaults to `true`. Turn it off with
  `npx convex account settings revoke-replaced-tokens false`, and the CLI then
  says how many replaced tokens it left active.
- **Which token authorizes it:** the revocation uses the same member's current
  token, so it can only delete that member's own tokens. Another member's token
  is never revoked.
- **Tokens still in use:** a token still saved anywhere in the config is never
  revoked.
- **Single-token configs:** nothing is revoked, as before.

## Checking deployment access before binding

When the current directory's `CONVEX_DEPLOYMENT` names a cloud deployment,
binding an account to the directory first checks that the account can see it
(`GET /v1/deployments/<name>`). Local and anonymous deployments are skipped.

| Situation                        | In a terminal                                         | With `--yes` or no terminal           |
| -------------------------------- | ----------------------------------------------------- | ------------------------------------- |
| Account can reach the deployment | Binds                                                 | Binds                                 |
| Account can't reach it           | Warns, asks "Use it anyway?" (default no)             | Warns, leaves the directory unchanged |
| Account's token doesn't work     | Warns that access can't be checked, asks (default no) | Warns, leaves the directory unchanged |

`npx convex account use <member-id> --force` binds without the check.
`account list` repeats the warning for the account the current directory uses.

## Older CLI versions

Each project pins its own `convex` version, so an older CLI can read the same
`~/.convex/config.json`.

- **Reading:** an older CLI reads the top-level `accessToken`, which mirrors the
  default account, and keeps working as that account in every directory.
- **Logging in:** an older `npx convex login` replaces only the top-level token
  and keeps `accounts`, `instances` and `settings`. The next write by this CLI
  puts the default account's token back in the mirror.
- **Logging out:** an older `npx convex logout` deletes the whole file,
  including every account.
- **Development configs:** configs written by development builds of this feature
  that marked the default with `isPrimary` are read as `isDefault`.

## Running without a terminal

Agents and CI never hit a prompt they can't answer:

| Command                            | Without a terminal                                                                             |
| ---------------------------------- | ---------------------------------------------------------------------------------------------- |
| `login --account`                  | Lists accounts, asks for `--account <member-id>` or `--yes`                                    |
| `login --account --yes`            | Logs in to a new account, takes every default                                                  |
| Binding offer after login          | Binds only if the directory looks like a project, isn't bound elsewhere, and access checks out |
| `account use` without access       | Leaves the directory unchanged, points to `--force`                                            |
| `logout` with several accounts     | Asks for `--account` or `--all`                                                                |
| `dev` with an inaccessible project | Explains, lists `account use` / `login --account` / `dev --configure`                          |
| Revoked old token after a login    | Kept as a separate entry                                                                       |

## Convex APIs used

| Call                                    | Used for                              | Accepts a personal access token  |
| --------------------------------------- | ------------------------------------- | -------------------------------- |
| `HEAD /api/authorize`                   | Is a token still valid                | Yes                              |
| `GET /api/dashboard/profile`            | Member id and email after a login     | No, only the browser-login token |
| `GET /api/teams`                        | Default description for a new account | Yes                              |
| `GET /v1/list_personal_access_tokens`   | Identifying old tokens                | Yes                              |
| `POST /v1/delete_personal_access_token` | Revoking replaced tokens              | Yes                              |
| `GET /v1/deployments/<name>`            | Deployment access check               | Yes                              |

`/api/dashboard/profile` is the endpoint the dashboard uses. It isn't part of
the public management API. A public "current member" endpoint that accepts
personal access tokens would remove the need for the token-list comparison.

## Tests

`accounts.test.ts` runs the built CLI as a separate process against a fake
Convex API on localhost, with a fresh home and project directory for each test.
`--override-access-token` stands in for the browser login, and commands run
without a terminal. It covers:

- existing single-account behavior;
- deploy keys;
- `login --account`;
- old tokens;
- revocation;
- binding and access checks;
- `account list`;
- `logout`;
- `dev` with a project another account owns.

The single-account tests also pass against the published CLI. Set
`CONVEX_CLI_UNDER_TEST` to its `bin/main.js` to check:

```bash
CONVEX_CLI_UNDER_TEST=/path/to/node_modules/convex/bin/main.js npx vitest run src/cli/accounts.test.ts -t "existing single-account behavior"
```

`lib/utils/globalConfig.test.ts` covers the config helpers: directory
resolution, adding, merging and removing accounts, the default role, and
settings.

Prompts that need a real terminal were tested by hand: the account picker, "Use
it anyway?", "Replace it?", and the spinner stopping before a prompt.
