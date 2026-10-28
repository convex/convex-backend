import {
  encodeDeploymentSelector,
  getMcpDeploymentSelection,
  RequestContext,
} from "../requestContext.js";
import {
  DeploymentSelectionWithinProject,
  deploymentSelectionWithinProjectFromOptions,
  loadSelectedDeploymentCredentials,
} from "../../api.js";
import { z } from "zod";
import { ConvexTool } from "./index.js";
import { deploymentDashboardUrlPage } from "../../../lib/dashboard.js";
import { getDeploymentSelection } from "../../../lib/deploymentSelection.js";

const projectDirDescription = `
The root directory of the Convex project. This is usually the editor's workspace directory
and often includes the 'package.json' file and the 'convex/' folder.

Pass this option unless explicitly instructed not to.
`;

const inputSchema = z.object({
  projectDir: z.string().optional().describe(projectDirDescription),
});
const outputSchema = z.object({
  availableDeployments: z.array(
    z.object({
      kind: z.string(),
      deploymentSelector: z.string(),
      url: z.string(),
      dashboardUrl: z.string().optional(),
      readOnly: z.boolean().optional(),
    }),
  ),
});

const description = `
Get all available deployments for a given Convex project directory.

Use this tool to find the deployment selector, URL, and dashboard URL for each
deployment associated with the project. Pass the deployment selector to other
tools to target a specific deployment.

When deployed to Convex Cloud, projects have a development ({"kind": "ownDev"}) and
production ({"kind": "prod"}) deployment. Generally default to using the development
deployment unless you'd specifically like to debug issues in production.

When running locally, there will be a single "urlWithAdminKey" deployment.

If a deployment has "readOnly: true", it can only be used with read-only tools
that don't expose PII (\`insights\`, \`functionSpec\`, \`tables\`). Tools that read
user data (\`data\`, \`logs\`, \`runOneoffQuery\`) and mutating tools will reject it.

If "readOnly" is false or absent, all tools can be used with the deployment.
`.trim();

export const StatusTool: ConvexTool<typeof inputSchema, typeof outputSchema> = {
  name: "status",
  description,
  inputSchema,
  outputSchema,
  handler: async (ctx: RequestContext, input) => {
    const projectDir = input.projectDir ?? ctx.options.projectDir;
    if (projectDir === undefined) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "No project directory provided. Either provide the `projectDir` argument or configure the MCP server with the `--project-dir` flag.",
      });
    }
    process.chdir(projectDir);
    const selectionWithinProject = deploymentSelectionWithinProjectFromOptions(
      ctx.options,
    );
    const deploymentSelection = await getDeploymentSelection(ctx, ctx.options);
    const credentials = await loadSelectedDeploymentCredentials(
      ctx,
      deploymentSelection,
    );
    let availableDeployments = [
      {
        kind: selectionWithinProject.kind,
        deploymentSelector: encodeDeploymentSelector(
          projectDir,
          selectionWithinProject,
        ),
        url: credentials.url,
        dashboardUrl:
          credentials.deploymentFields?.deploymentName &&
          deploymentDashboardUrlPage(
            credentials.deploymentFields.deploymentName,
            "",
          ),
      },
    ];
    // Also get the prod cloud deployment if we're using a cloud-hosted dev-deployment
    if (
      selectionWithinProject.kind === "unspecified" &&
      !(
        deploymentSelection.kind === "existingDeployment" &&
        deploymentSelection.deploymentToActOn.deploymentFields === null
      )
    ) {
      const prodSelectionWithinProject: DeploymentSelectionWithinProject = {
        kind: "prod",
      };
      const prodDeploymentSelection = await getMcpDeploymentSelection(
        ctx,
        prodSelectionWithinProject,
      );
      const prodCredentials = await loadSelectedDeploymentCredentials(
        ctx,
        prodDeploymentSelection,
      );
      if (
        prodCredentials.deploymentFields?.deploymentName &&
        prodCredentials.deploymentFields.deploymentType
      ) {
        availableDeployments.push({
          kind: prodSelectionWithinProject.kind,
          deploymentSelector: encodeDeploymentSelector(
            projectDir,
            prodSelectionWithinProject,
          ),
          url: prodCredentials.url,
          dashboardUrl: deploymentDashboardUrlPage(
            prodCredentials.deploymentFields.deploymentName,
            "",
          ),
        });
      }
    }
    if (ctx.productionDeploymentsDisabled) {
      const readOnly = ctx.productionPiiAllowed ? false : true;
      return {
        availableDeployments: availableDeployments.map((d) =>
          d.kind === "prod" ? { ...d, readOnly } : d,
        ),
      };
    }
    return { availableDeployments };
  },
};
