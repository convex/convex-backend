import { Command } from "@commander-js/extra-typings";
import { chalkStderr } from "chalk";
import { logMessage, logOutput } from "../bundler/log.js";
import { oneoffContext } from "../bundler/context.js";
import { jsonToConvex, type JSONValue } from "../values/value.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { actionDescription } from "./lib/command.js";
import {
  formatValue,
  pushToDeployment,
  runInDeployment,
  runSystemQuery,
} from "./lib/run.js";
import { ensureHasConvexDependency } from "./lib/utils/utils.js";
import { getDeploymentSelection } from "./lib/deploymentSelection.js";
import { withRunningBackend } from "./lib/localDeployment/run.js";
import {
  inlineQueryToQuerySource,
  runTestFunctionQuery,
} from "./lib/runTestFunction.js";
import {
  logAndHandleFetchError,
  ThrowingFetchError,
} from "./lib/utils/utils.js";

export const run = new Command("run")
  .summary(
    "Run a function or evaluate an inline readonly query on your deployment",
  )
  .description(
    [
      "Run a function or evaluate an inline readonly query on your deployment.",
      "",
      '• Run a function with JSON arguments: `npx convex run messages:send \'{"body": "hello", "author": "me"}\'`',
      "• Run a function on prod: `npx convex run messages:list --prod`",
      "• Live-update a query's result: `npx convex run messages:list --watch`",
      "• Push local code before running: `npx convex run messages:send '{}' --push`",
      "• Evaluate an inline readonly query: `npx convex run --inline-query 'await ctx.db.query(\"messages\").take(5)'`",
      "",
      "Arguments are specified as a JSON object. By default, this runs on your dev deployment.",
    ].join("\n"),
  )
  .allowExcessArguments(false)
  .addRunOptions()
  .addDeploymentSelectionOptions(actionDescription("Run the function on"))
  .showHelpAfterError()
  .action(async (functionName, argsString, options) => {
    const ctx = await oneoffContext(options);
    const target = await resolveRunTarget({
      ctx,
      functionName,
      argsString,
      options,
    });
    if (target.kind === "function" || options.push) {
      await ensureHasConvexDependency(ctx, "run");
    }

    const deploymentSelection = await getDeploymentSelection(ctx, options);
    const deployment = await loadSelectedDeploymentCredentials(
      ctx,
      deploymentSelection,
      { ensureLocalRunning: false },
    );

    if (
      deployment.deploymentFields?.deploymentType === "prod" &&
      options.push
    ) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          `\`convex run\` doesn't support pushing functions to prod deployments. ` +
          `Remove the --push flag. To push to production use \`npx convex deploy\`.`,
      });
    }

    await withRunningBackend({
      ctx,
      deployment: {
        deploymentUrl: deployment.url,
        deploymentFields: deployment.deploymentFields,
      },
      action: async () => {
        if (target.kind === "inlineQuery") {
          if (options.push) {
            await pushToDeployment(ctx, {
              deploymentUrl: deployment.url,
              adminKey: deployment.adminKey,
              deploymentName:
                deployment.deploymentFields?.deploymentName ?? null,
              typecheck: options.typecheck,
              typecheckComponents: options.typecheckComponents,
              codegen: options.codegen === "enable",
              liveComponentSources: Boolean(options.liveComponentSources),
            });
          }
          return await runInlineQueryInDeployment({
            ctx,
            deploymentUrl: deployment.url,
            adminKey: deployment.adminKey,
            inlineQuery: target.inlineQuery,
            ...(options.component !== undefined
              ? { componentPath: options.component }
              : {}),
          });
        }

        await runInDeployment(ctx, {
          deploymentUrl: deployment.url,
          adminKey: deployment.adminKey,
          deploymentName: deployment.deploymentFields?.deploymentName ?? null,
          functionName: target.functionName,
          argsString: target.argsString,
          componentPath: options.component,
          identityString: options.identity,
          push: Boolean(options.push),
          watch: Boolean(options.watch),
          typecheck: options.typecheck,
          typecheckComponents: options.typecheckComponents,
          codegen: options.codegen === "enable",
          liveComponentSources: Boolean(options.liveComponentSources),
        });
      },
    });
  });

