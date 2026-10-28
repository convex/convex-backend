---
name: convex-setup-auth
description: Sets up Convex authentication with user management, identity mapping, and access control. Use this skill when adding login or signup to a Convex app, configuring Convex Auth, Clerk, WorkOS AuthKit, Auth0, or custom JWT providers, wiring auth.config.ts, protecting queries and mutations with ctx.auth.getUserIdentity(), creating a users table with identity mapping, or setting up role-based access control, even if the user just says "add auth" or "make it require login."
---

# Convex Authentication Setup

Implement secure authentication in Convex with user management and access control.

## When to Use

- Setting up authentication for the first time
- Implementing user management (users table, identity mapping)
- Creating authentication helper functions
- Setting up auth providers (Convex Auth, Clerk, WorkOS AuthKit, Auth0, custom JWT)

## When Not to Use

- Auth for a non-Convex backend
- Pure OAuth/OIDC documentation without a Convex implementation
- Debugging unrelated bugs that happen to surface near auth code
- The auth provider is already fully configured and the user only needs a one-line fix

## First Step: Choose the Auth Provider

Convex supports multiple authentication approaches. Do not assume a provider.

Before writing setup code:

1. Ask the user which auth solution they want, unless the repository already makes it obvious
2. If the repo already uses a provider, continue with that provider unless the user wants to switch
3. If the user has not chosen a provider and the repo does not make it obvious, ask before proceeding

Common options:

- [Convex Auth](https://docs.convex.dev/auth/convex-auth) - good default when the user wants auth handled directly in Convex
- [Clerk](https://docs.convex.dev/auth/clerk) - use when the app already uses Clerk or the user wants Clerk's hosted auth features
- [WorkOS AuthKit](https://docs.convex.dev/auth/authkit/) - use when the app already uses WorkOS or the user wants AuthKit specifically
- [Auth0](https://docs.convex.dev/auth/auth0) - use when the app already uses Auth0
- Custom JWT provider - use when integrating an existing auth system not covered above

Look for signals in the repo before asking:

- Dependencies such as `@clerk/*`, `@workos-inc/*`, `@auth0/*`, or Convex Auth packages
- Existing files such as `convex/auth.config.ts`, auth middleware, provider wrappers, or login components
- Environment variables that clearly point at a provider

## After Choosing a Provider

Read the provider's official guide and the matching local reference file:

- Convex Auth: [official docs](https://docs.convex.dev/auth/convex-auth), then `references/convex-auth.md`
- Clerk: [official docs](https://docs.convex.dev/auth/clerk), then `references/clerk.md`
- WorkOS AuthKit: [official docs](https://docs.convex.dev/auth/authkit/), then `references/workos-authkit.md`
- Auth0: [official docs](https://docs.convex.dev/auth/auth0), then `references/auth0.md`

The local reference files contain the concrete workflow, expected files and env vars, gotchas, and validation checks.

Use those sources for:

- package installation
- client provider wiring
- environment variables
- `convex/auth.config.ts` setup
- login and logout UI patterns
- framework-specific setup for React, Vite, or Next.js

For shared auth behavior, use the official Convex docs as the source of truth:

- [Auth in Functions](https://docs.convex.dev/auth/functions-auth) for `ctx.auth.getUserIdentity()`
- [Storing Users in the Convex Database](https://docs.convex.dev/auth/database-auth) for optional app-level user storage
- [Authentication](https://docs.convex.dev/auth) for general auth and authorization guidance
- [Convex Auth Authorization](https://labs.convex.dev/auth/authz) when the provider is Convex Auth

Prefer official docs over recalled steps, because provider CLIs and Convex Auth internals change between versions. Inventing setup from memory risks outdated patterns.
For third-party providers, only add app-level user storage if the app actually needs user documents in Convex. Not every app needs a `users` table.
For Convex Auth, follow the Convex Auth docs and built-in auth tables rather than adding a parallel `users` table plus `storeUser` flow, because Convex Auth already manages user records internally.
After running provider initialization commands, verify generated files and complete the post-init wiring steps the provider reference calls out. Initialization commands rarely finish the entire integration.

## Core Pattern: Protecting Backend Functions

The most common auth task is checking identity in Convex functions.

```ts
// Bad: trusting a client-provided userId
export const getMyProfile = query({
  args: { userId: v.id("users") },
  handler: async (ctx, args) => {
    return await ctx.db.get(args.userId);
  },
});
```

```ts
// Good: verifying identity server-side
export const getMyProfile = query({
  args: {},
  handler: async (ctx) => {
    const identity = await ctx.auth.getUserIdentity();
    if (!identity) throw new Error("Not authenticated");

    return await ctx.db
      .query("users")
      .withIndex("by_tokenIdentifier", (q) =>
        q.eq("tokenIdentifier", identity.tokenIdentifier),
      )
      .unique();
  },
});
```

## Workflow

1. Determine the provider, either by asking the user or inferring from the repo
2. Ask whether the user wants local-only setup or production-ready setup now
3. Read the matching provider reference file
4. Follow the official provider docs for current setup details
5. Follow the official Convex docs for shared backend auth behavior, user storage, and authorization patterns
6. Only add app-level user storage if the docs and app requirements call for it
7. Add authorization checks for ownership, roles, or team access only where the app needs them
8. Verify login state, protected queries, environment variables, and production configuration if requested

If the flow blocks on interactive provider or deployment setup, ask the user explicitly for the exact human step needed, then continue after they complete it.
For UI-facing auth flows, offer to validate the real sign-up or sign-in flow after setup is done.
If the environment has browser automation tools, you can use them.
If it does not, give the user a short manual validation checklist instead.

## Reference Files

### Provider References

- `references/convex-auth.md`
- `references/clerk.md`
- `references/workos-authkit.md`
- `references/auth0.md`

## Checklist

- [ ] Chosen the correct auth provider before writing setup code
- [ ] Read the relevant provider reference file
- [ ] Asked whether the user wants local-only setup or production-ready setup
- [ ] Used the official provider docs for provider-specific wiring
- [ ] Used the official Convex docs for shared auth behavior and authorization patterns
- [ ] Only added app-level user storage if the app actually needs it
- [ ] Did not invent a cross-provider `users` table or `storeUser` flow for Convex Auth
- [ ] Added authentication checks in protected backend functions
- [ ] Added authorization checks where the app actually needs them
- [ ] Clear error messages ("Not authenticated", "Unauthorized")
- [ ] Client auth provider configured for the chosen provider
- [ ] If requested, production auth setup is covered too
