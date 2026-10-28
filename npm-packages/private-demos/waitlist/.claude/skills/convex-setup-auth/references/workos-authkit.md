# WorkOS AuthKit

Official docs:

- https://docs.convex.dev/auth/authkit/
- https://docs.convex.dev/auth/authkit/add-to-app
- https://docs.convex.dev/auth/authkit/auto-provision

Use this when the app already uses WorkOS or the user wants AuthKit
specifically.

## Workflow

1. Confirm the user wants WorkOS AuthKit
2. Determine whether they want:
   - a Convex-managed WorkOS team
   - an existing WorkOS team
3. Ask whether the user wants local-only setup or production-ready setup now
4. Read the official Convex and WorkOS AuthKit guide
5. Create or update `convex.json` for the app's framework and real local port
6. Follow the correct branch of the setup flow based on that choice
7. Configure the required WorkOS environment variables
8. Configure `convex/auth.config.ts` for WorkOS-issued JWTs
9. Wire the client provider and callback flow
10. Verify authenticated requests reach Convex
11. If the user wants production-ready setup, make sure the production WorkOS
    configuration is covered too
12. Only add `storeUser` or a `users` table if the app needs first-class user
    rows inside Convex

## What To Do

- Read the official Convex and WorkOS AuthKit guide before writing setup code
- Determine whether the user wants a Convex-managed WorkOS team or an existing
  WorkOS team
- Treat `convex.json` as a first-class part of the AuthKit setup, not an
  optional extra
- Follow the current setup flow from the docs instead of relying on older
  examples

## Key Setup Areas

- package installation for the app's framework
- `convex.json` with the `authKit` section for dev, and preview or prod if
  needed
- environment variables such as `WORKOS_CLIENT_ID`, `WORKOS_API_KEY`, and
  redirect configuration
- `convex/auth.config.ts` wiring for WorkOS-issued JWTs
- client provider setup and token flow into Convex
- login callback and redirect configuration

## Files and Env Vars To Expect

- `convex.json`
- `convex/auth.config.ts`
- frontend auth provider wiring
- callback or redirect route setup where the framework requires it
- WorkOS environment variables commonly include:
  - `WORKOS_CLIENT_ID`
  - `WORKOS_API_KEY`
  - `WORKOS_COOKIE_PASSWORD`
  - `VITE_WORKOS_CLIENT_ID`
  - `VITE_WORKOS_REDIRECT_URI`
  - `NEXT_PUBLIC_WORKOS_REDIRECT_URI`

For a managed WorkOS team, `convex dev` can provision the AuthKit environment
and write local env vars such as `VITE_WORKOS_CLIENT_ID` and
`VITE_WORKOS_REDIRECT_URI` into `.env.local` for Vite apps.

## Concrete Steps

1. Choose Convex-managed or existing WorkOS team
2. Create or update `convex.json` with the `authKit` section for the framework
   in use
3. Make sure the dev `redirectUris`, `appHomepageUrl`, `corsOrigins`, and local
   redirect env vars match the app's actual local port
4. For a managed WorkOS team, run `npx convex dev` and follow the interactive
   onboarding flow
5. For an existing WorkOS team, get `WORKOS_CLIENT_ID` and `WORKOS_API_KEY` from
   the WorkOS dashboard and set them with `npx convex env set`
6. Create or update `convex/auth.config.ts` for WorkOS JWT validation
7. Run the normal Convex dev or deploy flow so backend config is synced
8. Wire the WorkOS client provider in the app
9. Configure callback and redirect handling
10. Verify the user can sign in and return to the app
11. Verify Convex sees the authenticated user after login
12. If the user wants production-ready setup, configure the production client
    ID, API key, redirect URI, and deployment settings too

## Gotchas

- The docs split setup between Convex-managed and existing WorkOS teams, so ask
  which path the user wants if it is not obvious
- Keep dev and prod WorkOS configuration separate where the docs call for
  different client IDs or API keys
- Only add `storeUser` or a `users` table if the app needs first-class user rows
  inside Convex
- Do not mix dev and prod WorkOS credentials or redirect URIs
- If the repo already contains WorkOS setup, preserve the current tenant model
  unless the user wants to change it
- For managed WorkOS setup, `convex dev` is interactive the first time. In
  non-interactive terminals, stop and ask the user to complete the onboarding
  prompts.
- `convex.json` is not optional for the managed AuthKit flow. It drives redirect
  URI, homepage URL, CORS configuration, and local env var generation.
- If the frontend starts on a different port than the one in `convex.json`, the
  hosted WorkOS sign-in flow will point to the wrong callback URL. Update
  `convex.json`, update the local redirect env var, and run `npx convex dev`
  again.
- Vite can fall off `5173` if other apps are already running. Do not assume the
  default port still matches the generated AuthKit config.
- A successful WorkOS sign-in should redirect back to the local callback route
  and then reach a Convex-authenticated state. Do not stop at "the hosted WorkOS
  page loaded."

## Production

- Ask whether the user wants dev-only setup or production-ready setup
- If the answer is production-ready, make sure the production WorkOS client ID,
  API key, redirect URI, and Convex deployment config are all covered
- Verify the production redirect and callback settings before calling the task
  complete
- Do not silently write a notes file into the repo by default. If the user wants
  rollout or handoff docs, create one explicitly.

## Validation

- Verify the user can complete the login flow and return to the app
- Verify the callback URL matches the real frontend port in local dev
- Verify Convex receives authenticated requests after login
- Verify `convex.json` matches the framework and chosen WorkOS setup path
- Verify `convex/auth.config.ts` matches the chosen WorkOS setup path
- Verify environment variables differ correctly between local and production
  where needed
- If production-ready setup was requested, verify the production WorkOS
  configuration is also covered

## Checklist

- [ ] Confirm the user wants WorkOS AuthKit
- [ ] Ask whether the user wants local-only setup or production-ready setup
- [ ] Choose Convex-managed or existing WorkOS team
- [ ] Create or update `convex.json`
- [ ] Configure WorkOS environment variables
- [ ] Configure `convex/auth.config.ts`
- [ ] Verify authenticated requests reach Convex after login
- [ ] If requested, configure the production deployment too
