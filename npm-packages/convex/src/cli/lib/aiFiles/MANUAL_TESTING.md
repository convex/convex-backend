# Manual Testing Guide - Convex AI Files

Most smoke scenarios are now covered by automated tests:

- unit tests in `config.test.ts` and `index.test.ts`
- temp-dir integration tests in `integration.test.ts`
- interactive prompt tests in `prompt.test.ts` (uses `@inquirer/confirm` with
  controlled I/O to verify yes/no branching in `maybeSetupAiFiles`)

The integration suite covers install, update, status, disable, enable, remove
flows, functions directory overrides, nested-directory detection, AGENTS.md /
CLAUDE.md preservation, legacy cursor rules cleanup, staleness nag logic, and
locally modified guidelines skip behavior.

Only the scenarios below remain manual because they depend on real
environment/agent tooling and the live `skills` CLI behavior.

---

## Local version API override

When testing unreleased changes to `version.convex.dev`, point the CLI at a
local stub or local version service with:

`CONVEX_VERSION_API_ORIGIN=http://127.0.0.1:3212`

Example:

`CONVEX_VERSION_API_ORIGIN=http://127.0.0.1:3212 npx convex ai-files remove`

This is useful during rollout windows when the local CLI expects a newer
`/v1/agent_skills` response shape than the deployed service currently serves.

---

## Manual Test 1 - Real skills installation paths across agents

**Scenario:** verify `npx skills` writes to real agent-native locations.

After `npx convex ai-files install` (or `update`) verify:

- `.agents/skills/` (canonical project-scoped install)
- `.claude/skills/` (Claude Code symlinks when `claude-code` is configured)
- any other agent-specific path configured in `aiFiles.skills.agents`

Expected skill set (as of Apr 2026): `convex`, `convex-create-component`,
`convex-migration-helper`, `convex-performance-audit`, `convex-quickstart`,
`convex-setup-auth`.

Note: the live skill set is fetched remotely and may change. Check the current
list against what `npx convex ai-files status` reports after install.

Note: `.cursor/skills/` is not expected from the default install path today. The
default agents are `codex` and `claude-code`, which map to `.agents/skills` and
`.claude/skills`.

**Why manual:** these paths and symlink behaviors are environment-dependent and
are mocked in automated tests.

---

## Manual Test 2 - Pre-existing non-Convex skills survive remove

**Scenario:** verify `npx convex ai-files remove` does not delete a user skill
that was already installed before `npx convex ai-files install`.

Suggested flow in a fresh temp project:

1. Install a non-Convex skill first, for example:
   `npx skills add vercel-labs/agent-skills --skill web-design-guidelines --agent codex --yes`
2. Run `npx convex ai-files install`
3. Run `npx convex ai-files remove`
4. Run `npx skills ls --json`

Expected result:

- `web-design-guidelines` is still present
- the Convex-managed skills are removed

**Why manual:** this exercises the real on-disk interaction between
`skills add`, `skills remove`, and Convex's before/after skill snapshot logic.

---

## Manual Test 3 - Pre-existing Convex skills are not claimed

**Scenario:** verify the CLI no longer relies on local tracked skill names or
provenance for removal.

Suggested flow in a fresh temp project:

1. Install one Convex skill manually first, for example:
   `npx skills add convex/agent-skills --skill convex-quickstart --agent codex --yes`
2. Run `npx convex ai-files install`
3. Inspect `convex/_generated/ai/ai-files.state.json`
4. Run `npx convex ai-files remove`
5. Run `npx skills ls --json`

Expected result:

- `ai-files.state.json` does not contain `installedSkillNames`
- after `remove`, `convex-quickstart` is removed if it is present in the
  canonical catalog
- removal behavior is driven entirely by the canonical version catalog, not by
  whether a given skill was installed manually or by `ai-files install`

**Why manual:** this confirms the new simpler behavior after removing local
skill provenance tracking.

---

## Manual Test 4 - Remove uses the canonical version catalog

**Scenario:** verify `npx convex ai-files remove` ignores local tracked skill
state and instead removes every skill name returned by
`https://version.convex.dev/v1/agent_skills`, including deleted ones.

Suggested flow in a fresh temp project:

1. Install a non-Convex skill and a couple of Convex skills, for example:
   `npx skills add vercel-labs/agent-skills --skill web-design-guidelines --agent codex --yes`
   `npx skills add convex/agent-skills --skill convex-quickstart --agent codex --yes`
2. Write any `convex/_generated/ai/ai-files.state.json` you like, including junk
   fields from older CLIs if you want.
3. Run `npx convex ai-files remove`
4. Run `npx skills ls --json`

Expected result:

- `remove` succeeds without consulting local tracked skill names
- `web-design-guidelines` is still present
- Convex skills returned by the canonical catalog are removed
- remove still succeeds if the catalog includes a deleted skill name that is not
  installed locally

**Why manual:** this verifies the new canonical source of truth and confirms
that `npx skills remove` is idempotent for missing skills.
