import { Command } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import open from "open";
import { logMessage } from "../bundler/log.js";

export const docs = new Command("docs")
  .description("Open the docs in the browser")
  .allowExcessArguments(false)
  .option("--no-open", "Print docs URL instead of opening it in your browser")
  .action(async (options) => {
    await openDocs(options.open);
  });

async function openDocs(toOpen: boolean) {
  const docsUrl = "https://docs.convex.dev/?utm_source=convex-cli";
  if (toOpen) {
    await open(docsUrl);
    logMessage(chalkStderr.green("Docs have launched! Check your browser."));
  } else {
    logMessage(chalkStderr.green(`Find Convex docs here: ${docsUrl}`));
  }
}
