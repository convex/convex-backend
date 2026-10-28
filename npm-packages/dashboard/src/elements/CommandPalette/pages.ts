import type { PlatformDeploymentResponse, ProjectDetails } from "generatedApi";

// A layer in the palette's drill-in stack: a nested view the user has drilled
// into. The root is represented by an empty stack (no page); each drill pushes
// one of these.
export type PalettePage =
  | { type: "teams" }
  | { type: "projects" }
  | { type: "project"; project: ProjectDetails }
  // The deployments of a single project, without the project's own pages —
  // reached from the "Switch Deployment…" command.
  | { type: "deployments"; project: ProjectDetails }
  // Every deployment in the current team, across projects — reached from the
  // "Go to Deployment…" command when no project is selected.
  | { type: "teamDeployments" }
  | {
      type: "deployment";
      deployment: PlatformDeploymentResponse;
      projectSlug?: string;
    }
  | { type: "components" }
  | { type: "theme" }
  // Bulk-delete multiple projects in the current team.
  | { type: "deleteProjects" }
  // Picker mode (see picker.ts): hand a deployment back to the control that
  // opened the palette. `pickProject` is the root of that menu; menus open one
  // level in, on the deployments of the project they already point at.
  | { type: "pickProject" }
  | { type: "pickDeployment"; project: ProjectDetails };

// The input placeholder, scoped to whatever page the user has drilled into so
// it names what a search here will actually match (e.g. a deployment within the
// project you're switching inside of).
export function palettePlaceholder(
  page: PalettePage | undefined,
  teamName: string | undefined,
  projectName: string | undefined,
): string {
  switch (page?.type) {
    case undefined: {
      const scope =
        teamName && projectName
          ? `${teamName} and ${projectName}`
          : (teamName ?? projectName);
      return scope
        ? `Search for anything in ${scope}…`
        : "Search for anything…";
    }
    case "teams":
      return "Search for a team…";
    case "projects":
      return teamName
        ? `Search for a project in ${teamName}…`
        : "Search for a project…";
    case "project":
      return `Search in ${page.project.name || page.project.slug}…`;
    case "deployments":
      return `Search for a deployment in ${page.project.name || page.project.slug}…`;
    case "teamDeployments":
      return teamName
        ? `Search for a deployment in ${teamName}…`
        : "Search for a deployment…";
    case "deployment":
      return `Search in ${pageLabel(page)}…`;
    case "components":
      return "Search for a component…";
    case "theme":
      return "Search for a theme…";
    case "deleteProjects":
      return teamName
        ? `Search for a project to delete in ${teamName}…`
        : "Search for a project to delete…";
    case "pickProject":
      return teamName
        ? `Search for a project in ${teamName}…`
        : "Search for a project…";
    case "pickDeployment":
      return `Search for a deployment in ${page.project.name || page.project.slug}…`;
    default:
      page satisfies never;
      return "Search for anything…";
  }
}

export function pageLabel(page: PalettePage): string {
  switch (page.type) {
    case "teams":
      return "Switch Team";
    case "projects":
      return "Switch Project";
    case "project":
      return page.project.name || page.project.slug;
    case "deployments":
      return "Switch Deployment";
    case "teamDeployments":
      return "Go to Deployment";
    case "deployment":
      return "reference" in page.deployment
        ? page.deployment.reference
        : page.deployment.name;
    case "components":
      return "Switch Component";
    case "theme":
      return "Change Dashboard Theme";
    case "deleteProjects":
      return "Delete Projects";
    case "pickProject":
      return "Select Project";
    case "pickDeployment":
      return page.project.name || page.project.slug;
    default: {
      page satisfies never;
      return "";
    }
  }
}
