import { Command } from "@commander-js/extra-typings";
import { deploymentTokenCreate } from "./deploymentTokenCreate.js";
import { deploymentTokenDelete } from "./deploymentTokenDelete.js";

export const deploymentToken = new Command("token")
  .summary("Manage access tokens")
  .description(
    "Create and delete access tokens. Currently supports deploy keys.",
  )
  .addCommand(deploymentTokenCreate)
  .addCommand(deploymentTokenDelete);
