import { Command } from "@commander-js/extra-typings";
import path from "path";
import { chalkStderr } from "chalk";
import { oneoffContext } from "../bundler/context.js";
import { logMessage } from "../bundler/log.js";
import { getFunctionsDirectoryPath } from "./lib/config.js";
import { scaffoldRust } from "./lib/rust/scaffold.js";
import { ensureRustToolchain } from "./lib/rust.js";

const init = new Command("init")
  .summary('Scaffold convex/rust/ for "use rust" actions')
  .description(
    [
      "Creates convex/rust/ with a Cargo.toml, an example #[action], rust-toolchain.toml,",
      "a CLI-managed .cargo/config.toml and a .gitignore. Existing files are never overwritten.",
      "",
      "Set CONVEX_FN_PATH to use a local checkout of the `convex_fn` crate.",
    ].join("\n"),
  )
  .allowExcessArguments(false)
  .action(async () => {
    const ctx = await oneoffContext({
      url: undefined,
      adminKey: undefined,
      envFile: undefined,
    });
    const functionsDir = await getFunctionsDirectoryPath(ctx);
    const rustDir = path.join(functionsDir, "rust");
    const { created, skipped } = scaffoldRust(rustDir, {
      convexFnPath: process.env.CONVEX_FN_PATH || undefined,
      channel: process.env.CONVEX_RUST_CHANNEL || undefined,
    });
    const rel = (f: string) => path.relative("", path.join(rustDir, f));
    for (const f of created)
      logMessage(chalkStderr.green(`created  ${rel(f)}`));
    for (const f of skipped)
      logMessage(chalkStderr.gray(`skipped  ${rel(f)} (already exists)`));
    await ensureRustToolchain(ctx, rustDir);
    logMessage(
      `\nNext: run \`cargo add <crate>\` in ${path.relative("", rustDir)} to add libraries, and \`npx convex dev\` to build and push.`,
    );
  });

export const rust = new Command("rust")
  .summary('Tools for "use rust" actions')
  .addCommand(init);
