# GIPHY Action Example App

This example app demonstrates how to use Convex actions to call into third-party
services and how to use environment variables.

It allows the user to type a chat message, like `/giphy wombat`, query
[GIPHY](https://giphy.com/) for a wombat GIF, and have an animated GIF of a
wombat show up in the chat stream. It builds on the Convex
[tutorial](https://github.com/convex/convex/tree/main/npm-packages/demos/tutorial).

## Running the App

### 1. Configure a deployment

```sh
npm install
npx convex init
```

### 2. Set the `GIPHY_KEY` variable

Create a GIPHY [developer account](https://developers.giphy.com) and obtain a
free API app key on the
[developer dashboard](https://developers.giphy.com/dashboard/). This app
declares `GIPHY_KEY` in `convex/convex.config.ts`, so the deployment won't
accept code until it is set. Run this and paste the key at the prompt, which
keeps it hidden and out of your shell history:

```sh
npx convex env set GIPHY_KEY
```

You can also set it from your [Convex dashboard](https://dashboard.convex.dev/).
See
[environment variables](https://docs.convex.dev/using/environment-variables).

### 3. Start dev

```sh
npm run dev
```

Then visit [localhost:3000](http://localhost:3000).
