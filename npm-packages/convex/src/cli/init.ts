import { Command } from "@commander-js/extra-typings";
import { installSigintHandler, oneoffContext } from "../bundler/context.js";
import { deploymentCredentialsOrConfigure } from "./configure.js";
import { getDeploymentSelection } from "./lib/deploymentSelection.js";
import { checkVersionAndAiFilesStaleness } from "./lib/updates.js";
import { usageStateWarning } from "./lib/usage.js";

// Equivalent to `npx convex dev --once --skip-push`.
export const init = new Command("init")
  .description(
    "Ensures a Convex project is configured and initialized in the current directory. Does nothing if one is already configured. Unlike `npx convex dev`, it will not push code or typecheck.\nTypically you can skip this and run `npx convex dev` directly which will both initialize and continously deploy code. \n\nTo initialize an agent: `export CONVEX_AGENT_MODE=anonymous && npx convex init && npx convex env set < .env.defaults && npx convex dev`.",
  )
  .summary(
    "Ensures a Convex project is configured. Does not push code. No-ops if already configured.",
  )
  .allowExcessArguments(false)
  .action(async () => {
    const ctx = await oneoffContext({
      url: undefined,
      adminKey: undefined,
      envFile: undefined,
    });
    installSigintHandler(ctx);

    const deploymentSelection = await getDeploymentSelection(ctx, {});
    const credentials = await deploymentCredentialsOrConfigure(
      ctx,
      deploymentSelection,
      null,
      {
        prod: false,
        localOptions: {
          ports: {
            cloud: undefined,
            site: undefined,
          },
          forceUpgrade: false,
        },
      },
    );

    if (credentials.deploymentFields !== null) {
      await Promise.all([
        usageStateWarning(ctx, credentials.deploymentFields.deploymentName),
        checkVersionAndAiFilesStaleness(ctx),
      ]);
    }

    await ctx.flushAndExit(0);
  });