type RunTarget =
  | {
      kind: "function";
      functionName: string;
      argsString: string;
    }
  | {
      kind: "inlineQuery";
      inlineQuery: string;
    };

async function resolveRunTarget(args: {
  ctx: Awaited<ReturnType<typeof oneoffContext>>;
  functionName: string | undefined;
  argsString: string | undefined;
  options: {
    inlineQuery?: string;
    watch?: boolean;
    push?: boolean;
    identity?: string;
    component?: string;
  };
}): Promise<RunTarget> {
  const inlineQuery = args.options.inlineQuery?.trim();
  if (inlineQuery !== undefined && args.functionName !== undefined) {
    return await args.ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        "`npx convex run` accepts either <functionName> or `--inline-query`, not both.",
    });
  }
  if (inlineQuery === undefined) {
    if (args.functionName === undefined) {
      return await args.ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "`npx convex run` requires either <functionName> or `--inline-query`.",
      });
    }
    return {
      kind: "function",
      functionName: args.functionName,
      argsString: args.argsString ?? "{}",
    };
  }
  if (inlineQuery.length === 0) {
    return await args.ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: "`--inline-query` must not be empty.",
    });
  }
  if (args.options.watch) {
    return await args.ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        "`--inline-query` can't be combined with `--watch`. Use `convex run <functionName> --watch` for named deployed queries.",
    });
  }
  if (args.options.identity !== undefined) {
    return await args.ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: "`--inline-query` can't be combined with `--identity`.",
    });
  }
  return { kind: "inlineQuery", inlineQuery };
}

async function runInlineQueryInDeployment(args: {
  ctx: Awaited<ReturnType<typeof oneoffContext>>;
  deploymentUrl: string;
  adminKey: string;
  inlineQuery: string;
  componentPath?: string;
}) {
  try {
    const componentId = await resolveInlineQueryComponentId(args);
    const outcome = await runTestFunctionQuery(args.ctx, {
      deploymentUrl: args.deploymentUrl,
      adminKey: args.adminKey,
      querySource: inlineQueryToQuerySource(args.inlineQuery),
      ...(componentId !== undefined ? { componentId } : {}),
    });
    if (outcome.kind === "applicationFailure") {
      return await args.ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: chalkStderr.red(
          `Query failed: ${JSON.stringify(outcome.payload, null, 2)}`,
        ),
      });
    }

    for (const line of outcome.logLines) {
      logMessage(line);
    }

    const convexValue = jsonToConvex(outcome.value as JSONValue);
    if (convexValue !== null) logOutput(formatValue(convexValue));
  } catch (err) {
    if (err instanceof ThrowingFetchError) return await err.handle(args.ctx);
    return await logAndHandleFetchError(args.ctx, err);
  }
}

async function resolveInlineQueryComponentId(args: {
  ctx: Awaited<ReturnType<typeof oneoffContext>>;
  deploymentUrl: string;
  adminKey: string;
  componentPath?: string;
}) {
  const componentPath = args.componentPath?.trim();
  if (componentPath === undefined || componentPath === "_App") {
    return undefined;
  }
  if (componentPath.length === 0) {
    return await args.ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: "`--component` must not be empty.",
    });
  }

  const components = (await runSystemQuery(args.ctx, {
    deploymentUrl: args.deploymentUrl,
    adminKey: args.adminKey,
    functionName: "_system/frontend/components:list",
    componentPath: undefined,
    args: {},
  })) as { id: string; path: string }[];

  const component = components.find(({ path }) => path === componentPath);
  if (component !== undefined) {
    return component.id;
  }

  const availableComponents = components
    .map(({ path }) => path)
    .filter((path) => path.length > 0)
    .sort();
  const availableComponentsMessage =
    availableComponents.length === 0
      ? "This deployment has no mounted components."
      : `Available components:\n${availableComponents.map((path) => `• ${chalkStderr.gray(path)}`).join("\n")}`;

  return await args.ctx.crash({
    exitCode: 1,
    errorType: "fatal",
    printedMessage:
      `Component path "${componentPath}" was not found.\n\n` +
      `${availableComponentsMessage}\n` +
      "Omit `--component` to target the app root.",
  });
}
