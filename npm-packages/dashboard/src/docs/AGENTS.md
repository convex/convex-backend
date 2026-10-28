# Documentation Screenshots

This folder contains Storybook stories used to generate screenshots for the docs
site (`npm-packages/docs`).

## Importing screenshots in docs

```tsx
import { Screenshot } from "@site/src/components/Screenshot";

<Screenshot story="docs/pages/project/deployment/Data" alt="…" />

// For a non-default story:
<Screenshot story="docs/pages/project/deployment/Data#Add Document" alt="…" />
```

## Story types

- **`components/`** — Stories that render a single UI component.
- **`pages/`** — Stories that render an entire Next.js dashboard page. These use
  the decorator in
  `npm-packages/dashboard-storybook/.storybook/docsPageDecorator.tsx` which
  provides the page layout and common mocks. If you need to mock something,
  prefer adding it to the decorator so it's reused across all page stories.
  - Stories under `pages/project/` render pages inside a project.
  - Stories under `pages/project/deployment/` render pages inside a deployment.

## Timestamps

Capture runs in a browser pinned to the `en-US` locale and the `UTC` timezone,
so a story only needs to freeze `Date.now()` (in `beforeEach`) for its rendered
times to be identical on every machine. Write mock timestamps as UTC instants
(`new Date("2026-04-01T09:41:00Z").getTime()`) and read them back as UTC when
reviewing a screenshot — the local times you see in `just storybook` are your
own timezone, not what gets captured.

## Cropping screenshots

Page stories can be cropped to only show specific element(s) using
`screenshotSelector`:

```ts
export const Default: Story = {
  parameters: {
    screenshotSelector: '[data-testid="table-context-menu"]',
  },
};
```

A story whose content doesn't fit the default 1024x700 capture viewport can
widen or heighten it with `screenshotViewport: { width, height }`.

### Capturing the command palette

The team and project switchers in the header, and the deployment pill on a
deployment page, all open the command palette. It portals to `document.body`, so
query it through `screen` rather than the story canvas, and crop to the trigger
and the anchored menu together:

```ts
export const TeamSwitcher: Story = {
  parameters: {
    screenshotSelector:
      '[aria-label="Switch team"], .command-palette--anchored',
    // The menu's list is min(330px, 40vh) tall, so the default viewport height
    // clips its last row.
    screenshotViewport: { width: 1024, height: 1000 },
  },
  play: async () => {
    await userEvent.click(await screen.findByLabelText("Switch team"));
    await screen.findByText("Create Team…");
  },
};
```

## Interacting before the screenshot

All stories support a `play` function to interact with elements before the
screenshot is taken:

```ts
import { userEvent, within } from "storybook/test";

export const Default: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      canvas.getByRole("button", { name: "Open project settings" }),
    );
  },
};
```

## Workflow

After any change to stories, run:

```sh
just generate-docs-screenshots
```

Then open the changed `.webp` files to visually verify the screenshots look
correct.

### Regenerating only some screenshots

The command above recaptures every `docs/` story, which is slow. To regenerate
only the stories you changed, pass a case-insensitive substring of the story
title as the **first argument** — only stories whose title contains it are
recaptured:

```sh
# Recapture only stories whose title contains "UsageLimits"
just generate-docs-screenshots UsageLimits

# Narrow further with a path-like substring (matches the story title, which is
# its file path under docs/, e.g. "docs/pages/project/deployment/settings/…")
just generate-docs-screenshots settings/usagelimits
```

The substring is matched against the full story title. Use a distinctive part of
the component or path (e.g. `UsageLimits`, `Data`, `deployment/settings`) so you
don't accidentally match unrelated stories. When a filter is passed, the other
screenshots and their manifest entries are left untouched (no stale cleanup
runs), so it's safe to iterate on one screenshot. Omit the argument to
regenerate everything before committing.
