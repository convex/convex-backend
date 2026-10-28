# Dall-E Storage & Action Example App

This example app demonstrates how to use
[Convex storage](https://docs.convex.dev/using/file-storage) and
[actions](https://docs.convex.dev/using/actions) together to save an image in
Convex that you download in an action. By default, Dall-E only generates images
that last an hour, so to maintain access to the image, we
[store the image in Convex](./convex/sendDallE.js) and store the `storageId`
[with the message](./convex/sendMessage.js). To show the image, we turn the
`storageId` into a url to the Convex-hosted image
[on demand](./convex/listMessages.js).

It allows the user to type a chat message, like `/dall-e cute cat`, and have it
send a dall-e generated image of a cute cat in the chat. wombat show up in the
chat stream. It builds on the Convex
[tutorial](https://github.com/convex/convex/tree/main/npm-packages/demos/tutorial).

## Running the App

### 1. Configure a deployment

```sh
npm install
npx convex init
```

### 2. Set the `OPENAI_API_KEY` variable

Create a free account on openai.com and create your
[OpenAI API secret key](https://beta.openai.com/account/api-keys). This app
declares `OPENAI_API_KEY` in `convex/convex.config.ts`, so the deployment won't
accept code until it is set. Run this and paste the key at the prompt, which
keeps it hidden and out of your shell history:

```sh
npx convex env set OPENAI_API_KEY
```

You can also set it from your [Convex dashboard](https://dashboard.convex.dev/).
See
[environment variables](https://docs.convex.dev/using/environment-variables).

### 3. Start dev

```sh
npm run dev
```

Then visit [localhost:3000](http://localhost:3000).
