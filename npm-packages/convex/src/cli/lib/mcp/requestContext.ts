import { BigBrainAuth, Context, ErrorType } from "../../../bundler/context.js";
import { Filesystem, nodeFs } from "../../../bundler/fs.js";
import { Ora } from "ora";
import {
  DeploymentSelectionWithinProject,
  deploymentSelectionWithinProjectSchema,
  DeploymentSelectionOptions,
} from "../api.js";
import { z } from "zod";
import {
  DeploymentSelection,
  getDeploymentSelection,
} from "../deploymentSelection.js";

export interface McpOptions extends DeploymentSelectionOptions {
  projectDir?: string;
  disableTools?: string;
  dangerouslyEnableProductionDeployments?: boolean;
  cautiouslyAllowProductionPii?: boolean;
}

export class RequestContext implements Context {
  fs: Filesystem;
  deprecationMessagePrinted = false;
  spinner: Ora | undefined;
  _cleanupFns: Record<string, (exitCode: number, err?: any) => Promise<void>> =
    {};
  _bigBrainAuth: BigBrainAuth | null = null;
  constructor(public options: McpOptions) {
    this.fs = nodeFs;
    this.deprecationMessagePrinted = false;
  }

  async crash(args: {
    exitCode: number;
    errorType?: ErrorType;
    errForSentry?: any;
    printedMessage: string | null;
  }): Promise<never> {
    const cleanupFns = this._cleanupFns;
    this._cleanupFns = {};
    for (const fn of Object.values(cleanupFns)) {
      await fn(args.exitCode, args.errForSentry);
    }
    // eslint-disable-next-line no-restricted-syntax
    throw new RequestCrash(args.exitCode, args.errorType, args.printedMessage);
  }

  flushAndExit() {
    // eslint-disable-next-line no-restricted-syntax
    throw new Error("Not implemented");
  }

  registerCleanup(fn: (exitCode: number, err?: any) => Promise<void>): string {
    const handle = crypto.randomUUID();
    this._cleanupFns[handle] = fn;
    return handle;
  }

  removeCleanup(handle: string) {
    const value = this._cleanupFns[handle];
    delete this._cleanupFns[handle];
    return value ?? null;
  }

  bigBrainAuth(): BigBrainAuth | null {
    return this._bigBrainAuth;
  }

  _updateBigBrainAuth(auth: BigBrainAuth | null): void {
    this._bigBrainAuth = auth;
  }

  async decodeDeploymentSelector(encoded: string) {
    const { projectDir, deployment } = decodeDeploymentSelector(encoded);
    if (
      deployment.kind === "prod" &&
      !this.options.dangerouslyEnableProductionDeployments
    ) {
      return await this.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "This tool cannot be used with production deployments. Use a read-only tool like `insights` instead, or enable production access with --dangerously-enable-production-deployments.",
      });
    }
    return { projectDir, deployment };
  }

  /** Decode a deployment selector without checking the production guard. Use for read-only tools that don't expose PII (e.g. insights). */
  decodeDeploymentSelectorUnchecked(encoded: string) {
    return decodeDeploymentSelector(encoded);
  }

  /** Decode a deployment selector for read-only tools that may expose PII (e.g. data, logs, queries). Requires --cautiously-allow-production-pii. */
  async decodeDeploymentSelectorReadOnly(encoded: string) {
    const { projectDir, deployment } = decodeDeploymentSelector(encoded);
    if (
      deployment.kind === "prod" &&
      !this.options.dangerouslyEnableProductionDeployments &&
      !this.options.cautiouslyAllowProductionPii
    ) {
      return await this.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "This read-only tool may expose PII from production. Enable with --cautiously-allow-production-pii, or use --dangerously-enable-production-deployments for full access.",
      });
    }
    return { projectDir, deployment };
  }

  get productionDeploymentsDisabled() {
    return !this.options.dangerouslyEnableProductionDeployments;
  }

  get productionPiiAllowed() {
    return (
      this.options.dangerouslyEnableProductionDeployments ||
      this.options.cautiouslyAllowProductionPii
    );
  }
}

export class RequestCrash {
  printedMessage: string;
  constructor(
    private exitCode: number,
    private errorType: ErrorType | undefined,
    printedMessage: string | null,
  ) {
    this.printedMessage = printedMessage ?? "Unknown error";
  }

  toString(): string {
    return this.printedMessage;
  }
}

// Unfortunately, MCP clients don't seem to handle nested JSON objects very
// well (even though this is within spec). To work around this, encode the
// deployment selectors as an obfuscated string that the MCP client can
// opaquely pass around.
export function encodeDeploymentSelector(
  projectDir: string,
  deployment: DeploymentSelectionWithinProject,
) {
  const payload = {
    projectDir,
    deployment,
  };
  // ASCII JSON escapes support Unicode paths with the existing Latin-1 decoder.
  const serialized = JSON.stringify(payload).replace(
    /[\u0080-\uffff]/g,
    (character) =>
      `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`,
  );
  return `${deployment.kind}:${btoa(serialized)}`;
}

const payloadSchema = z.object({
  projectDir: z.string(),
  deployment: deploymentSelectionWithinProjectSchema,
});

function decodeDeploymentSelector(encoded: string) {
  const [_, serializedPayload] = encoded.split(":");
  return payloadSchema.parse(JSON.parse(atob(serializedPayload)));
}

/**
 * Get the deployment selection for MCP tools. The agent can pass different
 * values of `selectionWithinProject` into a tool, so we overwrite the
 * `selectionWithinProject` of the `DeploymentSelection` if it exists.
 */
export async function getMcpDeploymentSelection(
  ctx: RequestContext,
  decodedDeploymentSelector: DeploymentSelectionWithinProject,
): Promise<DeploymentSelection> {
  const initialSelection = await getDeploymentSelection(ctx, ctx.options);
  const hasSelectionWithinProject =
    initialSelection.kind !== "existingDeployment";
  return {
    ...initialSelection,
    ...(hasSelectionWithinProject && {
      selectionWithinProject: decodedDeploymentSelector,
    }),
  };
}
