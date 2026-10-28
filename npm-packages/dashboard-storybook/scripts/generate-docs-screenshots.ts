import * as fs from "node:fs";
import * as http from "node:http";
import * as os from "node:os";
import * as path from "node:path";
import { pathToFileURL } from "node:url";
import { spawn } from "node:child_process";
import { chromium } from "playwright";
import sharp from "sharp";
import pixelmatch from "pixelmatch";
import chalk from "chalk";
import getPort from "get-port";
import ora from "ora";
import serveHandler from "serve-handler";

const SCRIPT_DIR = path.dirname(new URL(import.meta.url).pathname);
const PACKAGE_DIR = path.resolve(SCRIPT_DIR, "..");
const OUTPUT_DIR = path.resolve(
  PACKAGE_DIR,
  "../docs/static/screenshots/storybook",
);
const MANIFEST_PATH = path.resolve(
  PACKAGE_DIR,
  "../docs/src/generated/screenshotManifest.ts",
);
const REPO_DIR = path.resolve(PACKAGE_DIR, "../..");
const BUILD_DIR = path.join(PACKAGE_DIR, "storybook-static");

// Stories render absolute times via `toLocaleString`/`Intl.DateTimeFormat`, and
// timezone abbreviations from the browser's resolved timezone, so pin both to
// keep screenshots identical no matter where they're regenerated.
const LOCALE = "en-US";
const TIMEZONE_ID = "UTC";

const CROP_PADDING = 32; // in (real) pixels
const CROP_PADDING_PAGE = 64; // in (real) pixels, for element crops in page stories
const DEVICE_SCALE_FACTOR = 2;

/** Convert PascalCase to snake_case */
function toSnakeCase(str: string): string {
  return str
    .replace(/([A-Z])/g, (m, c, offset) => (offset > 0 ? "_" : "") + c)
    .toLowerCase();
}

/** Derive output filename from story title, story name, and theme */
function filenameFromTitle(
  title: string,
  storyName: string,
  theme: "light" | "dark",
): string {
  // Strip "docs/" prefix
  const withoutPrefix = title.replace(/^docs\//i, "");
  // Split by "/" and convert each segment to snake_case
  const segments = withoutPrefix.split("/").map(toSnakeCase);
  if (storyName !== "Default") {
    segments.push(storyName.toLowerCase().replace(/\s+/g, "_"));
  }
  return `${segments.join("_")}_${theme}.webp`;
}

/** Run tasks with limited concurrency */
async function runWithConcurrency<T>(
  tasks: (() => Promise<T>)[],
  concurrency: number,
): Promise<T[]> {
  const results: T[] = [];
  let index = 0;
  const workers = Array.from({ length: concurrency }, async () => {
    while (index < tasks.length) {
      const i = index++;
      results[i] = await tasks[i]();
    }
  });
  await Promise.all(workers);
  return results;
}

/**
 * Build the static Storybook that the capture reads from.
 *
 * Going through turbo rather than calling `storybook build` keeps this a no-op
 * when nothing the build reads has changed, and builds the workspace dists it
 * needs on the way (see this package's turbo.json).
 */
async function buildStorybook(): Promise<void> {
  const proc = spawn(
    "just",
    ["turbo", "run", "build", "--filter=dashboard-storybook"],
    { cwd: REPO_DIR, stdio: ["ignore", "pipe", "pipe"] },
  );

  // Buffered rather than inherited: turbo's output would otherwise scroll the
  // spinner away on every run, and it only carries anything worth reading when
  // the build fails, where it is the only description of what went wrong.
  let output = "";
  proc.stdout?.on("data", (chunk) => (output += chunk));
  proc.stderr?.on("data", (chunk) => (output += chunk));

  await new Promise<void>((resolve, reject) => {
    proc.on("error", reject);
    // `close` rather than `exit`, which can fire while output is still buffered.
    proc.on("close", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(
          new Error(`Storybook build exited with code ${code}\n\n${output}`),
        );
      }
    });
  });
}

