set fallback := true
set shell := ["bash", "-uc"]
set windows-shell := ["sh", "-uc"]

instance_name := "open-source-convex-backend"
storage_dir := justfile_directory() / "convex_local_storage"
instance_secret_file := storage_dir / "oss_instance_secret"
admin_key_file := storage_dir / "oss_admin_key"

# `just --list` (or just `just`) will print all the recipes in
# the current Justfile. `just RECIPE` will run the macro/job.
#
# In several places there are recipes for running common scripts or commands.
# Instead of `Makefile`s, Convex uses Justfiles, which are similar, but avoid
# several footguns associated with Makefiles, since using make as a macro runner
# can sometimes conflict with Makefiles desire to have some rudimentary
# understanding of build artifacts and associated dependencies.
#
# Read up on just here: https://github.com/casey/just

_default:
  @just --list

set positional-arguments

# (*) Run the open source convex backend on port 3210
run-local-backend *ARGS:
  cargo run -p local_backend --bin convex-local-backend -- \
    --instance-name {{instance_name}} \
    --instance-secret "$(just init-instance-secret)" \
    "$@"

run-dashboard *ARGS:
  cd '{{justfile_directory() / "npm-packages/dashboard-self-hosted"}}' && \
    if [ -n "{{ARGS}}" ]; then \
      NEXT_PUBLIC_DEPLOYMENT_URL="{{ARGS}}" npm run dev; \
    else \
      NEXT_PUBLIC_DEPLOYMENT_URL="http://127.0.0.1:3210" \
      NEXT_PUBLIC_ADMIN_KEY="$(just generate-admin-key)" \
      npm run dev; \
    fi

# Uses an admin key from admin_key.txt for dev backends.
# This uses the default admin key for local backends, which is safe as long as the backend is
# running locally.
# (*) Run convex CLI commands like `convex dev` against local backend from `just run-local-backend`.
convex *ARGS:
  cd {{invocation_directory()}}; npx convex "$@" --admin-key "$(just generate-admin-key)" --url "http://127.0.0.1:3210"

# Clears any data or stored files from the local backend.
reset-local-backend:
  rm -rf convex_local_storage && rm -f convex_local_backend.sqlite3

# (*) install JS deps exactly as pinned in the lockfile (CI, after pulling)
install-js:
  cd "{{justfile_directory()}}/npm-packages"; just pnpm install --frozen-lockfile

# (*) install JS deps and update the lockfile (when you're changing JS deps)
update-js:
  cd "{{justfile_directory()}}/npm-packages"; just pnpm install

# Global JavaScript tools
# Common commands are
# - `just turbo run build` to build all projects in npm-packages
# - `just turbo run build --force` to build when turbo doesn't realize something's changed
# - `just install-js` when the repo has changed JS deps
# - `just update-js` when you're changing JS deps
# (*) turbo, the task runner for building/testing npm-packages projects
turbo *ARGS:
  #!/usr/bin/env bash
  # turbo shells out to `pnpm` by name, so the pinned copy must be on PATH.
  export PATH="{{justfile_directory()}}/scripts/node_modules/.bin:$PATH"
  cd "{{justfile_directory()}}/npm-packages"
  mkdir -p .turbo
  # turbo has no cross-process task lock, so concurrent runs (e.g. a JS build
  # racing isolate's build.rs during a parallel cargo build) can execute the
  # same task twice and race writes to shared outputs/cache entries. flock(1)
  # serializes them on a per-checkout lock file (shared with build.rs);
  # platforms without flock (macOS) run unlocked, where such races are
  # transient and a rerun fixes them.
  if command -v flock >/dev/null 2>&1; then
      exec flock .turbo/turbo.lock "{{justfile_directory()}}/scripts/node_modules/.bin/turbo" "$@"
  fi
  exec "{{justfile_directory()}}/scripts/node_modules/.bin/turbo" "$@"

# (*) pnpm, the JS package manager (pinned in scripts/package.json)
pnpm *ARGS:
  cd {{invocation_directory()}}; "{{justfile_directory()}}/scripts/node_modules/.bin/pnpm" "$@"

# Generates (or reuses) a random per-installation instance secret
init-instance-secret:
  @mkdir -p '{{storage_dir}}'
  @[ -s '{{instance_secret_file}}' ] || openssl rand -hex 32 > '{{instance_secret_file}}'
  @cat '{{instance_secret_file}}'

# Generates (or reuses) an admin key derived from the local instance secret
generate-admin-key:
  @just init-instance-secret > /dev/null
  @[ -s '{{admin_key_file}}' ] || cargo run --quiet --bin generate_key -- {{instance_name}} "$(cat '{{instance_secret_file}}')" 2>/dev/null > '{{admin_key_file}}'
  @cat '{{admin_key_file}}'
