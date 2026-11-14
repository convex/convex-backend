import { isDeepStrictEqual } from "node:util";
import { beforeEach, describe, expect, test, vi } from "vitest";
import {
  DeploymentSelectionWithinProject,
  deploymentSelectionWithinProjectFromOptions,
  DeploymentType,
  loadSelectedDeploymentCredentials,
} from "../../api.js";
import { getDeploymentSelection } from "../../deploymentSelection.js";
import {
  encodeDeploymentSelector,
  McpOptions,
  RequestContext,
} from "../requestContext.js";
import { convexTools } from "./index.js";

vi.mock("../../api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api.js")>()),
  loadSelectedDeploymentCredentials: vi.fn(),
}));

vi.mock("../../deploymentSelection.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../deploymentSelection.js")>()),
  getDeploymentSelection: vi.fn(),
}));

const PROD_DEPLOYMENT_NAME = "happy-otter-123";
const DEV_DEPLOYMENT_NAME = "dusty-heron-456";
const projectDir = process.cwd();

// Selectors that resolve to the production deployment without being of kind
// `prod`.
const productionSelectors: DeploymentSelectionWithinProject[] = [
  { kind: "deploymentName", deploymentName: PROD_DEPLOYMENT_NAME },
  { kind: "deploymentSelector", selector: "prod" },
  { kind: "deploymentSelector", selector: "team:project:prod" },
];

const developmentSelectors: DeploymentSelectionWithinProject[] = [
  { kind: "deploymentName", deploymentName: DEV_DEPLOYMENT_NAME },
  { kind: "deploymentSelector", selector: "dev" },
];

const mutatingToolArgs: Record<string, Record<string, unknown>> = {
  run: { functionName: "messages:send", args: "{}" },
  envList: {},
  envGet: { name: "SECRET" },
  envSet: { name: "SECRET", value: "value" },
  envRemove: { name: "SECRET" },
};

const piiToolArgs: Record<string, Record<string, unknown>> = {
  data: { tableName: "messages", order: "asc" },
  logs: {},
  runOneoffQuery: { query: "export default query({ handler: () => null })" },
};

// Tools read the admin key only to send a request, after the production guard.
// The mocked credentials record each read of it and then throw, so a test can
// tell whether a tool got past the guard while no request leaves the test.
const adminKeyReads: DeploymentType[] = [];

function resolveDeploymentType(selection: DeploymentSelectionWithinProject) {
  if (productionSelectors.some((s) => isDeepStrictEqual(s, selection))) {
    return "prod";
  }
  if (developmentSelectors.some((s) => isDeepStrictEqual(s, selection))) {
    return "dev";
  }
  // A tool resolved a selection other than the one it was given.
  throw new Error(
    `Unexpected deployment selection ${JSON.stringify(selection)}`,
  );
}

function getTool(name: string) {
  const tool = convexTools.find((t) => t.name === name);
  if (tool === undefined) {
    throw new Error(`Tool ${name} not found`);
  }
  return tool;
}

async function callTool(
  options: McpOptions,
  name: string,
  args: Record<string, unknown>,
) {
  const tool = getTool(name);
  return await tool.handler(
    new RequestContext(options),
    tool.inputSchema.parse(args),
  );
}

beforeEach(() => {
  adminKeyReads.length = 0;
  vi.mocked(getDeploymentSelection).mockImplementation(
    async (_ctx, options) => ({
      kind: "deploymentWithinProject",
      targetProject: {
        kind: "teamAndProjectSlugs",
        teamSlug: "team",
        projectSlug: "project",
      },
      selectionWithinProject:
        deploymentSelectionWithinProjectFromOptions(options),
    }),
  );
  vi.mocked(loadSelectedDeploymentCredentials).mockImplementation(
    async (_ctx, deploymentSelection) => {
      if (deploymentSelection.kind === "existingDeployment") {
        throw new Error("Unexpected existing deployment");
      }
      const deploymentType = resolveDeploymentType(
        deploymentSelection.selectionWithinProject,
      );
      return {
        get adminKey(): string {
          adminKeyReads.push(deploymentType);
          throw new Error(`Read the ${deploymentType} admin key`);
        },
        url: "http://127.0.0.1:1",
        deploymentFields: {
          deploymentName:
            deploymentType === "prod"
              ? PROD_DEPLOYMENT_NAME
              : DEV_DEPLOYMENT_NAME,
          deploymentType,
          projectSlug: "project",
          teamSlug: "team",
          reference: deploymentType === "prod" ? "production" : "dev/me",
          isDefault: true,
        },
      };
    },
  );
});

describe.each(productionSelectors)(
  "production deployment selected with %o",
  (deployment) => {
    const deploymentSelector = encodeDeploymentSelector(projectDir, deployment);

    test.each(Object.entries(mutatingToolArgs))(
      "%s is rejected",
      async (name, args) => {
        await expect(
          callTool({}, name, { deploymentSelector, ...args }),
        ).rejects.toMatchObject({
          printedMessage: expect.stringContaining(
            "This tool cannot be used with production deployments.",
          ),
        });
        expect(adminKeyReads).toEqual([]);
      },
    );

    test.each(Object.entries(piiToolArgs))(
      "%s is rejected",
      async (name, args) => {
        await expect(
          callTool({}, name, { deploymentSelector, ...args }),
        ).rejects.toMatchObject({
          printedMessage: expect.stringContaining(
            "This read-only tool may expose PII from production.",
          ),
        });
        expect(adminKeyReads).toEqual([]);
      },
    );
  },
);

describe.each(developmentSelectors)(
  "development deployment selected with %o",
  (deployment) => {
    const deploymentSelector = encodeDeploymentSelector(projectDir, deployment);

    test.each(Object.entries({ ...mutatingToolArgs, ...piiToolArgs }))(
      "%s gets past the guard",
      async (name, args) => {
        // Reading the admin key throws, so every call fails after the guard.
        await expect(
          callTool({}, name, { deploymentSelector, ...args }),
        ).rejects.toBeDefined();
        expect(adminKeyReads).toEqual(["dev"]);
      },
    );
  },
);

describe("status", () => {
  test("marks a production deployment selected by name as read-only", async () => {
    const result = await callTool(
      { deploymentName: PROD_DEPLOYMENT_NAME },
      "status",
      { projectDir },
    );
    expect(result.availableDeployments).toEqual([
      expect.objectContaining({ kind: "deploymentName", readOnly: true }),
    ]);
  });

  test("marks it as readable with --cautiously-allow-production-pii", async () => {
    const result = await callTool(
      {
        deploymentName: PROD_DEPLOYMENT_NAME,
        cautiouslyAllowProductionPii: true,
      },
      "status",
      { projectDir },
    );
    expect(result.availableDeployments).toEqual([
      expect.objectContaining({ kind: "deploymentName", readOnly: false }),
    ]);
  });

  test("doesn't mark it with --dangerously-enable-production-deployments", async () => {
    const result = await callTool(
      {
        deploymentName: PROD_DEPLOYMENT_NAME,
        dangerouslyEnableProductionDeployments: true,
      },
      "status",
      { projectDir },
    );
    expect(result.availableDeployments).toHaveLength(1);
    expect(result.availableDeployments[0]).not.toHaveProperty("readOnly");
  });

  test("doesn't mark a development deployment selected by name", async () => {
    const result = await callTool(
      { deploymentName: DEV_DEPLOYMENT_NAME },
      "status",
      { projectDir },
    );
    expect(result.availableDeployments).toHaveLength(1);
    expect(result.availableDeployments[0]).not.toHaveProperty("readOnly");
  });
});
