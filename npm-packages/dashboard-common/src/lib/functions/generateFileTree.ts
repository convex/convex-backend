import {
  AnalyzedModuleFunction,
  Module,
} from "system-udfs/convex/_system/frontend/common";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { test } from "fuzzy";
import { ComponentId, Nent } from "@common/lib/useNents";
import {
  File,
  FileOrFolder,
  Folder,
  ModuleFunction,
} from "@common/lib/functions/types";

export const ROOT_PATH = "";

export function functionIdentifierValue(
  identifier: string,
  componentPath?: string,
  componentId?: string,
) {
  return JSON.stringify({ identifier, componentPath, componentId });
}

export function functionIdentifierFromValue(value: string) {
  return JSON.parse(value) as {
    identifier: string;
    componentPath?: string;
    componentId?: Id<"_components">;
  };
}

export function generateFileTreeAllNents(
  modulesAllNents: Map<ComponentId | null, Map<string, Module>>,
  nents: Nent[] | undefined,
  searchTerm: string = "",
): Map<string, File | Folder | ModuleFunction> {
  if (!modulesAllNents) {
    return new Map();
  }
  const appTree = generateFileTree(
    new Map(modulesAllNents.get(null)?.entries() ?? []),
    null,
    null,
    searchTerm,
  );
  const tree = new Map();
  for (const [path, node] of appTree.entries()) {
    tree.set(functionIdentifierValue(path), node);
  }
  for (const [componentId, modulesInNent] of modulesAllNents.entries()) {
    const nent = nents?.find((n) => n.id === componentId);
    if (componentId !== null && !!nent) {
      const nentPath = nent.path;
      const componentTree = generateFileTree(
        modulesInNent,
        componentId,
        nentPath,
        searchTerm,
      );
      for (const node of componentTree.values()) {
        node.componentPath = nentPath;
        node.componentId = componentId;
        tree.set(
          functionIdentifierValue(
            node.identifier,
            node.componentPath,
            node.componentId,
          ),
          node,
        );
      }
    } else {
      // Skip unmounted components in tree.
    }
  }
  return tree;
}

function moduleDisplayName(filePath: string) {
  // First, ensure all parent directories exist.
  const pathComponents = filePath.split("/");
  if (!pathComponents.length) {
    throw new Error(`Invalid module path: ${filePath}`);
  }
  // Module display name is the last component of the path without the
  // extension
  const displayModuleName =
    pathComponents[pathComponents.length - 1].split(".")[0];
  return displayModuleName;
}

export function generateFileTree(
  modules: Map<string, Module>,
  componentId: ComponentId,
  componentPath: string | null,
  searchTerm: string = "",
): Map<string, File | Folder | ModuleFunction> {
  const nodes: Map<string, File | Folder | ModuleFunction> = new Map();
  for (const [filePath, module] of modules.entries()) {
    let { functions } = module;
    // If we have a search query, skip modules that don't match.
    if (searchTerm !== "") {
      const filteredFunctions = functions.filter((f) =>
        test(searchTerm, `${filePath}:${f.name}`),
      );
      const functionNameMatches = filteredFunctions.length > 0;
      if (functionNameMatches) {
        functions = filteredFunctions;
      } else {
        continue;
      }
    }

    // We don't display any file uploaded to convex that isn't a function.
    if (!functions || functions.length === 0) {
      continue;
    }

    const pathComponents = filePath.split("/");
    // NB: `i = 0` creates the root directory as the empty string.
    for (let i = 0; i < pathComponents.length; i++) {
      const parentPath = pathComponents.slice(0, i).join("/");
      if (nodes.has(parentPath)) {
        continue;
      }
      const directory: Folder = {
        name: pathComponents[i - 1],
        identifier: parentPath,
        type: "folder",
        children: [],
        componentId,
        componentPath,
      };
      nodes.set(parentPath, directory);
    }

    const file: File = {
      name: moduleDisplayName(filePath),
      identifier: filePath,
      type: "file",
      functions: [],
      componentId,
      componentPath,
    };
    nodes.set(filePath, file);

    for (const moduleFunction of functions) {
      const f = processAnalyzedModuleFunction(
        moduleFunction,
        file.identifier,
        componentId,
        componentPath,
      );
      file.functions.push(f);
      nodes.set(f.identifier, f);
    }

    // Sort functions by line number.
    file.functions.sort((a, b) => (a.lineno ?? -1) - (b.lineno ?? -1));
  }

  // Stitch together the `children` children lists.
  for (const [filePath, node] of nodes.entries()) {
    // The root doesn't have a parent.
    if (filePath === ROOT_PATH) {
      continue;
    }
    // We only need to link files and folders into their parents.
    if (!(node.type === "file" || node.type === "folder")) {
      continue;
    }
    const pathComponents = filePath.split("/");
    const parentPath = pathComponents
      .slice(0, pathComponents.length - 1)
      .join("/");
    const parent = nodes.get(parentPath);
    if (!parent || parent.type !== "folder") {
      throw new Error(`Invalid parent at ${parentPath} for ${filePath}`);
    }
    parent.children.push(node);
  }

  // Sort the child lists.
  for (const fileOrDirectory of nodes.values()) {
    if (fileOrDirectory.type === "folder") {
      fileOrDirectory.children.sort((a, b) => {
        const sortKey = (f: FileOrFolder) =>
          `${f.type === "folder" ? "0" : "1"}:${f.name}`;
        return sortKey(a).localeCompare(sortKey(b));
      });
    }
  }
  return nodes;
}

export function processAnalyzedModuleFunction(
  moduleFunction: AnalyzedModuleFunction,
  filePath: string,
  componentId: ComponentId,
  componentPath: string | null,
): ModuleFunction {
  const identifier =
    moduleFunction.udfType === "HttpAction"
      ? moduleFunction.name
      : `${filePath}:${moduleFunction.name}`;

  const name =
    moduleFunction.udfType !== "HttpAction" && moduleFunction.name === "default"
      ? moduleDisplayName(filePath)
      : moduleFunction.name;

  const nameToDisplay =
    moduleFunction.udfType === "HttpAction" ? name : displayName(identifier);

  return {
    name,
    displayName: nameToDisplay,
    type: "function",
    udfType: moduleFunction.udfType,
    identifier,
    visibility: moduleFunction.visibility,
    lineno: moduleFunction.lineno,
    componentId,
    componentPath,
    file: {
      name: moduleDisplayName(filePath),
      identifier: filePath,
    },
  };
}

export function displayName(
  udfPath: string,
  componentPath?: string | null,
): string {
  let filePart: string = "";
  let exportName: string = "default";

  if (udfPath.includes(":")) {
    [filePart, exportName] = udfPath.split(":");
  } else {
    filePart = udfPath;
  }
  if (filePart.endsWith(".js")) {
    filePart = filePart.slice(0, filePart.length - 3);
  }
  const componentSuffix = componentPath ? ` in ${componentPath}` : "";

  return (
    filePart +
    (exportName === "default" ? "" : `:${exportName}`) +
    componentSuffix
  );
}
