# Convex Auth

Official docs: https://docs.convex.dev/auth/convex-auth Setup guide:
https://labs.convex.dev/auth/setup

Use this when the user wants auth handled directly in Convex rather than through
a third-party provider.

## Workflow

1. Confirm the user wants Convex Auth specifically
2. Determine which sign-in methods the app needs:
   - magic links or OTPs
   - OAuth providers
   - passwords and password reset
3. Ask whether the user wants local-only setup or production-ready setup now
4. Read the Convex Auth setup guide before writing code
5. Make sure the project has a configured Convex deployment:
   - run `npx convex dev` first if `CONVEX_DEPLOYMENT` is not set
   - if CLI configuration requires interactive human input, stop and ask the
     user to complete that step before continuing
6. Install the auth packages:
   - `npm install @convex-dev/auth @auth/core@0.37.0`
7. Run the initialization command:
   - `npx @convex-dev/auth`
8. Confirm the initializer created:
   - `convex/auth.config.ts`
   - `convex/auth.ts`
   - `convex/http.ts`
9. Add the required `authTables` to `convex/schema.ts`
10. Replace plain `ConvexProvider` wiring with `ConvexAuthProvider`
11. Configure at least one auth method in `convex/auth.ts`
12. Run `npx convex dev --once` or the normal dev flow to push the updated
    schema and generated code
13. Verify the client can sign in successfully
14. Verify Convex receives authenticated identity in backend functions
15. If the user wants production-ready setup, make sure the same auth setup is
    configured for the production deployment as well
16. Only add a `users` table and `storeUser` flow if the app needs app-level
    user records inside Convex

## What This Reference Is For

- choosing Convex Auth as the default provider for a new Convex app
- understanding whether the app wants magic links, OTPs, OAuth, or passwords
- keeping the setup provider-specific while using the official Convex Auth docs
  for identity and authorization behavior

## What To Do

- Read the Convex Auth setup guide before writing setup code
- Follow the setup flow from the docs rather than recreating it from memory
- If the app is new, consider starting from the official starter flow instead of
  hand-wiring everything
- Treat `npx @convex-dev/auth` as a required initialization step for existing
  apps, not an optional extra

## Concrete Steps

1. Install `@convex-dev/auth` and `@auth/core@0.37.0`
2. Run `npx convex dev` if the project does not already have a configured
   deployment
3. If `npx convex dev` blocks on interactive setup, ask the user explicitly to
   finish configuring the Convex deployment
4. Run `npx @convex-dev/auth`
5. Confirm the generated auth setup is present before continuing:
   - `convex/auth.config.ts`
   - `convex/auth.ts`
   - `convex/http.ts`
6. Add `authTables` to `convex/schema.ts`
7. Replace `ConvexProvider` with `ConvexAuthProvider` in the app entry
8. Configure the selected auth methods in `convex/auth.ts`
9. Run `npx convex dev --once` or the normal dev flow so the updated schema and
   auth files are pushed
10. Verify login locally
11. If the user wants production-ready setup, repeat the required auth
    configuration against the production deployment

## Expected Files and Decisions

- `convex/schema.ts`
- frontend app entry such as `src/main.tsx` or the framework-equivalent provider
  file
- generated Convex Auth setup produced by `npx @convex-dev/auth`
- an existing configured Convex deployment, or the ability to create one with
  `npx convex dev`
- `convex/auth.ts` starts with `providers: []` until the app configures actual
  sign-in methods

- Decide whether the user is creating a new app or adding auth to an existing
  app
- For a new app, prefer the official starter flow instead of rebuilding setup by
  hand
- Decide which auth methods the app needs:
  - magic links or OTPs
  - OAuth providers
  - passwords
- Decide whether the user wants local-only setup or production-ready setup now
- Decide whether the app actually needs a `users` table inside Convex, or
  whether provider identity alone is enough

## Gotchas

- Do not assume a specific sign-in method. Ask which methods the app needs
  before wiring UI and backend behavior.
- `npx @convex-dev/auth` is important because it initializes the auth setup,
  including the key material. Do not skip it when adding Convex Auth to an
  existing project.
- `npx @convex-dev/auth` will fail if the project does not already have a
  configured `CONVEX_DEPLOYMENT`.
- `npx convex dev` may require interactive setup for deployment creation or
  project selection. If that happens, ask the user explicitly for that human
  step instead of guessing.
- `npx @convex-dev/auth` does not finish the whole integration by itself. You
  still need to add `authTables`, swap in `ConvexAuthProvider`, and configure at
  least one auth method.
- A project can still build even if `convex/auth.ts` still has `providers: []`,
  so do not treat a successful build as proof that sign-in is fully configured.
- Convex Auth does not mean every app needs a `users` table. If the app only
  needs authentication gates, `ctx.auth.getUserIdentity()` may be enough.
- If the app is greenfield, starting from the official starter flow is usually
  better than partially recreating it by hand.
- Do not stop at local dev setup if the user expects production-ready auth. The
  production deployment needs the auth setup too.
- Keep provider-specific setup and Convex Auth authorization behavior in the
  official docs instead of inventing shared patterns from memory.

## Production

- Ask whether the user wants dev-only setup or production-ready setup
- If the answer is production-ready, make sure the auth configuration is applied
  to the production deployment, not just the dev deployment
- Verify production-specific redirect URLs, auth method configuration, and
  deployment settings before calling the task complete
- Do not silently write a notes file into the repo by default. If the user wants
  rollout or handoff docs, create one explicitly.

## Human Handoff

If `npx convex dev` or deployment setup requires human input:

- stop and explain exactly what the user needs to do
- say why that step is required
- resume the auth setup immediately after the user confirms it is done

## Validation

- Verify the user can complete a sign-in flow
- Offer to validate sign up, sign out, and sign back in with the configured auth
  method
- If browser automation is available in the environment, you can do this
  directly
- If browser automation is not available, give the user a short manual
  validation checklist instead
- Verify `ctx.auth.getUserIdentity()` returns an identity in protected backend
  functions
- Verify protected UI only renders after Convex-authenticated state is ready
- Verify environment variables and redirect settings match the current app
  environment
- Verify `convex/auth.ts` no longer has an empty `providers: []` configuration
  once the app is meant to support real sign-in
- Run `npx convex dev --once` or the normal dev flow after setup changes and
  confirm Convex codegen and push succeed
- If production-ready setup was requested, verify the production deployment is
  also configured correctly

## Checklist

- [ ] Confirm the user wants Convex Auth specifically
- [ ] Ask whether the user wants local-only setup or production-ready setup
- [ ] Ensure a Convex deployment is configured before running auth
      initialization
- [ ] Install `@convex-dev/auth` and `@auth/core@0.37.0`
- [ ] Run `npx convex dev` first if needed
- [ ] Run `npx @convex-dev/auth`
- [ ] Confirm `convex/auth.config.ts`, `convex/auth.ts`, and `convex/http.ts`
      were created
- [ ] Follow the setup guide for package install and wiring
- [ ] Add `authTables` to `convex/schema.ts`
- [ ] Replace `ConvexProvider` with `ConvexAuthProvider`
- [ ] Configure at least one auth method in `convex/auth.ts`
- [ ] Run `npx convex dev --once` or the normal dev flow after setup changes
- [ ] Confirm which sign-in methods the app needs
- [ ] Verify the client can sign in and the backend receives authenticated
      identity
- [ ] Offer end-to-end validation of sign up, sign out, and sign back in
- [ ] If requested, configure the production deployment too
- [ ] Only add extra `users` table sync if the app needs app-level user records
