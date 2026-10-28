# Private Demos E2E Tests

Playwright tests for private demo projects against a local
`convex-local-backend`.

```bash
just test-private-demos         # build backend, install deps, run all tests
just test-private-demos-only    # rerun tests without rebuilding or reinstalling
```

## Adding tests

1. Add a mapping in `playwright.config.ts` (`projectMap`) and `run-e2e-tests.sh`
   (`project_dir_for`) pointing to the demo's directory.
2. Add a corresponding npm script in `package.json`.
3. Create a directory under `tests/<project-name>/` with Playwright spec files.
   The `PROJECT` env var selects which project to test.
