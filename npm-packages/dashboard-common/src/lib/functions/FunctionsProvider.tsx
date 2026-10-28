import { ReactNode, useContext, useMemo } from "react";
import { useRouter } from "next/router";
import { cn } from "@ui/cn";
import { Module } from "system-udfs/convex/_system/frontend/common";
import { createGlobalState } from "react-use";
import {
  functionIdentifierValue,
  generateFileTreeAllNents,
  processAnalyzedModuleFunction,
  ROOT_PATH,
} from "@common/lib/functions/generateFileTree";
import { ModuleFunction } from "@common/lib/functions/types";
import { useListModulesAllNents } from "@common/lib/functions/useListModules";
import { createContextHook } from "@common/lib/createContextHook";
import { ComponentId, Nent, useNents } from "@common/lib/useNents";
import { LoadingLogo } from "@ui/Loading";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

const [FunctionsContext, useFunctions] = createContextHook<
  Map<ComponentId, Map<string, Module>>
>({
  name: "Functions",
});

// Duplicated from convex/server
export const ROUTABLE_HTTP_METHODS = [
  "GET",
  "POST",
  "PUT",
  "DELETE",
  "OPTIONS",
  "PATCH",
] as const;

export function displayNameToIdentifier(path: string) {
  // HTTP actions are special-cased top-level functions
  for (const method of ROUTABLE_HTTP_METHODS) {
    if (path.startsWith(`${method} `)) {
      let route = path.substring(method.length + 1);
      try {
        const url = new URL(route);
        route = url.pathname + url.search + url.hash;
      } catch {
        // Not a valid URL, keep route as is
      }
      return `${method} ${route}`;
    }
  }

  let filePath = "";
  let exportName: string = "default";

  if (path.includes(":")) {
    [filePath, exportName] = path.split(":");
  } else {
    filePath = path;
  }
  if (!filePath.endsWith(".js")) {
    filePath = `${filePath}.js`;
  }

  return `${filePath}:${exportName}`;
}

export const useFunctionSearchTerm = createGlobalState("");

function FunctionsProvider({ children }: { children: ReactNode }) {
  // Get functions
  const modules = useListModulesAllNents();
  const { nents } = useNents();

  if (!modules || !nents) {
    return (
      <div className={cn("flex size-full items-center justify-center")}>
        <LoadingLogo />
      </div>
    );
  }

  return (
    <FunctionsContext.Provider value={modules}>
      {children}
    </FunctionsContext.Provider>
  );
}

// For mocking in tests only
export { FunctionsContext };

export { FunctionsProvider };

// Returns the currently open function, or null if none is open
// The currently open function refers to the function that is currently being viewed
// on the functions page
export function useCurrentOpenFunction() {
  const router = useRouter();
  const moduleFunctions = useModuleFunctions();
  const { selectedNent, nents } = useNents();

  const selectedModule = useMemo(() => {
    if (!router.query.function) {
      return null;
    }

    const functionIdentifier = displayNameToIdentifier(
      router.query.function as string,
    );
    let componentId = selectedNent?.id ?? null;
    let componentPath =
      componentId === null
        ? undefined
        : nents?.find((n) => n.id === componentId)?.path;
    if (router.query.componentPath) {
      componentPath = router.query.componentPath as string;
      componentId = nents?.find((n) => n.path === componentPath)?.id ?? null;
    }

    const selectedFunctionIdentifier = functionIdentifierValue(
      functionIdentifier,
      componentPath,
      componentId || undefined,
    );

    const currentOpenFunction = selectedFunctionIdentifier
      ? moduleFunctions.find(
          (f) => itemIdentifier(f) === selectedFunctionIdentifier,
        )
      : undefined;

    if (
      router.query.function &&
      moduleFunctions.length > 0 &&
      !currentOpenFunction
    ) {
      delete router.query.function;
      void router.replace({ query: router.query }, undefined, {
        shallow: true,
      });
      return null;
    }

    return currentOpenFunction;
  }, [router, selectedNent?.id, nents, moduleFunctions]);

  return selectedModule;
}

// Returns a flat list of the functions across all components.
export function useModuleFunctions(): ModuleFunction[] {
  const modules = useFunctions();
  const { nents } = useNents();
  const { captureMessage } = useContext(DeploymentInfoContext);

  return useMemo(() => {
    if (!nents) {
      captureMessage(
        "File tree map called before modules or nents were loaded",
        "error",
      );
      return [];
    }

    return modulesToModuleFunctions(modules, nents);
  }, [captureMessage, modules, nents]);
}

// Exported for testing only
export function modulesToModuleFunctions(
  modules: Map<ComponentId | null, Map<string, Module>>,
  nents: Nent[],
) {
  const moduleFunctions: ModuleFunction[] = [];
  for (const [componentId, componentModules] of modules.entries()) {
    for (const [filePath, { functions }] of componentModules.entries()) {
      for (const moduleFunction of functions) {
        moduleFunctions.push(
          processAnalyzedModuleFunction(
            moduleFunction,
            filePath,
            componentId,
            nents.find((n) => n.id === componentId)?.path ?? null,
          ),
        );
      }
    }
  }
  return moduleFunctions;
}

// Returns a tree of functions within the current component.
export function useRootEntries() {
  const modules = useFunctions();
  const [searchTerm] = useFunctionSearchTerm();
  const { selectedNent, nents } = useNents();
  const { captureMessage } = useContext(DeploymentInfoContext);

  const rootEntries = useMemo(() => {
    if (!nents) {
      captureMessage("Root entries called before nents were loaded", "error");
      return [];
    }

    const filteredFileTreeMapAllNents = generateFileTreeAllNents(
      modules,
      nents,
      searchTerm,
    );

    const filteredFileTreeMap = new Map(
      Array.from(filteredFileTreeMapAllNents.entries()).filter(([, value]) =>
        selectedNent
          ? selectedNent.id === value.componentId
          : !value.componentId,
      ),
    );

    const rootDirectory = Array.from(filteredFileTreeMap.values()).find(
      (value) => value.identifier === ROOT_PATH,
    );

    if (rootDirectory && rootDirectory.type !== "folder") {
      captureMessage("Root directory is not a folder", "error");
      return [];
    }

    return rootDirectory?.children ?? [];
  }, [captureMessage, modules, nents, searchTerm, selectedNent]);
  return rootEntries;
}

export function itemIdentifier(item: ModuleFunction) {
  return functionIdentifierValue(
    item.identifier,
    item.componentPath ?? undefined,
    item.componentId ?? undefined,
  );
}
