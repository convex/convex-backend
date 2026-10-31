# Building docker images

The contents of this directory are used to build the docker images for the
self-hosted backend and dashboard. If you're looking for ways to run self-hosted
Convex, see the [these instructions](../README.md). You may build the images
locally from here, but we recommend using the images we provide on GHCR.

Build the backend from scratch by running:

```sh
docker build -t convex-backend -f self-hosted/docker-build/Dockerfile.backend .
```

Build the dashboard from scratch by running:

```sh
docker build -t convex-dashboard -f self-hosted/docker-build/Dockerfile.dashboard .
```

The backend image stores temporary files under `$DATA_DIR/tmp` by default.
Before starting the backend, it removes stale `.tmp*` entries from that default
directory. An explicitly set `TMPDIR` is left alone. Cleanup is also skipped if
that directory contains the configured storage, SQLite database, or credentials
path. Set `CLEAN_TMPDIR_ON_START=0` to disable startup cleanup. Each data
directory must belong to one backend process; do not share it with another
running backend. A cleanup failure logs a warning with the number of failed
entries and allows the backend to start. If a configured path cannot be
resolved, the sweep is skipped with a warning.

Run the entrypoint regression tests without building the backend:

```sh
python3 self-hosted/docker-build/test_run_backend.py
```
