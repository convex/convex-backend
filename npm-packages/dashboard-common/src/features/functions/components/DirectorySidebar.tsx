import { ChangeEvent, useRef } from "react";
import classNames from "classnames";
import {
  useFunctionSearchTerm,
  useRootEntries,
} from "@common/lib/functions/FunctionsProvider";
import { FileTree } from "@common/features/functions/components/FileTree";
import { NentSwitcher } from "@common/elements/NentSwitcher";
import { MagnifyingGlassIcon } from "@radix-ui/react-icons";
import type { FileOrFolder } from "@common/lib/functions/types";

// Opening every root-level entry mounts a row per function of each root file,
// which deployments with a flat directory of thousands of functions can't
// render. Above this many rows, root entries start collapsed.
export const MAX_AUTO_EXPANDED_ROWS = 200;

// Rows the sidebar mounts when root entries start open: each open folder shows
// its direct children, and each open file shows its functions.
function expandedRowCount(rootEntries: FileOrFolder[]) {
  let rows = 0;
  for (const entry of rootEntries) {
    rows +=
      1 + (entry.type === "folder" ? entry.children : entry.functions).length;
  }
  return rows;
}

export function DirectorySidebar({
  onChangeFunction,
}: {
  onChangeFunction: () => void;
}) {
  const [searchTerm, setSearchTerm] = useFunctionSearchTerm();
  const rootEntries = useRootEntries();
  const onChange = (input: ChangeEvent<HTMLInputElement>) => {
    setSearchTerm(input.currentTarget.value);
  };

  const ref = useRef<HTMLDivElement>(null);

  return (
    <div
      className={classNames(
        "flex h-full w-full pt-4 flex-col bg-background-secondary overflow-x-hidden scrollbar mb-2",
      )}
      ref={ref}
    >
      <div className="mb-2 flex flex-col px-3">
        <NentSwitcher className="mb-4" />
        <h5>Functions</h5>
      </div>
      <div className="flex items-center gap-1 border-b px-3 py-1.5">
        <MagnifyingGlassIcon className="text-content-secondary" />
        <input
          id="Search functions"
          placeholder="Search functions..."
          onChange={onChange}
          defaultValue={searchTerm}
          type="search"
          className={classNames(
            "placeholder:text-content-tertiary truncate relative w-full text-left text-xs text-content-primary disabled:bg-background-tertiary disabled:text-content-secondary disabled:cursor-not-allowed",
            "focus:outline-hidden bg-background-secondary font-normal",
          )}
        />
      </div>
      <div className="scrollbar w-full overflow-x-hidden pt-1 pb-4">
        <FileTree
          tree={rootEntries}
          onChangeFunction={onChangeFunction}
          defaultOpen={expandedRowCount(rootEntries) <= MAX_AUTO_EXPANDED_ROWS}
          nestingLevel={0}
        />
      </div>
    </div>
  );
}
