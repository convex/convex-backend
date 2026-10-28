import { FolderIcon, FolderOpenIcon } from "@heroicons/react/24/outline";
import { Disclosure, DisclosurePanel } from "@headlessui/react";
import { FileItem } from "@common/features/functions/components/FileItem";
import { DirectoryItem } from "@common/features/functions/components/FunctionItem";
import type { FileOrFolder, Folder } from "@common/lib/functions/types";
import { useCurrentOpenFunction } from "@common/lib/functions/FunctionsProvider";

type FileTreeProps = {
  tree: FileOrFolder[];
  onChangeFunction: () => void;
  defaultOpen?: boolean;
  nestingLevel: number;
};

function FolderItem({
  folder,
  onChangeFunction,
  defaultOpen = false,
  nestingLevel,
}: {
  folder: Folder;
  defaultOpen?: boolean;
  onChangeFunction: () => void;
  nestingLevel: number;
}) {
  const currentOpenFunction = useCurrentOpenFunction();
  const descendentIsCurrentFunction =
    currentOpenFunction?.file.identifier.startsWith(folder.identifier);
  return (
    <Disclosure
      key={folder.name}
      defaultOpen={descendentIsCurrentFunction || defaultOpen}
    >
      {({ open }) => (
        <>
          <DirectoryItem disclosure nestingLevel={nestingLevel}>
            {/* This div makes sure the icon is not resized  */}
            <div className="w-4">
              {open ? (
                <FolderOpenIcon className="size-4 text-content-tertiary" />
              ) : (
                <FolderIcon className="size-4 text-content-tertiary" />
              )}
            </div>
            <div className="truncate">{folder.name}</div>
          </DirectoryItem>
          {folder.children !== undefined && (
            <DisclosurePanel>
              <FileTree
                tree={folder.children}
                onChangeFunction={onChangeFunction}
                nestingLevel={nestingLevel + 1}
              />
            </DisclosurePanel>
          )}
        </>
      )}
    </Disclosure>
  );
}

// Recursively rendered FileTree
export function FileTree({
  tree,
  onChangeFunction,
  nestingLevel,
  defaultOpen = false,
}: FileTreeProps) {
  if (!tree || tree.length === 0) {
    return (
      <span className="text-xs text-content-secondary">
        No functions match your search.
      </span>
    );
  }
  return (
    <div className="flex w-full flex-col">
      {tree.map((item) => {
        if (item.type === "file") {
          return (
            <FileItem
              key={item.identifier}
              file={item}
              onChangeFunction={onChangeFunction}
              defaultOpen={defaultOpen}
              nestingLevel={nestingLevel}
            />
          );
        }
        if (item.type === "folder") {
          return (
            <FolderItem
              key={item.identifier}
              folder={item}
              onChangeFunction={onChangeFunction}
              defaultOpen={defaultOpen}
              nestingLevel={nestingLevel}
            />
          );
        }
        throw new Error(`Can't create a FileTree from ${item}`);
      })}
    </div>
  );
}
