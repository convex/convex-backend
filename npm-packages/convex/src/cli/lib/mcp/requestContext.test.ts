import { describe, expect, test } from "vitest";
import { DeploymentType, DetailedDeploymentCredentials } from "../api.js";
import { encodeDeploymentSelector, RequestContext } from "./requestContext.js";

const PRODUCTION_DISABLED =
  "This tool cannot be used with production deployments.";
const PRODUCTION_PII_DISABLED =
  "This read-only tool may expose PII from production.";

const NON_PRODUCTION_DEPLOYMENT_TYPES: DeploymentType[] = [
  "dev",
  "preview",
  "custom",
  "local",
  "anonymous",
];

function credentials(
  deploymentType: DeploymentType,
): DetailedDeploymentCredentials {
  return {
    adminKey: "admin-key",
    url: "http://127.0.0.1:1",
    deploymentFields: {
      deploymentName: "happy-otter-123",
      deploymentType,
      projectSlug: "project",
      teamSlug: "team",
      reference: null,
      isDefault: false,
    },
  };
}

const selfHostedCredentials: DetailedDeploymentCredentials = {
  adminKey: "admin-key",
  url: "http://127.0.0.1:1",
  deploymentFields: null,
};

describe("checkResolvedDeployment", () => {
  test("rejects production deployments", async () => {
    const ctx = new RequestContext({});
    await expect(
      ctx.checkResolvedDeployment(credentials("prod")),
    ).rejects.toMatchObject({
      printedMessage: expect.stringContaining(PRODUCTION_DISABLED),
    });
  });

  test("rejects production deployments when only PII access is allowed", async () => {
    const ctx = new RequestContext({ cautiouslyAllowProductionPii: true });
    await expect(
      ctx.checkResolvedDeployment(credentials("prod")),
    ).rejects.toMatchObject({
      printedMessage: expect.stringContaining(PRODUCTION_DISABLED),
    });
  });

  test("allows production deployments with --dangerously-enable-production-deployments", async () => {
    const ctx = new RequestContext({
      dangerouslyEnableProductionDeployments: true,
    });
    await expect(
      ctx.checkResolvedDeployment(credentials("prod")),
    ).resolves.toBeUndefined();
  });

  test.each(NON_PRODUCTION_DEPLOYMENT_TYPES)(
    "allows %s deployments",
    async (deploymentType) => {
      const ctx = new RequestContext({});
      await expect(
        ctx.checkResolvedDeployment(credentials(deploymentType)),
      ).resolves.toBeUndefined();
    },
  );

  test("allows deployments without deployment fields", async () => {
    const ctx = new RequestContext({});
    await expect(
      ctx.checkResolvedDeployment(selfHostedCredentials),
    ).resolves.toBeUndefined();
  });
});

describe("checkResolvedDeploymentReadOnly", () => {
  test("rejects production deployments", async () => {
    const ctx = new RequestContext({});
    await expect(
      ctx.checkResolvedDeploymentReadOnly(credentials("prod")),
    ).rejects.toMatchObject({
      printedMessage: expect.stringContaining(PRODUCTION_PII_DISABLED),
    });
  });

  test("allows production deployments with --cautiously-allow-production-pii", async () => {
    const ctx = new RequestContext({ cautiouslyAllowProductionPii: true });
    await expect(
      ctx.checkResolvedDeploymentReadOnly(credentials("prod")),
    ).resolves.toBeUndefined();
  });

  test("allows production deployments with --dangerously-enable-production-deployments", async () => {
    const ctx = new RequestContext({
      dangerouslyEnableProductionDeployments: true,
    });
    await expect(
      ctx.checkResolvedDeploymentReadOnly(credentials("prod")),
    ).resolves.toBeUndefined();
  });

  test.each(NON_PRODUCTION_DEPLOYMENT_TYPES)(
    "allows %s deployments",
    async (deploymentType) => {
      const ctx = new RequestContext({});
      await expect(
        ctx.checkResolvedDeploymentReadOnly(credentials(deploymentType)),
      ).resolves.toBeUndefined();
    },
  );
});

describe("decodeDeploymentSelector", () => {
  test("rejects selectors of kind prod before they are resolved", async () => {
    const ctx = new RequestContext({});
    const selector = encodeDeploymentSelector("/project", { kind: "prod" });
    await expect(ctx.decodeDeploymentSelector(selector)).rejects.toMatchObject({
      printedMessage: expect.stringContaining(PRODUCTION_DISABLED),
    });
    await expect(
      ctx.decodeDeploymentSelectorReadOnly(selector),
    ).rejects.toMatchObject({
      printedMessage: expect.stringContaining(PRODUCTION_PII_DISABLED),
    });
  });

  test("leaves other selectors to checkResolvedDeployment", async () => {
    const ctx = new RequestContext({});
    const deployment = {
      kind: "deploymentName" as const,
      deploymentName: "happy-otter-123",
    };
    const selector = encodeDeploymentSelector("/project", deployment);
    await expect(ctx.decodeDeploymentSelector(selector)).resolves.toEqual({
      projectDir: "/project",
      deployment,
    });
  });
});
