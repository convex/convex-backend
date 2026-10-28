import { Command } from "@commander-js/extra-typings";
import { deploymentSelect } from "./deploymentSelect.js";
import { deploymentCreate } from "./deploymentCreate.js";
import { deploymentToken } from "./deploymentToken.js";
import { usage, usageLimits } from "./usageLimits.js";

export const deployment = new Command("deployment")
  .summary("Manage deployments")
  .description("Manage deployments in your project.")
  .addCommand(deploymentSelect)
  .addCommand(deploymentCreate)
  .addCommand(deploymentToken)
  .addCommand(usage)
  .addCommand(usageLimits);
