# Upstream sync playbook

`Bl4ckBl1zZ/convex-backend` is a fork of
[`convex/convex-backend`](https://github.com/convex/convex-backend) (formerly
`get-convex/convex-backend`; GitHub redirects the old name, GHCR does not).
Upstream ships ~10 commits a day. Syncs are done by hand: merge upstream into a
branch, resolve, verify, and open a pull request against `main`.

The fork is deliberately small. Since the 2026-10-18 production review
(`self-hosted/advanced/fork-production.md`) it carries only the three changes
listed under [Invariants](#invariants); everything else is upstream. Keep it
that way: a change that does not measurably help the production deployment
belongs upstream or nowhere.

## Merge strategy

**Always merge, never squash and never rebase.** Merge `upstream/main` with a
real merge commit, and merge the sync PR into `main` with a merge commit.
Squashing or rebasing detaches the fork from upstream's history, so
`git merge-base` stops advancing and every later sync re-conflicts over the
entire backlog.

```bash
git remote add upstream https://github.com/convex/convex-backend.git  # once
git fetch upstream main
git switch -c upstream-sync/$(date +%F) origin/main
git merge upstream/main
.github/fork_workflows.sh --prune  # drop upstream workflows; see invariant 3
```

**Merging to `main` redeploys production, but not its backend.** Dokploy
auto-deploys the profile on every push to `main`, so compose, SQL and dashboard
changes go live on merge. The backend image is pinned by `CONVEX_BACKEND_IMAGE`
in the Dokploy environment, so a merge alone keeps the old backend. To roll out
a new backend:

1. Wait for "Publish Fork Backend Image" to finish for the merge commit. It runs
   on every push to `main` except merges that touch only the fork-owned paths in
   its `paths-ignore` list (sync merges always publish). Without an image for
   the merge commit, start the workflow by hand on `main`.
2. Resolve its digest:
   `docker buildx imagetools inspect ghcr.io/bl4ckbl1zz/convex-backend:sha-<merge-sha>`.
3. Deploy it to ModularBots Dev first (its raw compose pins the image), then set
   `CONVEX_BACKEND_IMAGE=ghcr.io/bl4ckbl1zz/convex-backend:sha-<merge-sha>@sha256:<digest>`
   on production and redeploy.
4. A new backend runs its database migrations in the background after startup.
   Errors such as `missing field` can appear until the logs show
   `Migration complete`; wait for that before deciding to roll back.

## Conflict resolution principle

Upstream owns how the engine works. The fork owns how it is deployed. Take
upstream's side of every conflict except where it would drop an invariant below;
then re-apply the invariant through upstream's new API. Never revert an upstream
bug fix. Upstream API changes can break fork code in files that git merged
cleanly, so always compile after resolving.

## Invariants

### 1. Production deployment profile

`self-hosted/docker/docker-compose.dokploy.yml`,
`self-hosted/docker/postgresql.dokploy.conf`,
`self-hosted/docker/postgres-autovacuum.sql`,
`self-hosted/docker/postgres-autovacuum-health.sql`

Fork-only files: PostgreSQL 18.6 (Debian image), the backend from GHCR, the
dashboard, and an autovacuum maintenance sidecar. They never conflict, but check
upstream's release notes for renamed or removed environment variables the
profile sets. Convex must receive a PostgreSQL **cluster** URL (no database name
in the path). `docker-compose.restorecord.dokploy.yml` is a second, separately
owned profile; do not change it during a sync.

The maintenance SQL runs every six hours and must stay idempotent: it may only
`ALTER` a table when its options differ, and must skip tables that are being
vacuumed, because both `ALTER TABLE ... SET` and `ANALYZE` cancel a running
autovacuum.

### 2. Local Node executor pool

`crates/node_executor/src/local.rs`

`LocalNodeExecutor` runs `LOCAL_NODE_EXECUTOR_POOL_SIZE` Node processes (default
1, production 4) and sends each request to the least-loaded one. Upstream
changes to the executor (spawn arguments, environment, callback timing, health
checks, timeouts) must apply to every process in the pool, not only to one.
Guarded by `cargo test -p node_executor --lib`.

`crates/local_backend/src/main.rs` also passes upstream's
`HTTP_SERVER_MAX_CONCURRENT_REQUESTS` knob to the HTTP server instead of the
hardcoded `MAX_CONCURRENT_REQUESTS` (128); production sets 1024.

### 3. CI and image publishing

`.github/workflows/publish_fork_backend.yml`,
`.github/workflows/build_local_backend.yml`, `.github/workflows/prettier.yml`,
`.github/fork_workflows.sh`, `.github/actions/setup-rust/action.yml`,
`self-hosted/docker-build/Dockerfile.backend`

CI exists to produce the image Dokploy deploys. Three workflows remain; every
other upstream workflow is deleted. Those either target Convex's private
`[self-hosted, aws, ...]` runners, which do not exist here, or release Convex's
own artifacts.

| Workflow                                        | Runs on                                     | Cost                         |
| ----------------------------------------------- | ------------------------------------------- | ---------------------------- |
| Publish Fork Backend Image (fork-only)          | push to `main` (see `paths-ignore`), manual | one amd64 + one arm64 runner |
| Build Convex Backend (required check on `main`) | pull requests, manual                       | one runner, about 20 minutes |
| Prettier (required check on `main`)             | pull requests, manual                       | under a minute               |

- **Publish** builds `Dockerfile.backend` natively on `ubuntu-24.04` and
  `ubuntu-24.04-arm` in parallel, pushes `sha-<sha>-amd64` and
  `sha-<sha>-arm64`, then joins them into `sha-<sha>` (plus `latest` and `main`
  on `main` only). Both architectures are needed: production is amd64,
  RestoreCord's MicroApi Convex host is arm64. Keep the image name and the
  `sha-<commit>` scheme: `CONVEX_BACKEND_IMAGE` pins depend on them. It uses no
  layer cache on purpose: cargo writes to BuildKit cache mounts, which a layer
  cache does not save, and exporting it cost 5-22 minutes per run without making
  cargo faster. `CARGO_BUILD_JOBS=4` matches the runners' 4 vCPUs (the
  Dockerfile's default is also 4).
- **Build Convex Backend** is the pre-merge check for syncs: `cargo check`, a
  clippy pass over the fork's crates and the Node executor pool tests. Its first
  step runs `.github/fork_workflows.sh`.
- `setup-rust` falls back to local-disk sccache when Convex's R2 credentials are
  absent. Keep that fallback.

**Keeping upstream workflows out.** Upstream still ships its workflows, and a
merge brings them back: an upstream change to a deleted workflow is a
modify/delete conflict, and a new upstream workflow arrives silently and would
run. After every `git merge upstream/main`, run
`.github/fork_workflows.sh --prune`; it `git rm`s every workflow outside its
allowlist, which also resolves those conflicts. Build Convex Backend runs the
same script without `--prune` and fails while a stray workflow is present. To
add a workflow to the fork, add it to the script's allowlist.

For `build_local_backend.yml` and `prettier.yml`, take upstream's changes but
keep the fork's triggers (pull requests and manual runs only), the allowlist
check step and the `ubuntu-24.04` / `ubuntu-latest` runners.

## Lockfiles

The fork changes no dependencies. Take upstream's `Cargo.lock` and
`npm-packages/pnpm-lock.yaml` as they are.

## Verification

Run what CI runs. The JS build must come first: `isolate` will not compile
without it.

```bash
npm ci --prefix scripts
just install-js
just turbo run build --filter=component-tests... --filter=convex... \
  --filter=system-udfs... --filter=udf-runtime... --filter=udf-tests...

cargo check --locked -p local_backend -p node_executor
cargo clippy --locked -p local_backend -p node_executor --all-targets --no-deps -- -D warnings
cargo test --locked -p node_executor --lib
```

Before merging a sync that touches the Node executor, also run a few Node
actions concurrently against a backend with `LOCAL_NODE_EXECUTOR_POOL_SIZE=4`
and confirm they are served by more than one process.
