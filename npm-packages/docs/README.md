# Convex Docs

This website is built using [Docusaurus 2](https://docusaurus.io/), a modern
static website generator.

## Local Development

```sh
just install-js
npm run dev
```

This command starts a local dev server and opens up a browser window. Most
changes are reflected live without having to restart the server.

If you make changes to the `convex` NPM package and want to see them reflected
in API docs, run `just turbo run build --filter=convex...` and restart the
server.

The command runs `npm run dev`, which will not run all checks in our presubmits.
For example, broken links are not checked. To view all errors, try building and
testing:

```sh
npm run test
npm run build
```

## llms.txt

This is a file that was manually generated using Firecrawl:
https://www.firecrawl.dev/blog/How-to-Create-an-llms-txt-File-for-Any-Website

You need to get an API key from Firecrawl and follow the instructions on that
blog post above.

I then did a few manual edits:

- Removed all Google Analytics references (simple regex find and replace)
- Put the home page text at the top
- Cleaned up youtube embeds output they were pretty messy.

Otherwise it generated reasonably decent output. We should eventually make this
more automated with every publish.

See
[here](https://linear.app/convex/issue/DX-1412/create-an-llmstxt-file-for-the-website-and-docs-page).
For the full background.

## Spell-checking in VS Code

You can enable spell checking in VS Code by installing
[Code Spell Checker](https://marketplace.visualstudio.com/items?itemName=streetsidesoftware.code-spell-checker).

## Build

```sh
npm run build
```

This command generates static content into the `build` directory and can be
served using any static contents hosting service.

## Deploying to production

See [here](/ops/services/docs/release.md).

## Preview Deployment

See [here](/ops/services/docs/release.md#preview-deployment).

## Updating the Component docs

The Component docs are in the /docs folder of the Components that we keep in
sync. For example, https://github.com/convex/component/tree/main/docs To update
them, run the following command in this directory:

```sh
node scripts/pull-component-docs.js
```

This will pull the latest docs from the `main` branch and update the docs
directory, doing some replacing of relative links back to the original repo for
code snippets.

This is a manual process and generally only needs to be done when the component
docs change and there is a new release of the component package.

# Dependency notes

These typedoc plugins are forked because they didn't work when the monorepo used
Rush. FIXME: they might work unforked now that this is a plain pnpm workspace.

We needed to update a couple, so we forked them at
https://github.com/convex/typedoc-plugin-markdown

Iterating on typedoc plugins is rough, typedoc implements their own module
resolution such that our pnpm workspace setup doesn't work. So to iterate I

1. cloned our typedoc-plugin-markdown fork and set an override in
   pnpm-workspace.yaml (overrides)
2. make changes there and did a build with yarn run build
3. removed the dependency from dashboard's package.json
4. just update-js
5. re-added the dependency to dashboard's package.json
6. just update-js
7. repeat from 2.
8. remove the globalOverridel, increment the typedoc-plugin-markdown version
   number and publish, and update docs package.json deps
