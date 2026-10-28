import { useQuery } from "convex/react";
import { Value } from "convex/values";
import { useRouter } from "next/router";
import { useContext, useMemo } from "react";
import { api } from "system-udfs/convex/_generated/api";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { PermissionsContext } from "@common/lib/deploymentContext";

export const NENT_APP_PLACEHOLDER = "_App";

export type ComponentId = Id<"_components"> | null;
export type Nent = {
  name: string | null;
  id: ComponentId;
  path: string;
  args: Record<string, Value>;
  state?: "active" | "unmounted";
};

// Using a stable reference to avoid rerenders
const EMPTY_ARRAY = [] as const;

export function useNents(): {
  nents?: Nent[];
  selectedNent: Nent | null;
  setSelectedNent: (nent?: string) => Promise<void>;
} {
  const { query, pathname, push } = useRouter();
  // Get canViewDataCached from a special memoized context because useNents is called in a lot of components,
  // which may have hundreds of instantiations.
  const { canViewDataCached } = useContext(PermissionsContext);
  const allComponentsOrSkipped = useQuery(
    api._system.frontend.components.list,
    canViewDataCached ? {} : "skip",
  );
  const allComponents = canViewDataCached
    ? allComponentsOrSkipped
    : EMPTY_ARRAY;

  // Ensure the selected component is in the list of all components
  if (allComponents !== undefined && typeof query.component === "string") {
    const found = allComponents.find((c) => c.id === query.component);
    if (!found) {
      delete query.component;
    }
  }

  const selectedNent = (query.component as string | undefined) ?? null;

  const setSelectedNent = async (nent?: string) => {
    const newQuery = { ...query };
    if (nent) {
      newQuery.component = nent;
    } else {
      delete newQuery.component;
    }
    await push({ pathname, query: newQuery }, undefined, { shallow: true });
  };

  const nents: Nent[] | undefined = useMemo(
    () =>
      allComponents
        ? [
            {
              name: null,
              id: NENT_APP_PLACEHOLDER as Id<"_components">,
              path: NENT_APP_PLACEHOLDER,
              args: {},
              state: "active",
            },
            ...allComponents,
          ]
        : undefined,
    [allComponents],
  );

  return {
    selectedNent: allComponents?.find((n) => n.id === selectedNent) || null,
    setSelectedNent,
    nents,
  };
}
