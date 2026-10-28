# Vector Search Example App

This example demonstrates how to use
[Convex vector search](https://docs.convex.dev/vector-search).

It has a "Food search" and "Movie search". The "Food search" shows the simplest
way to set up a vector search, and matches the Convex
[documentation](https://docs.convex.dev/vector-search).

The "Movies search" shows some alternative pattens documented
[here](https://docs.convex.dev/vector-search#advanced-patterns).

## Running the App

### 1. Configure a deployment

```sh
npm install
npx convex init
```

### 2. Set the `OPENAI_KEY` variable

This app uses OpenAI to generate embeddings, and declares `OPENAI_KEY` in
`convex/convex.config.ts`, so the deployment won't accept code until it is set.
Run this and paste your OpenAI API key at the prompt, which keeps it hidden and
out of your shell history:

```sh
npx convex env set OPENAI_KEY
```

You can also set it from your [Convex dashboard](https://dashboard.convex.dev/).
See
[environment variables](https://docs.convex.dev/production/environment-variables).

### 3. Start dev

```sh
npm run dev
```
