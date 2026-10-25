import { Command } from "@commander-js/extra-typings";
import { aiFiles } from "./aiFiles.js";
import { auth } from "./auth.js";
import { codegen } from "./codegen.js";
import { convexExport } from "./convexExport.js";
import { convexImport } from "./convexImport.js";
import { dashboard } from "./dashboard.js";
import { data } from "./data.js";
import { deploy } from "./deploy.js";
import { deployment } from "./deployment.js";
import { deployments } from "./deployments.js";
import { dev } from "./dev.js";
import { rust } from "./rust.js";
import { docs } from "./docs.js";
import { env } from "./env.js";
import { functionSpec } from "./functionSpec.js";
import { init } from "./init.js";
import { insights } from "./insights.js";
import { integration } from "./integration.js";
import { login } from "./login.js";
import { logout } from "./logout.js";
import { logs } from "./logs.js";
import { mcp } from "./mcp.js";
import { networkTest } from "./network_test.js";
import { project } from "./project.js";
import { reinit } from "./reinit.js";
import { run } from "./run.js";
import { typecheck } from "./typecheck.js";
import { update } from "./update.js";
import { version } from "./version.js";

export function buildProgram() {
  const program = new Command();
  return (
    program
      .name("convex")
      .usage("<command> [options]")
      .description("Start developing with Convex by running `npx convex dev`.")
      .addCommand(login, { hidden: true })
      .addCommand(init, { hidden: true })
      .addCommand(reinit, { hidden: true })
      .addCommand(dev)
      .addCommand(deploy)
      .addCommand(deployments, { hidden: true })
      .addCommand(run)
      .addCommand(convexImport)
      .addCommand(dashboard)
      .addCommand(docs)
      .addCommand(logs)
      .addCommand(typecheck, { hidden: true })
      .addCommand(auth, { hidden: true })
      .addCommand(convexExport)
      .addCommand(env)
      .addCommand(data)
      .addCommand(deployment)
      .addCommand(project)
      .addCommand(codegen)
      .addCommand(rust)
      .addCommand(update)
      .addCommand(logout)
      .addCommand(networkTest, { hidden: true })
      .addCommand(integration, { hidden: true })
      .addCommand(functionSpec)
      .addCommand(insights)
      .addCommand(mcp)
      .addCommand(aiFiles)
      .helpCommand("help <command>", "Show help for given <command>")
      .version(version)
      // Hide version and help so they don't clutter
      // the list of commands.
      .configureHelp({ visibleOptions: () => [] })
      .showHelpAfterError()
  );
}
