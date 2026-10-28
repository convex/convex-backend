# local_backend

## OpenAPI specs

HTTP handlers annotated with `#[utoipa::path(...)]` feed three OpenAPI doc roots
in `src/router.rs`, each with a checked-in spec:

| Doc root          | Served at                     | Checked-in spec                                                    |
| ----------------- | ----------------------------- | ------------------------------------------------------------------ |
| `PlatformApiDoc`  | `/api/v1/openapi.json`        | `npm-packages/@convex-dev/platform/deployment-openapi.json`        |
| `PublicApiDoc`    | `/api/public_openapi.json`    | `npm-packages/@convex-dev/platform/public-deployment-openapi.json` |
| `DashboardApiDoc` | `/api/dashboard_openapi.json` | `npm-packages/dashboard/dashboard-deployment-openapi.json`         |

After changing anything that feeds the specs — a route, its
`#[utoipa::path(...)]` annotation, or a docstring baked into a description — run
`just generate-api-specs`, or `test_api_specs_match` will fail. Commit the
regenerated JSON specs and TypeScript clients alongside the Rust change.
