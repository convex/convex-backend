import { ChalkInstance } from "chalk";
import { logMessage } from "../../bundler/log.js";
import { chalkStderr } from "chalk";
import {
  deploymentDashboardUrl,
  deploymentDashboardUrlPage,
} from "./dashboard.js";
import type { DeploymentType, DetailedDeploymentCredentials } from "./api.js";

type AnnouncedDeployment = Omit<DetailedDeploymentCredentials, "adminKey">;

function stylesFor(dtype: DeploymentType | null): {
  label: string;

  bg: string;
  fg: string;
  bar: string;
} {
  switch (dtype) {
    case "prod":
      return {
        label: "Production",
        bg: "#eacae7",
        fg: "#641aa9",
        bar: "#8b21f1",
      };
    case "dev":
      return {
        label: "Development",
        bg: "#d2ecbb",
        fg: "#2b6536",
        bar: "#3ea34b",
      };
    case "local":
      return {
        label: "Local",
        bg: "#d2ecbb",
        fg: "#2b6536",
        bar: "#3ea34b",
      };
    case "anonymous":
      return {
        label: "Local",
        bg: "#d2ecbb",
        fg: "#2b6536",
        bar: "#3ea34b",
      };
    case "preview":
      return {
        label: "Preview",
        bg: "#fcedd7",
        fg: "#933615",
        bar: "#e25706",
      };
    case "custom":
      return {
        label: "Custom",
        bg: "#dfe2e9",
        fg: "#292b30",
        bar: "#989aa3",
      };
    case null:
      // Self-hosted / --url + --admin-key
      return {
        label: "",
        bg: "#dfe2e9",
        fg: "#292b30",
        bar: "#989aa3",
      };
    default:
      dtype satisfies never;
      return {
        label: "Unknown",
        bg: "#dfe2e9",
        fg: "#292b30",
        bar: "#989aa3",
      };
  }
}

function osc8Link(text: string, url: string): string {
  if (!process.stderr.isTTY) return text;
  return `\x1b]8;;${url}\x1b\\${text}\x1b]8;;\x1b\\`;
}

export type DeploymentAnnouncementHeader =
  | "Developing against deployment:"
  | "Showing logs of deployment:"
  | "Deploying code to deployment:"
  | null;

export function formatTargetedDeployment(
  header: DeploymentAnnouncementHeader,
  creds: AnnouncedDeployment,
  chalk: ChalkInstance,
): string {
  const fields = creds.deploymentFields;

  const style = stylesFor(fields?.deploymentType ?? null);
  const bar = chalk.hex(style.bar)("▌");
  const tag =
    chalk.level > 0
      ? chalk.bold.bgHex(style.bg).hex(style.fg)(` ${style.label} `)
      : `[${style.label}]`;
  const urlStyled = chalk.dim.underline(osc8Link(creds.url, creds.url));

  const dashUrl = dashboardUrlFor(fields);
  const dashboardSuffix = dashUrl
    ? process.stderr.isTTY
      ? ` (${osc8Link(chalk.underline("dashboard"), dashUrl)})`
      : ` (dashboard: ${dashUrl})`
    : "";

  let refLine: string | null;
  if (fields === null) {
    refLine = null;
  } else if (fields.deploymentType === "local") {
    const port = portFromUrl(creds.url);
    const portPart = port !== null ? `Port ${chalk.bold(port)}` : creds.url;
    const projectPart =
      fields.teamSlug && fields.projectSlug
        ? ` • in ${fields.teamSlug}${chalk.dim(":")}${fields.projectSlug}`
        : "";
    refLine = `${tag} ${portPart}${projectPart}${dashboardSuffix}`;
  } else if (fields.deploymentType === "anonymous") {
    const port = portFromUrl(creds.url);
    const portPart = port !== null ? `Port ${chalk.bold(port)}` : creds.url;
    refLine = `${tag} ${portPart} • No Convex account (run ${chalk.bold("npx convex login")} to link to a project)`;
  } else {
    // We display the deployment using the `team:project:ref`
    // syntax so that users can copy-paste the value when using `--deployment`.
    const { dimPrefix, boldTail, defaultAlias } = deploymentRefFor(fields);
    const aliasPart =
      defaultAlias === null
        ? ""
        : ` ${chalk.dim("(")}${chalk.bold(defaultAlias)}${chalk.dim(")")}`;
    refLine = `${tag} ${chalk.dim(dimPrefix)}${chalk.bold(boldTail)}${aliasPart}${dashboardSuffix}`;
  }

  return [
    header === null ? null : `${bar} ${header}`,
    refLine === null ? null : `${bar} ${refLine}`,
    `${bar} ${chalk.dim("└─")} ${urlStyled}`,
  ]
    .filter((x) => x !== null)
    .join("\n");
}

function portFromUrl(url: string): string | null {
  try {
    const parsed = new URL(url);
    return parsed.port || null;
  } catch {
    return null;
  }
}

export function announceDeploymentTarget(
  header: DeploymentAnnouncementHeader,
  creds: AnnouncedDeployment,
) {
  logMessage(formatTargetedDeployment(header, creds, chalkStderr));
}

function deploymentRefFor(
  fields: DetailedDeploymentCredentials["deploymentFields"] | null,
): {
  dimPrefix: string;
  boldTail: string;
  // For default dev/prod deployments, the short-form alias to append after
  // the reference in parens (e.g. "dev" or "prod"). `null` otherwise.
  defaultAlias: "dev" | "prod" | null;
} {
  if (fields === null) {
    return { dimPrefix: "", boldTail: "", defaultAlias: null };
  }
  if (fields.deploymentType === "local") {
    return { dimPrefix: "", boldTail: "local", defaultAlias: null };
  }
  const isCloud =
    fields.deploymentType === "dev" ||
    fields.deploymentType === "preview" ||
    fields.deploymentType === "prod" ||
    fields.deploymentType === "custom";
  if (isCloud && fields.teamSlug && fields.projectSlug && fields.reference) {
    const defaultAlias =
      fields.isDefault &&
      (fields.deploymentType === "dev" || fields.deploymentType === "prod")
        ? fields.deploymentType
        : null;
    return {
      dimPrefix: `${fields.teamSlug}:${fields.projectSlug}:`,
      boldTail: fields.reference,
      defaultAlias,
    };
  }
  return { dimPrefix: "", boldTail: fields.deploymentName, defaultAlias: null };
}

function dashboardUrlFor(
  fields: DetailedDeploymentCredentials["deploymentFields"] | null,
): string | null {
  if (fields === null || fields.deploymentType === "anonymous") {
    return null;
  }
  if (fields.teamSlug && fields.projectSlug) {
    return deploymentDashboardUrl(
      fields.teamSlug,
      fields.projectSlug,
      fields.deploymentName,
    );
  }
  return deploymentDashboardUrlPage(fields.deploymentName, "");
}
