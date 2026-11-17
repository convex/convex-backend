# Changelog

Find the latest versions of the `convex-backend` package
[here](https://github.com/convex/convex-backend/pkgs/container/convex-backend)
and `convex-dashboard` package
[here](https://github.com/convex/convex-backend/pkgs/container/convex-dashboard).
Make sure to use the same version of `convex-backend` and `convex-dashboard`.
Different versions are not guaranteed to be compatible with one another.

Follow the instructions in the [README](README.md#software-upgrades) to upgrade
your self-hosted backend and dashboard.

## 2025-09-17

As of ~2025-09-17, we're releasing more frequently, so we're not going to update
this changelog as frequently. Check `git log` for a detailed listing of changes.

## 2025-09-12 `4d3b0a5de955a258e70e3813bbff486051d88820`

- Schema Validation progress on dashboard.
- Improved error messages
- Misc improvements.

## 2025-09-03 `00bd92723422f3bff968230c94ccdeb8c1719832`

- Fix command line / env var configuration flag issue.
- Support AWS credential handling from multiple sources, including IAM. (thanks
  HeathHopkins)
- Support disabling AWS S3 SSE/Checksums for better compatibility with AWS
  compatible services (thanks jovermier)

## 2025-08-27 `08139ef318b1898dad7731910f49ba631631c902`

- Support index backfill progress and staged indexes
- Fix full text search bug where some filters were ignored
- Build database indexes in parallel

## 2025-08-05 `33cef775a8a6228cbacee4a09ac2c4073d62ed13`

- Add support for `AWS_S3_FORCE_PATH_STYLE` (thanks Squipward00 and cayter),
  allowing support for MinIO and DigitalOcean S3-compatible storage.
- Variety of bug fixes and performance improvements to backend

## 2025-07-01 `6efab6f2b6c182b90255774d747328cfc7b80dd9`

- Add support for integrations (axiom/datadog/sentry/fivetran/airbyte) to
  self-hosted
- Fix node actions bug affecting multiple concurrent requests with local node
  executor
- Variety of performance improvements to backend (caching, memory usage, CPU
  usage)

## 2025-05-29 `c1a7ac393888d743e704de56cf569a154b4526d4`

- Fix bug that prevented folks with crons from upgrading existing older
  self-hosted deployments to `478d197d54ee6e873f06cf9e9deae1eb4aa35bb5`.

## 2025-05-23 `478d197d54ee6e873f06cf9e9deae1eb4aa35bb5`

- Tons and tons of backend improvements.
- MCP + Self-hosting works (requires npm package convex >= 1.24.1)
- Speed up pushing to node actions with
  [external packages](https://docs.convex.dev/functions/bundling#external-packages).
- Support setting `CONVEX_CLOUD_ORIGIN`, `CONVEX_SITE_ORIGIN`, and
  `NEXT_PUBLIC_DEPLOYMENT_URL` in .env next to docker compose file. (Thx
  @natac13)
- Add support for CA file for postgres (thx @tahvane1)
- Docs improvements. Incl DevContainer guide (thx @iamfj).

## 2025-03-10 `5143fec81f146ca67495c12c6b7a15c5802c37e2`

- Speed up Node actions by 50x. No more cold starts on every request. See
  [this commit](https://github.com/convex/convex-backend/commit/6be386a490909dda5b8fb1c12b6cca25326847c6)
  for more details.

## 2025-03-06 `be8a4f397810ce3d04dc3cb32bc81969fe64685a`

- Add R2 compatibility. See https://github.com/convex/convex-backend/pull/53.
  [Docs](https://github.com/convex/convex-backend/blob/main/self-hosted/README.md#using-s3-storage)
- Tolerate missing or malformed sourcemaps (eg. from `ai` npm library).

## 2025-02-26 `161e32648a971fb8ef591e61212f7b9fb7ff4f2c`

- Add support for S3 storage for exports, snapshots, modules, files, and search
  indexes. Read more on how to set up S3 storage
  [here](README.md#using-s3-storage).

## 2025-02-24 `fff8e431b95f4d9fde899ce348f8e8f23210aad3`

- Support streaming import. Read more on how to set up streaming import
  [here](https://docs.convex.dev/production/integrations/streaming-import-export#streaming-import)
- Fix bug in routing to HTTP actions. Read more
  [here](https://github.com/convex/convex-backend/commit/1652ee81d8a01fdeed98b0e4c923a89d1672f8ad).

## 2025-02-19 `86ae5d34c8164075b66fa0c52beabd19212d8df7`

- Fix bug in MySQL where certificates were not verified upon connection. Now,
  you must set `DO_NOT_REQUIRE_SSL` for running locally.

## 2025-02-19 `663640f5a01018914dc4314145f23a31f3afdca6`

- Add support for MySQL! The `DATABASE_URL` env variable is now `POSTGRES_URL`
  or `MYSQL_URL`. Known issue: MySQL certificates are not verified upon
  connection. The next release will include a fix.
- Optimize database queries, so simple mutations get 4x faster when running
  against a Postgres or MySQL db in a different datacenter or region.

## 2025-02-18 `62ef09aa604b0c5f873b59e0944b5e89f84b66b2`

- Add support for running Docker image with local Postgres using the
  `DO_NOT_REQUIRE_SSL` environment variable.

## 2025-02-13 `6c974d219776b753cd23d26f4a296629ff7c2cad`

- Fix a bug where every node action request would write to temporary files that
  were never cleaned up. Caused disk space to fill up on volumes.

## 2025-02-12 `4499dd4fd7f2148687a7774599c613d052950f46`

> ⚠️ **WARNING**: DO NOT use this version in production! This initial release
> contains a critical bug that fills up disk space. Use version
> [6c974d219776b753cd23d26f4a296629ff7c2cad](##6c974d219776b753cd23d26f4a296629ff7c2cad)
> or later.

- Initial release of self-hosted backend and dashboard.
