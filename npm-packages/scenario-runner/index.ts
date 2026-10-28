import { Command } from "@commander-js/extra-typings";
import { WebSocket } from "ws";
import { RunFunction } from "./scenarios/run_function.js";
import { ObserveInsert } from "./scenarios/observe_insert.js";
import "@sentry/tracing";
import * as Sentry from "@sentry/node";
import { SnapshotExport } from "./scenarios/snapshot_export.js";
import { CloudBackup } from "./scenarios/cloud_backup.js";
import { Config, ProvisionerInfo, Scenario } from "./scenario.js";
import { Search } from "./scenarios/search.js";
import { VectorSearch } from "./scenarios/vector_search.js";
import { ConvexClient } from "convex/browser";
import { CLOSE_TIMEOUT, ScenarioMessage, ScenarioSpec } from "./types.js";
import { RunHttpAction } from "./scenarios/run_http_action.js";
import dns from "node:dns";
import { ManyIntersections } from "./scenarios/many_intersections.js";
import { HoldSubscriptions } from "./scenarios/hold_subscriptions.js";
import { runAtRate, runScenarioOnce, validateRate } from "./runner.js";

Sentry.init({
  tracesSampleRate: 0.1,
});

/**
 * Node defaults to ipv6, and since usher runs locally with ipv4 addresses,
 * set the default result order to ipv4
 */
dns.setDefaultResultOrder("ipv4first");

async function main(
  deploymentUrl: string,
  adminKey: string,
  lgPort: number,
  provisionHost: string | undefined,
  accessToken: string | undefined,
  scenarios: ScenarioMessage[],
) {
  for (const { rate } of scenarios) {
    if (rate !== null) {
      validateRate(rate);
    }
  }

  console.log(`ScenarioRunner is running! ${deploymentUrl}.`);
  const ws = new WebSocket(`ws://127.0.0.1:${lgPort}/sync`);

  const provisionerInfo =
    provisionHost && accessToken
      ? await getProvisionerInfo(provisionHost, accessToken, deploymentUrl)
      : undefined;

  const config = {
    deploymentUrl,
    loadGenWS: ws,
    provisionerInfo,
  };
  await Promise.all(
    scenarios.map((scenarioMessage) =>
      runScenario(config, scenarioMessage, adminKey),
    ),
  );
}

async function getProvisionerInfo(
  provisionHost: string,
  accessToken: string,
  deploymentUrl: string,
): Promise<ProvisionerInfo> {
  const deploymentName = await (
    await fetch(`${deploymentUrl}/instance_name`)
  ).text();

  const response = await fetch(
    `${provisionHost}/api/deployment/${deploymentName}/team_and_project`,
    {
      headers: {
        Authorization: `Bearer ${accessToken}`,
      },
    },
  );

  if (!response.ok) {
    const responseText = await response.text();
    throw new Error(
      `HTTP error ${response.status}: ${response.statusText}. Response: ${responseText}`,
    );
  }

  let info;
  try {
    info = await response.json();
  } catch (e: unknown) {
    const responseText = await response.text();
    throw new Error(
      `Failed to parse JSON response: ${e instanceof Error ? e.message : String(e)}. Full response: ${responseText}`,
    );
  }
  const deploymentId = info.deploymentId;

  return {
    provisionHost,
    accessToken,
    deploymentName,
    deploymentId,
  };
}

/**
 * `client.close()` resolves only when the WebSocket `onclose` event fires. On a
 * half-open / wedged connection (e.g. after the backend closes with 1011
 * InternalServerError, or the WS upgrade fails with a non-101 status) that event
 * may never arrive, so an un-bounded `await client.close()` can hang the scenario
 * loop forever -- silently halting all throughput for the scenario until the
 * process restarts. Bound it so a stuck close just moves on.
 */
async function closeWithTimeout(client: ConvexClient) {
  await Promise.race([
    client.close(),
    new Promise<void>((resolve) => setTimeout(resolve, CLOSE_TIMEOUT)),
  ]);
}

