import { Command, Option } from "@commander-js/extra-typings";
import { oneoffContext } from "../bundler/context.js";
import { logError } from "../bundler/log.js";
import { CallToolRequest, Server } from "@modelcontextprotocol/server";
import { serveStdio } from "@modelcontextprotocol/server/stdio";
import { actionDescription } from "./lib/command.js";
import { checkAuthorization } from "./lib/login.js";
import {
  McpOptions,
  RequestContext,
  RequestCrash,
} from "./lib/mcp/requestContext.js";
import { mcpTool, convexTools, ConvexTool } from "./lib/mcp/tools/index.js";
import { Mutex } from "./lib/utils/mutex.js";
import { initializeBigBrainAuth } from "./lib/deploymentSelection.js";

const allToolNames = convexTools.map((t) => t.name).sort();

export const mcp = new Command("mcp")
  .summary("Manage the Model Context Protocol server for Convex [BETA]")
  .description(
    "Commands to initialize and run a Model Context Protocol server for Convex that can be used with AI tools.\n" +
      "This server exposes your Convex codebase to AI tools in a structured way.",
  )
  .allowExcessArguments(false);

mcp
  .command("start")
  .summary("Start the MCP server")
  .description(
    "Start the Model Context Protocol server for Convex that can be used with AI tools.",
  )
  .option(
    "--project-dir <project-dir>",
    "Run the MCP server for a single project. By default, the MCP server can run for multiple projects, and each tool call specifies its project directory.",
  )
  .option(
    "--disable-tools <tool-names>",
    `Comma separated list of tool names to disable (options: ${allToolNames.join(", ")})`,
  )
  .option(
    "--cautiously-allow-production-pii",
    "Allow read-only tools (data, logs, queries) on production deployments. These tools may expose PII. Defaults to false.",
    false,
  )
  .option(
    "--dangerously-enable-production-deployments",
    "DANGEROUSLY allow the MCP server to access production deployments, including mutating tools. Defaults to false.",
    false,
  )
  // Deprecated option, we swapped the default. no-op.
  .addOption(
    new Option("--disable-production-deployments")
      .conflicts("--dangerously-enable-production-deployments")
      .hideHelp(),
  )
  .addDeploymentSelectionOptions(actionDescription("Run the MCP server on"))
  .action(async (options) => {
    const ctx = await oneoffContext(options);
    try {
      // `serveStdio` only calls the factory once a client connects, and answers
      // a factory throw with an opaque `-32603`, so reject a bad
      // `--disable-tools` here instead.
      enabledTools(options);
    } catch (error: any) {
      await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        errForSentry: `Failed to start MCP server: ${error}`,
        printedMessage: `Failed to start MCP server: ${error}`,
      });
    }
    // The SDK picks the era from the client's opening message: a request
    // carrying the 2026-07-28 `_meta` envelope pins a stateless connection,
    // while an `initialize` is still served by the same factory over the legacy
    // protocol.
    // The returned handle's `close()` is intentionally never called: the
    // server lives until the client kills the process, and process exit tears
    // the transport down.
    serveStdio(() => makeServer(options), {
      legacy: "serve",
      // The only channel for out-of-band failures (transport errors, dropped
      // malformed notifications). stdout carries the protocol, so they go to
      // stderr.
      onerror: (error) => logError(`MCP server error: ${error.message}`),
    });
    // Keep the process running
    await new Promise(() => {});
  });

function enabledTools(
  options: McpOptions,
): Record<string, ConvexTool<any, any>> {
  const disabledToolNames = new Set<string>();
  for (const toolName of options.disableTools?.split(",") ?? []) {
    const name = toolName.trim();
    if (!allToolNames.includes(name)) {
      // eslint-disable-next-line no-restricted-syntax
      throw new Error(
        `Disabled tool ${name} not found (valid tools: ${allToolNames.join(", ")})`,
      );
    }
    disabledToolNames.add(name);
  }

  const enabledToolsByName: Record<string, ConvexTool<any, any>> = {};
  for (const tool of convexTools) {
    if (!disabledToolNames.has(tool.name)) {
      enabledToolsByName[tool.name] = tool;
    }
  }
  return enabledToolsByName;
}

export function makeServer(options: McpOptions) {
  const enabledToolsByName = enabledTools(options);
  const mutex = new Mutex();
  const server = new Server(
    {
      name: "Convex MCP Server",
      version: "0.0.1",
    },
    {
      capabilities: {
        tools: {},
      },
    },
  );
  server.setRequestHandler("tools/call", async (request: CallToolRequest) => {
    const ctx = new RequestContext(options);
    await initializeBigBrainAuth(ctx, options);
    try {
      const authorized = await checkAuthorization(ctx, false);
      if (!authorized) {
        await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage:
            "Not Authorized: Run `npx convex dev` to login to your Convex project.",
        });
      }
      if (!request.params.arguments) {
        await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: "No arguments provided",
        });
      }
      const convexTool = enabledToolsByName[request.params.name];
      if (!convexTool) {
        await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: `Tool ${request.params.name} not found`,
        });
      }
      const input = convexTool.inputSchema.parse(request.params.arguments);

      // Serialize tool handlers since they're mutating the current working directory.
      const result = await mutex.runExclusive(async () => {
        return await convexTool.handler(ctx, input);
      });
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(result),
          },
        ],
      };
    } catch (error: any) {
      let message: string;
      if (error instanceof RequestCrash) {
        message = error.printedMessage;
      } else if (error instanceof Error) {
        message = error.message;
      } else {
        message = String(error);
      }
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({ error: message }),
          },
        ],
        isError: true,
      };
    }
  });
  server.setRequestHandler("tools/list", async () => {
    return {
      tools: Object.values(enabledToolsByName).map(mcpTool),
    };
  });
  return server;
}
