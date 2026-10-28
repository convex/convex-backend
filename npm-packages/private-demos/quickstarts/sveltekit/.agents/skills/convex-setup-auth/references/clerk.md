# Clerk

Official docs:

- https://docs.convex.dev/auth/clerk
- https://clerk.com/docs/guides/development/integrations/databases/convex

Use this when the app already uses Clerk or the user wants Clerk's hosted auth features.

## Workflow

1. Confirm the user wants Clerk
2. Make sure the user has a Clerk account and a Clerk application
3. Determine the app framework:
   - React
   - Next.js
   - TanStack Start
4. Ask whether the user wants local-only setup or production-ready setup now
5. Gather the Clerk keys and the Clerk Frontend API URL
6. Follow the correct framework section in the official docs
7. Complete the backend and client wiring
8. Verify Convex reports the user as authenticated after login
9. If the user wants production-ready setup, make sure the production Clerk config is also covered

## What To Do

- Read the official Convex and Clerk guide before writing setup code
- If the user does not already have Clerk set up, send them to `https://dashboard.clerk.com/sign-up` to create an account and `https://dashboard.clerk.com/apps/new` to create an application
- Send the user to `https://dashboard.clerk.com/apps/setup/convex` if the Convex integration is not already active
- Match the guide to the app's framework, usually React, Next.js, or TanStack Start
- Use the official examples for `ConvexProviderWithClerk`, `ClerkProvider`, and `useAuth`

## Key Setup Areas

- install the Clerk SDK for the framework in use
- configure `convex/auth.config.ts` with the Clerk issuer domain
- set the required Clerk environment variables
- wrap the app with `ClerkProvider` and `ConvexProviderWithClerk`
- use Convex auth-aware UI patterns such as `Authenticated`, `Unauthenticated`, and `AuthLoading`

## Files and Env Vars To Expect

- `convex/auth.config.ts`
- React or Vite client entry such as `src/main.tsx`
- Next.js client wrapper for Convex if using App Router
- Clerk account sign-up page: `https://dashboard.clerk.com/sign-up`
- Clerk app creation page: `https://dashboard.clerk.com/apps/new`
- Clerk Convex integration page: `https://dashboard.clerk.com/apps/setup/convex`
- Clerk API keys page: `https://dashboard.clerk.com/last-active?path=api-keys`
- Clerk environment variables:
  - `CLERK_JWT_ISSUER_DOMAIN` for Convex backend validation in the Convex docs
  - `CLERK_FRONTEND_API_URL` in the Clerk docs
  - `VITE_CLERK_PUBLISHABLE_KEY` for Vite apps
  - `NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY` for Next.js apps
  - `CLERK_SECRET_KEY` for Next.js server-side Clerk setup where required

`CLERK_JWT_ISSUER_DOMAIN` and `CLERK_FRONTEND_API_URL` refer to the same Clerk Frontend API URL value. Do not treat them as two different URLs.

## Concrete Steps

1. If needed, create a Clerk account at `https://dashboard.clerk.com/sign-up`
2. If needed, create a Clerk application at `https://dashboard.clerk.com/apps/new`
3. Open `https://dashboard.clerk.com/last-active?path=api-keys` and copy the publishable key, plus the secret key for Next.js where needed
4. Open `https://dashboard.clerk.com/apps/setup/convex`
5. Activate the Convex integration in Clerk if it is not already active
6. Copy the Clerk Frontend API URL shown there
7. Install the Clerk package for the app's framework
8. Create or update `convex/auth.config.ts` so Convex validates Clerk tokens
9. Set the publishable key in the frontend environment
10. Set the issuer domain or Frontend API URL so Convex can validate the JWT
11. Replace plain `ConvexProvider` wiring with `ConvexProviderWithClerk`
12. Wrap the app in `ClerkProvider`
13. Use Convex auth helpers for authenticated rendering
14. Run the normal Convex dev or deploy flow after updating backend auth config
15. If the user wants production-ready setup, configure the production Clerk values and production issuer domain too

## Gotchas

- Prefer `useConvexAuth()` over raw Clerk auth state when deciding whether Convex-authenticated UI can render
- For Next.js, keep server and client boundaries in mind when creating the Convex provider wrapper
- After changing `convex/auth.config.ts`, run the normal Convex dev or deploy flow so the backend picks up the new config
- Do not stop at "Clerk login works". The important check is that Convex also sees the session and can authenticate requests.
- If the repo already uses Clerk, preserve its existing auth flow unless the user asked to change it.
- Do not assume the same Clerk values work for both dev and production. Check the production issuer domain and publishable key separately.
- The Convex setup page is where you get the Clerk Frontend API URL for Convex. Keep using the Clerk API keys page for the publishable key and the secret key.
- If Convex says no auth provider matched the token, first confirm the Clerk Convex integration was activated at `https://dashboard.clerk.com/apps/setup/convex`
- After activating the Clerk Convex integration, sign out completely and sign back in before retesting. An old Clerk session can keep using a token that Convex rejects.

## Production

- Ask whether the user wants dev-only setup or production-ready setup
- If the answer is production-ready, make sure production Clerk keys and issuer configuration are included
- Verify production redirect URLs and any production Clerk domain values before calling the task complete
- Do not silently write a notes file into the repo by default. If the user wants rollout or handoff docs, create one explicitly.

## Validation

- Verify the user can sign in with Clerk
- If the Clerk integration was just activated, verify after a full Clerk sign-out and fresh sign-in
- Verify `useConvexAuth()` reaches the authenticated state after Clerk login
- Verify protected Convex queries run successfully inside authenticated UI
- Verify `ctx.auth.getUserIdentity()` is non-null in protected backend functions
- If production-ready setup was requested, verify the production Clerk configuration is also covered

## Checklist

- [ ] Confirm the user wants Clerk
- [ ] Ask whether the user wants local-only setup or production-ready setup
- [ ] Follow the correct framework section in the official guide
- [ ] Set Clerk environment variables
- [ ] Configure `convex/auth.config.ts`
- [ ] Verify Convex authenticated state after login
- [ ] If requested, configure the production deployment too
