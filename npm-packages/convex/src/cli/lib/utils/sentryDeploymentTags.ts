import * as Sentry from "@sentry/node";
import type { DeploymentType } from "../api.js";

/**
 * Whether a deployment type's names are safe to send to Sentry. Typed as a
 * `Record` over every `DeploymentType` so that a new type fails to compile
 * until it is listed here.
 */
const SENDS_DEPLOYMENT_NAME: Record<DeploymentType, boolean> = {
  prod: true,
  dev: true,
  preview: true,
  custom: true,
  // `local-<team>-<project>`
  local: false,
  // `anonymous-<project directory name>`
  anonymous: false,
};

/**
 * Tags later Sentry events with the deployment the command acts on, so CLI
 * errors can be tied to the affected deployment.
 *
 * Only cloud deployment names are sent: they are generated (`happy-animal-123`).
 * Local and anonymous deployment names contain team and project slugs or the
 * project's directory name, so only their type is sent.
 */
export function tagSentryWithDeployment(
  deploymentFields: {
    deploymentName: string;
    deploymentType: DeploymentType;
  } | null,
) {
  if (deploymentFields === null) {
    return;
  }
  Sentry.setTag("deployment_type", deploymentFields.deploymentType);
  if (SENDS_DEPLOYMENT_NAME[deploymentFields.deploymentType]) {
    Sentry.setTag("deployment", deploymentFields.deploymentName);
  }
}