function createScenario(
  config: Config,
  scenarioSpec: ScenarioSpec,
  adminKey: string,
): Scenario {
  switch (scenarioSpec.name) {
    case "RunFunction":
      return new RunFunction(config, scenarioSpec.path, scenarioSpec.fn_type);
    case "ObserveInsert":
      return new ObserveInsert(config, scenarioSpec.search_indexes);
    case "ManyIntersections":
      return new ManyIntersections(config, scenarioSpec.num_subscriptions);
    case "HoldSubscriptions":
      return new HoldSubscriptions(
        config,
        scenarioSpec.num_subscriptions,
        scenarioSpec.hold_duration_secs,
        scenarioSpec.invalidation_interval_secs,
        scenarioSpec.num_invalidations,
      );
    case "SnapshotExport":
      return new SnapshotExport(config, adminKey);
    case "CloudBackup":
      return new CloudBackup(config);
    case "Search":
      return new Search(config);
    case "VectorSearch":
      return new VectorSearch(config);
    case "RunHttpAction":
      return new RunHttpAction(config, scenarioSpec.path, scenarioSpec.method);
    default: {
      scenarioSpec satisfies never;
      throw new Error(`Invalid scenario: ${scenarioSpec}`);
    }
  }
}

async function runScenario(
  config: Config,
  scenarioMessage: ScenarioMessage,
  adminKey: string,
) {
  const scenarioSpec = scenarioMessage.scenario;
  console.log(`Running scenario: ${scenarioSpec.name}`);
  const createScenarioInstance = () =>
    createScenario(config, scenarioSpec, adminKey);
  const scenarioName = createScenarioInstance().name;
  const handleError = (scenario: Scenario, error: unknown) => {
    scenario.sendDefaultError(error);
    Sentry.captureException(error);
    console.error(
      `Failed to run scenario ${scenario.name} with error: ${error}`,
    );
  };

  if (scenarioMessage.rate === null) {
    const numThreads = scenarioMessage.threads || 1;
    const threads = Array.from({ length: numThreads }, async () => {
      const client = new ConvexClient(config.deploymentUrl);
      try {
        for (;;) {
          await runScenarioOnce(createScenarioInstance, client, handleError);
        }
      } finally {
        await closeWithTimeout(client);
      }
    });
    await Promise.all(threads);
    return;
  }

  const rate = scenarioMessage.rate;
  await runAtRate({
    rate,
    run: async () => {
      const client = new ConvexClient(config.deploymentUrl);
      try {
        await runScenarioOnce(createScenarioInstance, client, handleError);
      } finally {
        await closeWithTimeout(client);
      }
    },
    onError: (error) => {
      Sentry.captureException(error);
      console.error(
        `Failed to run scenario ${scenarioName} with error: ${error}`,
      );
    },
    onReport: ({ issued, missed, inFlight }) => {
      console.log(
        `rate scenario ${scenarioName}: issued=${issued} missed=${missed} ` +
          `inFlight=${inFlight} target=${rate}/s`,
      );
    },
  });
}

const program = new Command();
program
  .name("scenario-runner")
  .description(
    "scenario-runner runs client-side scenarios against test deployments",
  )
  .usage("command url [options]")
  .requiredOption(
    "--deployment-url <url>",
    "URL of the deployment to run scenarios against",
  )
  .requiredOption("--admin-key <admin_key>", "Admin key to access deployment")
  .requiredOption(
    "--scenarios <scenarios>",
    "JSON scenarios to run against the given deployment",
  )
  .requiredOption(
    "--load-generator-port <port>",
    "Port to connect to load generator",
  )
  .option("--provision-host <host>", "Port to connect to big brain")
  .option("--access-token <token>", "Access token for talking to big-bran")
  .action(async (options) => {
    const scenarios = JSON.parse(options.scenarios);
    await main(
      options.deploymentUrl,
      options.adminKey,
      Number(options.loadGeneratorPort),
      options.provisionHost,
      options.accessToken,
      scenarios.scenarios,
    );
  });
void program.parseAsync(process.argv);
