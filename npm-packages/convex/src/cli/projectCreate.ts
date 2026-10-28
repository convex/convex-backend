import { Command, Option } from "@commander-js/extra-typings";
import { oneoffContext } from "../bundler/context.js";
import { logFinishedStep, logMessage, showSpinner } from "../bundler/log.js";
import { chalkStderr } from "chalk";
import { createProject } from "./lib/api.js";
import { ensureAuthCanCreateDeployment } from "./lib/deploymentSelection.js";
import { validateOrSelectTeam } from "./lib/utils/utils.js";
import { promptString } from "./lib/utils/prompts.js";
import { projectDashboardUrl } from "./lib/dashboard.js";

type ProjectCreateOptions = {
  team?: string | undefined;
};

async function runProjectCreate(
  nameArg: string | undefined,
  options: ProjectCreateOptions,
): Promise<void> {
  const ctx = await oneoffContext({
    url: undefined,
    adminKey: undefined,
    envFile: undefined,
  });

  // Creating a project goes through the platform API, which accepts personal
  // access tokens and project keys but not deployment/preview deploy keys.
  // Fail fast (and clearly) for the unsupported keys rather than surfacing an
  // opaque error later.
  await ensureAuthCanCreateDeployment(ctx);

  const { team } = await validateOrSelectTeam(ctx, options.team, "Team:");

  let projectName = nameArg;
  if (!projectName) {
    if (!process.stdin.isTTY) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "Specify a project name:\n" + "  `npx convex project create my-app`",
      });
    }
    projectName = await promptString(ctx, {
      message: "Project name:",
    });
  }

  showSpinner(`Creating project ${projectName}...`);
  // `project create` only creates the project. Provisioning a deployment is a
  // separate, explicit step (`npx convex deployment create`) so this command
  // doesn't have to mirror every deployment setting (region, class, ...).
  const { projectSlug } = await createProject(ctx, {
    teamId: team.id,
    projectName,
    deploymentToProvision: null,
  });

  logFinishedStep(
    `Created project ${chalkStderr.bold(projectSlug)} in team ` +
      `${chalkStderr.bold(team.slug)}, manage it at ${chalkStderr.bold(
        projectDashboardUrl(team.slug, projectSlug),
      )}`,
  );
  logMessage(
    chalkStderr.gray(
      `Next, add a deployment with ` +
        `\`npx convex deployment create ${team.slug}:${projectSlug}:dev/my-feature --type dev --select\` ` +
        `(pass \`--region us\` to choose a region).`,
    ),
  );
}

export const projectCreate = new Command("create")
  .summary("Create a new project")
  .description(
    [
      "Create a new project.",
      "",
      "Provisioning a deployment is a separate step — after creating the",
      "project, run `npx convex deployment create` to add one.",
      "",
      "• Create a project in your only team: `npx convex project create my-app`",
      "• Pick the team: `npx convex project create my-app --team my-team`",
    ].join("\n"),
  )
  .allowExcessArguments(false)
  .argument(
    "[name]",
    "The name of the new project. Prompted for when omitted in an " +
      "interactive terminal; required otherwise.",
  )
  .addOption(
    new Option(
      "--team <team_slug>",
      "The team to create the project in. Defaults to your only team, or " +
        "prompts when you belong to several.",
    ),
  )
  .action(runProjectCreate);