/** Serve the built Storybook off disk, returns { close } */
async function startStaticServer(port: number): Promise<{ close: () => void }> {
  const server = http.createServer((req, res) =>
    serveHandler(req, res, { public: BUILD_DIR, cleanUrls: false }),
  );

  await new Promise<void>((resolve, reject) => {
    server.on("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });

  return { close: () => server.close() };
}

/**
 * `process.exit()` drops writes still queued on stderr, truncating the failure
 * log when stderr is a pipe (CI). Wait for the queue to flush first.
 */
async function exitWithFailure(): Promise<never> {
  await new Promise<void>((resolve) => {
    process.stderr.write("", () => resolve());
  });
  process.exit(1);
}

// 1. Build the Storybook, then serve it off disk. Capturing against the dev
// server instead would make every page load wait on Vite compiling modules on
// demand, which both slows the run down and delays each story's render enough
// to change what its `play` function races against.
let spinner = ora("Building Storybook...").start();
try {
  await buildStorybook();
} catch (error) {
  spinner.fail("Storybook build failed");
  console.error(error instanceof Error ? error.message : error);
  await exitWithFailure();
}
spinner.succeed("Storybook built");

const port = await getPort();
spinner = ora(`Serving Storybook on port ${port}...`).start();
const { close: closeServer } = await startStaticServer(port);
spinner.succeed(`Storybook served on port ${port}`);

// 2. Fetch stories from the running server
spinner = ora("Fetching stories...").start();
const indexUrl = `http://127.0.0.1:${port}/index.json`;
const indexRes = await fetch(indexUrl);
if (!indexRes.ok) {
  spinner.fail(`Failed to fetch ${indexUrl}: ${indexRes.status}`);
  closeServer();
  await exitWithFailure();
}
const index = (await indexRes.json()) as {
  entries: Record<
    string,
    { id: string; title: string; name: string; type: string }
  >;
};

// An optional case-insensitive substring passed on the command line
// (`just generate-docs-screenshots UsageLimits`) regenerates only the matching
// stories; without it every docs/ story is captured. Matched stories still
// overwrite their existing files, and unmatched files are left untouched.
const titleFilter = process.argv[2]?.toLowerCase();
const docsStories = Object.values(index.entries).filter(
  (e) =>
    e.title.toLowerCase().startsWith("docs/") &&
    e.type === "story" &&
    (!titleFilter || e.title.toLowerCase().includes(titleFilter)),
);

if (docsStories.length === 0) {
  spinner.warn(
    titleFilter
      ? `No docs/ stories matched "${process.argv[2]}".`
      : "No docs/ stories found in storybook index.",
  );
}

spinner.succeed(`Found ${docsStories.length} stories`);

// 3. Launch Playwright
spinner = ora("Launching Playwright...").start();
const browser = await chromium.launch();
spinner.succeed("Playwright launched");

// Ensure output dir exists
fs.mkdirSync(OUTPUT_DIR, { recursive: true });

// Track which filenames are current (to detect stale files)
const currentFilenames = new Set<string>();

const results: {
  filename: string;
  theme: "light" | "dark";
  storyTitle: string;
  status: "created" | "updated" | "unchanged";
}[] = [];

// 4. Screenshot each story in light and dark mode
// Half the cores, because each capture also encodes and diffs its image in this
// process, so the browsers can't have all of them. Giving a context to every
// core is slightly faster but measurably less stable: over three runs each on a
// 16-core machine, 8 took ~55s and 12 ~53s with one run of the three differing,
// while 16 took ~48s and every run differed. The added CPU contention widens the
// window in which a story's header settles after the command palette has already
// measured where to anchor its menu.
const CONCURRENCY = Math.max(4, Math.floor(os.availableParallelism() / 2));
const total = docsStories.length * 2;
let completed = 0;
const inProgress = new Set<string>();
const errors: { filename: string; error: unknown }[] = [];

function updateSpinner() {
  const lines = [`Screenshots: ${completed}/${total}`];
  for (const f of inProgress) {
    lines.push(chalk.dim(`  ◌ ${f}`));
  }
  spinner.text = lines.join("\n");
}

async function captureScreenshot(
  story: { id: string; title: string; name: string; type: string },
  theme: "light" | "dark",
): Promise<{
  filename: string;
  theme: "light" | "dark";
  storyTitle: string;
  status: "created" | "updated" | "unchanged";
} | null> {
  const filename = filenameFromTitle(story.title, story.name, theme);
  const storyTitle =
    story.name === "Default" ? story.title : `${story.title}#${story.name}`;
  const outputPath = path.join(OUTPUT_DIR, filename);
  const url = `http://127.0.0.1:${port}/iframe.html?id=${encodeURIComponent(story.id)}&viewMode=story&globals=theme:${theme}`;

  // Read the existing file before any async work so the snapshot is consistent
  // regardless of what other concurrent tasks write during page navigation.
  const existingWebp = fs.existsSync(outputPath)
    ? fs.readFileSync(outputPath)
    : null;

  inProgress.add(filename);
  updateSpinner();

  let context: Awaited<ReturnType<typeof browser.newContext>> | null = null;
  try {
    // Load the story in a page of the given context.
    const openStoryPage = async (ctx: NonNullable<typeof context>) => {
      const p = await ctx.newPage();
      // Nothing answers either route under Storybook — there is no NextAuth
      // server, and the status endpoint belongs to the dev server. Fail them
      // outright so the app stops rather than retrying against 404s. Both globs
      // must stay this narrow — `**/api/**` would also match the dashboard's
      // own `src/api/*.ts` modules.
      await p.route("**/api/status", (route) => route.abort());
      await p.route("**/api/auth/**", (route) => route.abort());
      // Disable CSS animations and cursor blinking to ensure stable screenshots
      // of components like Monaco editor that otherwise have non-deterministic renders.
      await p.emulateMedia({ reducedMotion: "reduce" });
      await p.goto(url, { waitUntil: "networkidle", timeout: 60_000 });
      // Network idle does not imply the story rendered and its `play` function
      // ran, so wait for the render to report that it finished.
      await p.waitForFunction(
        (storyId: string) =>
          [...((window as any).__STORYBOOK_PREVIEW__?.storyRenders ?? [])].some(
            (r: any) => r.id === storyId && r.phase === "finished",
          ),
        story.id,
        { timeout: 60_000 },
      );
      await p.evaluate(() => document.fonts.ready);

      // Hide Monaco editor cursors to ensure stable screenshots
      await p.addStyleTag({
        content:
          ".monaco-editor .cursors-layer > .cursor { display: none !important; }",
      });
      await p.addStyleTag({
        content: ".monaco-editor .slider { opacity: 0 !important; }",
      });
      return p;
    };

    // Create a fresh browser context for each screenshot to avoid flaky
    // rendering caused by shared state between stories. Assign `context`
    // right away so the finally block closes it even when the page setup
    // fails partway through.
    context = await browser.newContext({
      viewport: { width: 1024, height: 700 },
      deviceScaleFactor: DEVICE_SCALE_FACTOR,
      locale: LOCALE,
      timezoneId: TIMEZONE_ID,
    });
    let page = await openStoryPage(context);

    // Read the element-level crop selector and optional viewport override from
    // story parameters. In Storybook 10, the store API is
    // storyStoreValue.loadStory().
    const storyParams = await page.evaluate(async (storyId: string) => {
      const empty = {
        screenshotSelector: null as string | null,
        screenshotViewport: null as { width: number; height: number } | null,
      };
      try {
        const preview = (window as any).__STORYBOOK_PREVIEW__;
        const store = preview?.storyStoreValue;
        if (!store) return empty;
        const story = await store.loadStory({ storyId });
        return {
          screenshotSelector: story?.parameters?.screenshotSelector ?? null,
          screenshotViewport: story?.parameters?.screenshotViewport ?? null,
        };
      } catch {
        return empty;
      }
    }, story.id);
    const { screenshotSelector, screenshotViewport } = storyParams;

    // A story whose page doesn't fit the default 1024x700 (e.g. a wide table
    // that would otherwise clip) can widen/heighten the capture viewport.
    // Reload the story in a fresh context at that size instead of resizing the
    // page: a resize makes width-dependent UI (e.g. the deployment badge)
    // remount mid-capture, and its entrance animations then race the
    // screenshot and get frozen at opacity 0.
    if (screenshotViewport) {
      await context.close();
      context = null;
      context = await browser.newContext({
        viewport: screenshotViewport,
        deviceScaleFactor: DEVICE_SCALE_FACTOR,
        locale: LOCALE,
        timezoneId: TIMEZONE_ID,
      });
      page = await openStoryPage(context);
    }

    const isComponentStory = story.title
      .toLowerCase()
      .startsWith("docs/components/");

    let png: Buffer;
    let bgColor: string | undefined;
    if (isComponentStory) {
      const root = page.locator(".sb-main-padded");
      bgColor = await page.evaluate(
        () => getComputedStyle(document.body).backgroundColor,
      );
      png = await root.screenshot({
        omitBackground: true,
        caret: "hide",
        animations: "disabled",
      });
    } else {
      png = await page.screenshot({
        fullPage: false,
        caret: "hide",
        animations: "disabled",
      });
    }

    // If a screenshotSelector is specified, crop to the union bounding box
    // of all matching elements with 16px padding.
    if (screenshotSelector) {
      const elements = await page.locator(screenshotSelector).all();
      const boxes = (
        await Promise.all(elements.map((el) => el.boundingBox()))
      ).filter(
        (b): b is { x: number; y: number; width: number; height: number } =>
          b !== null,
      );

      if (boxes.length > 0) {
        // Compute union bounding box in CSS pixels
        const minX = Math.min(...boxes.map((b) => b.x));
        const minY = Math.min(...boxes.map((b) => b.y));
        const maxX = Math.max(...boxes.map((b) => b.x + b.width));
        const maxY = Math.max(...boxes.map((b) => b.y + b.height));

        // Get image dimensions to clamp
        const meta = await sharp(png).metadata();
        const imgW = meta.width!;
        const imgH = meta.height!;

        // Convert to pixel coordinates and add padding, clamped to image bounds
        const padding = isComponentStory ? CROP_PADDING : CROP_PADDING_PAGE;
        const left = Math.max(
          0,
          Math.round(minX * DEVICE_SCALE_FACTOR) - padding,
        );
        const top = Math.max(
          0,
          Math.round(minY * DEVICE_SCALE_FACTOR) - padding,
        );
        const right = Math.min(
          imgW,
          Math.round(maxX * DEVICE_SCALE_FACTOR) + padding,
        );
        const bottom = Math.min(
          imgH,
          Math.round(maxY * DEVICE_SCALE_FACTOR) + padding,
        );

        png = await sharp(png)
          .extract({
            left,
            top,
            width: right - left,
            height: bottom - top,
          })
          .toBuffer();
      }
    }

    const pipeline = sharp(png);
    if (!screenshotSelector && isComponentStory && bgColor) {
      pipeline.trim().extend({
        top: CROP_PADDING,
        bottom: CROP_PADDING,
        left: CROP_PADDING,
        right: CROP_PADDING,
        background: bgColor,
      });
    }
    const webp = await pipeline.webp({ lossless: true }).toBuffer();

    let status: "created" | "updated" | "unchanged" = "created";
    if (existingWebp !== null) {
      if (existingWebp.equals(webp)) {
        // Fast path: identical bytes means identical image.
        status = "unchanged";
      } else {
        // Bytes differ — decode both and do a perceptual comparison to
        // distinguish real changes from minor rendering non-determinism
        // (sub-pixel anti-aliasing, font hinting, etc.).
        const [a, b] = await Promise.all([
          sharp(existingWebp)
            .ensureAlpha()
            .raw()
            .toBuffer({ resolveWithObject: true }),
          sharp(webp).ensureAlpha().raw().toBuffer({ resolveWithObject: true }),
        ]);
        if (a.info.width !== b.info.width || a.info.height !== b.info.height) {
          status = "updated";
        } else {
          const diff = pixelmatch(
            a.data,
            b.data,
            null,
            a.info.width,
            a.info.height,
            { threshold: 0.1 },
          );
          // Allow up to 0.05% of pixels to differ — handles minor rendering
          // variations that are invisible to the human eye.
          const maxDiff = Math.ceil(a.info.width * a.info.height * 0.0005);
          status = diff <= maxDiff ? "unchanged" : "updated";
        }
      }
    }

    if (status !== "unchanged") {
      fs.writeFileSync(outputPath, webp);
    }

    inProgress.delete(filename);
    completed++;
    spinner.clear();
    if (status === "created") {
      process.stdout.write(
        chalk.green(`  ✓ ${chalk.white.bgGreen("  Created  ")} ${filename}\n`),
      );
    } else if (status === "updated") {
      process.stdout.write(
        chalk.blue(`  ✓ ${chalk.white.bgBlue("  Updated  ")} ${filename}\n`),
      );
    } else {
      process.stdout.write(
        chalk.gray(`  ✓ ${chalk.white.bgGray(" Unchanged ")} ${filename}\n`),
      );
    }
    updateSpinner();
    spinner.render();

    return { filename, theme, storyTitle, status };
  } catch (error) {
    inProgress.delete(filename);
    completed++;
    spinner.clear();
    process.stdout.write(chalk.red(`  ✗ failed: ${filename}: ${error}\n`));
    updateSpinner();
    spinner.render();
    errors.push({ filename, error });
    // If the file already existed, preserve it by returning an "unchanged"
    // result so it won't be deleted as stale and stays in the manifest.
    if (existingWebp !== null) {
      return { filename, theme, storyTitle, status: "unchanged" };
    }
    return null;
  } finally {
    if (context) {
      try {
        await context.close();
      } catch (closeError) {
        process.stdout.write(
          chalk.red(
            `  ✗ failed to close context: ${filename}: ${closeError}\n`,
          ),
        );
      }
    }
  }
}

const tasks = docsStories.flatMap((story) =>
  (["light", "dark"] as const).map(
    (theme) => () => captureScreenshot(story, theme),
  ),
);

spinner = ora(`Screenshots: 0/${total}`).start();

const taskResults = await runWithConcurrency(tasks, CONCURRENCY);

spinner.succeed(`Completed ${total} screenshots`);

for (const result of taskResults) {
  if (result === null) continue;
  currentFilenames.add(result.filename);
  results.push(result);
}

if (errors.length > 0) {
  console.error(chalk.red(`\n${errors.length} screenshot(s) failed:`));
  for (const { filename, error } of errors) {
    console.error(chalk.red(`  ${filename}: ${error}`));
  }
}

// 5. Delete stale screenshots. Skip this when a title filter is active: only
// the matched stories were captured, so every other file would look "stale".
if (titleFilter) {
  ora().info("Skipped stale cleanup (title filter active)");
} else {
  const deleted: string[] = [];
  spinner = ora("Cleaning up stale screenshots...").start();
  const existingWebps = fs.existsSync(OUTPUT_DIR)
    ? fs.readdirSync(OUTPUT_DIR).filter((f) => f.endsWith(".webp"))
    : [];
  for (const file of existingWebps) {
    if (!currentFilenames.has(file)) {
      fs.unlinkSync(path.join(OUTPUT_DIR, file));
      deleted.push(file);
      console.log(chalk.red(`  ✓ ${chalk.white.bgRed("  Deleted  ")} ${file}`));
    }
  }
  if (deleted.length > 0) {
    spinner.succeed(`Deleted ${deleted.length} stale screenshot(s)`);
  } else {
    spinner.succeed("No stale screenshots to delete");
  }
}

// 6. Write manifest
spinner = ora("Writing manifest...").start();
const byStory = new Map<string, { light?: string; dark?: string }>();
for (const { filename, theme, storyTitle } of results) {
  if (!byStory.has(storyTitle)) byStory.set(storyTitle, {});
  byStory.get(storyTitle)![theme] = filename;
}

const getDimensions = async (filename: string) => {
  const { width, height } = await sharp(
    path.join(OUTPUT_DIR, filename),
  ).metadata();
  return { width: width!, height: height! };
};

const generatedEntries = await Promise.all(
  [...byStory.entries()].map(async ([storyTitle, themes]) => ({
    storyTitle,
    light: themes.light
      ? { filename: themes.light, ...(await getDimensions(themes.light)) }
      : undefined,
    dark: themes.dark
      ? { filename: themes.dark, ...(await getDimensions(themes.dark)) }
      : undefined,
  })),
);

// A filtered run only regenerated some stories, so merge into the existing
// manifest instead of replacing it — keep every prior entry and overwrite the
// ones we just captured.
const manifestByStory = new Map<string, (typeof generatedEntries)[number]>();
if (titleFilter && fs.existsSync(MANIFEST_PATH)) {
  const { screenshots: existing } = (await import(
    pathToFileURL(MANIFEST_PATH).href
  )) as { screenshots: (typeof generatedEntries)[number][] };
  for (const entry of existing) manifestByStory.set(entry.storyTitle, entry);
}
for (const entry of generatedEntries) {
  manifestByStory.set(entry.storyTitle, entry);
}
// Map iteration preserves insertion order, so existing entries stay where they
// were (overwriting a key keeps its position) and newly added stories append.
const manifestArray = [...manifestByStory.values()];

const manifestContent = `// @generated by dashboard-storybook/scripts/generate-docs-screenshots.ts
// Do not edit manually.

export const screenshots = ${JSON.stringify(manifestArray, null, 2)} as const;
`;

fs.mkdirSync(path.dirname(MANIFEST_PATH), { recursive: true });
fs.writeFileSync(MANIFEST_PATH, manifestContent);
spinner.succeed("Manifest written");

// 7. Cleanup
spinner = ora("Cleaning up...").start();
await browser.close();
closeServer();
spinner.succeed("Done!");
