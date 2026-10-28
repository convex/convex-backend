import { useQuery } from "convex/react";
import { useContext, useMemo } from "react";
import udfs from "@common/udfs";
import { Module } from "system-udfs/convex/_system/frontend/common";
import { ComponentId, useNents } from "@common/lib/useNents";
import { PermissionsContext } from "@common/lib/deploymentContext";

export function useListModules(): Map<string, Module> | undefined {
  const { selectedNent } = useNents();
  const modulesAllNents = useListModulesAllNents();

  return useMemo(
    () => modulesAllNents?.get(selectedNent?.id ?? null) ?? undefined,
    [modulesAllNents, selectedNent],
  );
}

export function useListModulesAllNents():
  | Map<ComponentId | null, Map<string, Module>>
  | undefined {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const rawModulesOrSkipped = useQuery(
    udfs.modules.listForAllComponents,
    canViewData ? {} : "skip",
  );

  const allModules: Map<ComponentId | null, Map<string, Module>> | undefined =
    useMemo(() => {
      const rawModules = canViewData ? rawModulesOrSkipped : [];
      if (rawModules === undefined) {
        return undefined;
      }

      const allModulesMap = new Map<ComponentId | null, Map<string, Module>>();
      for (const [componentId, modules] of rawModules) {
        allModulesMap.set(
          componentId as ComponentId | null,
          new Map(modules as [string, Module][]),
        );
      }

      return allModulesMap;
    }, [canViewData, rawModulesOrSkipped]);

  return allModules;
}
