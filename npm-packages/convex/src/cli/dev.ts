import { Command, Option } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import { installSigintHandler, oneoffContext } from "../bundler/context.js";
import { deploymentCredentialsOrConfigure } from "./configure.js";
import { announceDeploymentTarget } from "./lib/announceDeploymentTarget.js";
import { usageStateWarning } from "./lib/usage.js";
import { normalizeDevOptions } from "./lib/command.js";
import { devAgainstDeployment } from "./lib/dev.js";
import {
  CONVEX_DEPLOYMENT_ENV_VAR_NAME,
  CONVEX_SELF_HOSTED_URL_VAR_NAME,
  ensureHasConvexDependency,
} from "./lib/utils/utils.js";
import {
  getDeploymentSelection,
  type DeploymentSelection,
} from "./lib/deploymentSelection.js";
import { checkVersionAndAiFilesStaleness } from "./lib/updates.js";

export const dev = new Command("dev")
  .summary("Develop against a dev deployment, watching for changes")
  .description(
    "Develop against a dev deployment, watching for changes\n\n" +
      "  1. Configures a new or existing project (if needed)\n" +
      "  2. Updates generated types and pushes code to the configured dev deployment\n" +
      "  3. Runs the provided command (if `--start` or `--run` is used)\n" +
      "  4. Watches for file changes, and repeats step 2\n",
  )
  .allowExcessArguments(false)
  .option("-v, --verbose", "Show full listing of changes")
  .addOption(
    new Option(
      "--typecheck <mode>",
      `Check TypeScript files with \`tsc --noEmit\`.`,
    )
      .choices(["enable", "try", "disable"] as const)
      .default("try" as const),
  )
  .option(
    "--typecheck-components",
    "Check TypeScript files within component implementations with `tsc --noEmit`.",
    false,
  )
  .addOption(
    new Option("--codegen <mode>", "Regenerate code in `convex/_generated/`")
      .choices(["enable", "disable"] as const)
      .default("enable" as const),
  )
  .addOption(
    new Option(
      "--push-all-modules",
      "Push all modules without checking for unchanged module hashes from the server",
    )
      .default(false)
      .hideHelp(),
  )
  .option(
    "--once",
    "Execute only the first 3 steps, stop on any failure",
    false,
  )
  .option(
    "--until-success",
    "Execute only the first 3 steps, on failure watch for local and remote changes and retry steps 2 and 3",
    false,
  )
  .addOption(
    new Option(
      "--start <command>",
      "Start a long-running command alongside `npx convex dev`, like a frontend " +
        "dev server. The command inherits stdin/stdout so you can interact " +
        "with it directly. Example: npx convex dev --start 'vite --open'",
    ).conflicts(["--run", "--run-sh"]),
  )
  .addOption(
    new Option("--run-sh <command>", "Deprecated: use --start instead.")
      .conflicts(["--start", "--run"])
      .hideHelp(),
  )
  .addOption(
    new Option(
      "--run <functionName>",
      "The identifier of the function to run in step 3, " +
        "like `api.init.createData` or `myDir/myFile:myFunction`",
    ).conflicts(["--start"]),
  )
  .option(
    "--run-component <functionName>",
    'If --run is used and the function is in a component, the path to the component (e.g. "workflow" or "workflow/workpool"). ' +
      "Components are a beta feature. This flag is unstable and may change in subsequent releases.",
  )
  .addOption(
    new Option(
      "--tail-logs [mode]",
      [
        "Choose whether to tail Convex function logs in this terminal: ",
        "• `always` shows logs continuously",
        "• `pause-on-deploy` (the default) pauses logs during deploys so you can spot sync issues",
        "• `disable` hides logs while developing.",
      ].join("\n"),
    )
      .choices(["always", "pause-on-deploy", "disable"] as const)
      .default("pause-on-deploy"),
  )
  .addOption(new Option("--trace-events").default(false).hideHelp())
  .addOption(new Option("--debug-bundle-path <path>").hideHelp())
  .addOption(new Option("--debug-node-apis").hideHelp())
  .addOption(new Option("--live-component-sources").hideHelp())
  .addOption(
    new Option(
      "--configure [choice]",
      "Ignore existing configuration and configure new or existing project, interactively or set by --team <team_slug>, --project <project_slug>, and --dev-deployment local|cloud",
    )
      .choices(["new", "existing"] as const)
      .conflicts(["--url", "--admin-key", "--env-file"]),
  )
  .addOption(
    new Option(
      "--team <team_slug>",
      "The team you'd like to use for this project",
    ).hideHelp(),
  )
  .addOption(
    new Option(
      "--project <project_slug>",
      "The name of the project you'd like to configure",
    ).hideHelp(),
  )
  .addOption(
    new Option(
      "--dev-deployment <mode>",
      "Use a local or cloud deployment for dev for this project",
    )
      .choices(["cloud", "local"] as const)
      .conflicts(["--prod"])
      .hideHelp(),
  )
  .addOption(
    new Option(
      "--prod",
      "Develop live against this project's production deployment.",
    )
      .default(false)
      .hideHelp(),
  )
  .addOption(
    // Only `--prod` pushes check this; hidden alongside it.
    new Option(
      "--allow-unstaging-schema",
      "Skip the confirmation prompt when a push to production discards a staged table validator that is validating or already validated.",
    )
      .default(false)
      .hideHelp(),
  )
  .addOption(
    new Option(
      "--env-file <envFile>",
      `Path to a custom file of environment variables, for choosing the \
deployment, e.g. ${CONVEX_DEPLOYMENT_ENV_VAR_NAME} or ${CONVEX_SELF_HOSTED_URL_VAR_NAME}. \
Same format as .env.local or .env files, and overrides them.`,
    ),
  )
  .addOption(new Option("--skip-push").default(false).hideHelp())
  .addOption(new Option("--admin-key <adminKey>").hideHelp())
  .addOption(new Option("--url <url>").hideHelp())
  // Options for testing
  .addOption(new Option("--override-auth-url <url>").hideHelp())
  .addOption(new Option("--override-auth-client <id>").hideHelp())
  .addOption(new Option("--override-auth-username <username>").hideHelp())
  .addOption(new Option("--override-auth-password <password>").hideHelp())
  .addOption(new Option("--local-cloud-port <port>").hideHelp())
  .addOption(new Option("--local-site-port <port>").hideHelp())
  .addOption(new Option("--local-backend-version <version>").hideHelp())
  .addOption(new Option("--local-force-upgrade").default(false).hideHelp())
  .addOption(new Option("--deployment <deployment>").hideHelp())
  .addOption(new Option("--local").default(false).hideHelp())
  .addOption(new Option("--cloud").default(false).hideHelp())
  .showHelpAfterError()
  .action(async (cmdOptions) => {
    const ctx = await oneoffContext(cmdOptions);
    installSigintHandler(ctx);

    if (cmdOptions.deployment !== undefined) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "`--deployment` can’t be used with `npx convex dev`. \n\n" +
          "  To select this deployment for development, run: \n" +
          chalkStderr.bold(
            `      npx convex deployment select ${cmdOptions.deployment}\n`,
          ) +
          "  Then, run `npx convex dev` again.",
      });
    }

    if (cmdOptions.local) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "`--local` is deprecated. \n\n" +
          "  To select your local deployment, run: \n" +
          chalkStderr.bold("      npx convex deployment select local\n") +
          "  Then, run `npx convex dev` again.",
      });
    }

    if (cmdOptions.cloud) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "`--cloud` is deprecated. \n\n" +
          "  To select your personal cloud dev deployment, run: \n" +
          chalkStderr.bold("      npx convex deployment select dev\n") +
          "  Then, run `npx convex dev` again.",
      });
    }

    const devOptions = await normalizeDevOptions(ctx, cmdOptions);

    // Check that we're in a Convex project (a package.json with a `convex`
    // dependency) before doing anything else, so we fail fast with a clear
    // error instead of prompting to create/select a project and only then
    // crashing because there's nothing to push.
    await ensureHasConvexDependency(ctx, "run `npx convex dev`");

    if (cmdOptions.configure === undefined) {
      if (cmdOptions.team || cmdOptions.project || cmdOptions.devDeployment)
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage:
            "`--team, --project, and --dev-deployment can can only be used with `--configure`.",
        });
    }

    const localOptions: {
      ports: { cloud: number | undefined; site: number | undefined };
      backendVersion: string | undefined;
      forceUpgrade: boolean;
    } = {
      ports: {
        cloud:
          cmdOptions.localCloudPort !== undefined
            ? parseInt(cmdOptions.localCloudPort)
            : undefined,
        site:
          cmdOptions.localSitePort !== undefined
            ? parseInt(cmdOptions.localSitePort)
            : undefined,
      },
      backendVersion: cmdOptions.localBackendVersion,
      forceUpgrade: cmdOptions.localForceUpgrade,
    };

    const configure =
      cmdOptions.configure === true ? "ask" : (cmdOptions.configure ?? null);
    // --configure means "pick a project" — skip deployment selection entirely
    const deploymentSelection =
      configure !== null
        ? ({
            kind: "chooseProject",
            selectionWithinProject: {
              // For backwards compatibility, allow `--configure --prod`
              kind: cmdOptions.prod ? "prod" : "unspecified",
            },
          } satisfies DeploymentSelection)
        : await getDeploymentSelection(ctx, cmdOptions);
    const credentials = await deploymentCredentialsOrConfigure(
      ctx,
      deploymentSelection,
      configure,
      {
        ...cmdOptions,
        localOptions,
      },
    );

    announceDeploymentTarget("Developing against deployment:", credentials);

    await Promise.all([
      ...(!cmdOptions.skipPush
        ? [
            devAgainstDeployment(
              ctx,
              {
                url: credentials.url,
                adminKey: credentials.adminKey,
                deploymentName:
                  credentials.deploymentFields?.deploymentName ?? null,
                ...(credentials.deploymentFields?.deploymentType !== undefined
                  ? {
                      deploymentType:
                        credentials.deploymentFields.deploymentType,
                    }
                  : {}),
                backendVersion: localOptions.backendVersion,
              },
              devOptions,
            ),
          ]
        : []),
      ...(credentials.deploymentFields !== null
        ? [
            usageStateWarning(ctx, credentials.deploymentFields.deploymentName),
            checkVersionAndAiFilesStaleness(ctx),
          ]
        : []),
    ]);
  });
