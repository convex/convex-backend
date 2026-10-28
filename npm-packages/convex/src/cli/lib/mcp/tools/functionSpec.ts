import { z } from "zod";
import { ConvexTool } from "./index.js";
import { loadSelectedDeploymentCredentials } from "../../api.js";
import { runSystemQuery } from "../../run.js";
import { getMcpDeploymentSelection } from "../requestContext.js";
import { convexToJson } from "../../../../values/index.js";

const inputSchema = z.object({
  deploymentSelector: z
    .string()
    .describe(
      "Deployment selector (from the status tool) to get function metadata from.",
    ),
});

const outputSchema = z
  .any()
  .describe("Function metadata including arguments and return values");

const description = `
Get the function metadata from a Convex deployment.

Returns an array of structured objects for each function the deployment. Each function's
metadata contains its identifier (which is its path within the convex/ folder joined
with its exported name), its argument validator, its return value validator, its type
(i.e. is it a query, mutation, or action), and its visibility (i.e. is it public or
internal).
`.trim();

export const FunctionSpecTool: ConvexTool<
  typeof inputSchema,
  typeof outputSchema
> = {
  name: "functionSpec",
  description,
  inputSchema,
  outputSchema,
  handler: async (ctx, args) => {
    const { projectDir, deployment } = ctx.decodeDeploymentSelectorUnchecked(
      args.deploymentSelector,
    );
    process.chdir(projectDir);
    const deploymentSelection = await getMcpDeploymentSelection(
      ctx,
      deployment,
    );
    const credentials = await loadSelectedDeploymentCredentials(
      ctx,
      deploymentSelection,
    );
    const functions = await runSystemQuery(ctx, {
      deploymentUrl: credentials.url,
      adminKey: credentials.adminKey,
      functionName: "_system/cli/modules:apiSpec",
      componentPath: undefined,
      args: {},
    });
    return convexToJson(functions);
  },
};
