# Users and Authentication Example App

This example demonstrates how to add users and authentication to a basic chat
app. It uses [Clerk](https://clerk.dev/) for authentication.

Users are initially presented with a "Log In" button. After user's sign up,
their information is persisted to a `users` table via a webhook. When users send
messages, each message is associated with the user that sent it. Lastly, users
can log out with a "Log Out" button.

## Running the App

This app declares `CLERK_JWT_ISSUER_DOMAIN` and `CLERK_WEBHOOK_SECRET` in
`convex/convex.config.ts`, so the deployment won't accept code until both are
set. Configure a deployment first:

```sh
npm install
npx convex init
```

### Set up Clerk credentials

Follow the instructions in https://docs.convex.dev/auth/clerk#get-started to
obtain:

- A _publishable key_, set it as `VITE_CLERK_PUBLISHABLE_KEY` in `.env.local`.
- A JWT template _Issuer URL_, set it as `CLERK_JWT_ISSUER_DOMAIN` on your
  Convex dashboard.

You still need to set up one more variable.

### Setting up webhooks in Clerk

On your Clerk dashboard, go to _Webhooks_, click on _+ Add Endpoint_.

Set _Endpoint URL_ to
`https://<your deployment name>.convex.site/clerk-users-webhook`. You can see
your deployment name in `.env.local` in this directory. For example, the
endpoint URL could be:
`https://ardent-mammoth-732.convex.site/clerk-users-webhook`.

In _Message Filtering_, select **user** for all user events (scroll down or use
the search input).

Click on _Create_.

After the endpoint is saved, copy the _Signing Secret_ (on the right side of the
UI), it should start with `whsec_`. Set it as the value of the
`CLERK_WEBHOOK_SECRET` environment variable in your Convex dashboard.

### Start dev

```sh
npm run dev
```

From now on, when a user signs up you should see logs from the HTTP handler as
well as a new row in the users table in the Convex dashboard.

### Debugging webhooks

If your setup wasn't correct and you signed in already, Clerk will not sent
another user creation event to your webhook endpoint. To repeat the webhook flow
from initial sign up, go to the Clerk dashboard, click on _Users_, find your
user and delete it from the _..._ menu.
