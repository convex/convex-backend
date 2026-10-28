import { useQuery } from "convex/react";
import { useContext } from "react";
import udfs from "@common/udfs";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { ModuleFunction } from "@common/lib/functions/types";

export type HttpActionRoute =
  /** The absolute URL the HTTP action is served at. */
  | { status: "mounted"; url: string }
  /** The action's component isn't mounted over HTTP, so it serves no routes. */
  | { status: "unmounted" };

/**
 * Where an HTTP action is served, or `null` if it isn't an HTTP action (or the
 * deployment's routing info isn't loaded yet).
 */
export function useHttpActionRoute(
  moduleFunction: ModuleFunction,
): HttpActionRoute | null {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const isHttpAction = moduleFunction.udfType === "HttpAction" && canViewData;

  const convexSiteUrl = useQuery(
    udfs.convexSiteUrl.default,
    isHttpAction ? {} : "skip",
  );
  const components = useQuery(udfs.components.list, isHttpAction ? {} : "skip");

  if (!isHttpAction || !convexSiteUrl || components === undefined) {
    return null;
  }

  // Routes are served under the component's `httpPrefix`: the app's own
  // `httpPrefix` for the root, the path the parent mounted it at for a child.
  const component = moduleFunction.componentId
    ? components.find((c) => c.id === moduleFunction.componentId)
    : components.find((c) => c.path === "");
  const httpPrefix = component?.httpPrefix ?? null;
  if (moduleFunction.componentId && !httpPrefix) {
    return { status: "unmounted" };
  }

  // HTTP actions are named `"<METHOD> <path>"` — URI paths can't contain a raw
  // space.
  const routePath = moduleFunction.name.slice(
    moduleFunction.name.indexOf(" ") + 1,
  );
  return {
    status: "mounted",
    url: `${convexSiteUrl}${(httpPrefix ?? "").replace(/\/$/, "")}${routePath}`,
  };
}
