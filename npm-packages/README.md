## About

npm-packages contains the cli, demos, tutorials, tests, scripts and more. We use
pnpm workspaces for package management, so don't run `npm install` in these
directories. See the main convex/README.md file for details.

## Adding a new convex package

To add a new package that uses convex, follow these steps instead of our
quickstart:

1. Create a new subdirectory
2. Copy the contents of the npm-packages/tutorial into the subdirectory
3. Update `"name"` in your new `package.json` file to match your subdirectory
   name
4. Add your new package to
   [pnpm-workspace.yaml](https://github.com/convex/convex/blob/main/npm-packages/pnpm-workspace.yaml)
5. Run `just update-js` to install dependencies

## Running a convex package

You can run projects against either a local backend, or against prod.

If your project requires changes that have not yet been deployed, you'll need to
test against a local backend.

To use a local backend:

1. Run `just run-local-backend`
2. In a separate terminal window, run `just convex dev`

To use a local big-brain (runs both backend and big brain locally):

1. Run `just run-big-brain`
2. Copy the `Test with:` line output by big-brain:

`2023-07-12T19:07:07.422400Z INFO big_brain::model: Test with: CONVEX_PROVISION_HOST=http://0.0.0.0:8050 npx convex dev --override-auth-url "https://cheerful-lake-55-staging.authkit.app/" " --override-auth-client "XXXXXXXXX"`

3. Paste it into another terminal that's in your convex directory.

To run against prod:

Just run `npx convex dev`

## Updating the API spec

The OpenAPI spec for the Platform APIs in `@convex-dev/platform` is
automatically generated from the Rust code.

### Management API specs (big_brain)

Run `cargo test -p big_brain test_api_specs_match` to rebuild the management API
specs, `cargo test -p local_backend test_api_specs_match` to rebuild the
deployment API specs, and `npm run generateApiSpec` to rebuild the clients.

This updates:

- `management-openapi.json` - Platform management API
- `dashboard-management-openapi.json` - Dashboard management API

### Deployment API specs (local_backend)

Run `cargo test -p local_backend test_api_specs_match` to rebuild the deployment
API specs.

This updates:

- `deployment-openapi.json` - Public deployment API (queries, mutations,
  actions)
- `dashboard-deployment-openapi.json` - Dashboard deployment API (admin
  operations)
