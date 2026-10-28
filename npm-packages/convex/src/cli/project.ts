import { Command } from "@commander-js/extra-typings";
import { projectCreate } from "./projectCreate.js";

export const project = new Command("project")
  .summary("Manage projects")
  .description("Manage projects in your team.")
  .addCommand(projectCreate);
