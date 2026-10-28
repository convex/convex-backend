import { ChangeEvent, useRef } from "react";
import classNames from "classnames";
import {
  useFunctionSearchTerm,
  useRootEntries,
} from "@common/lib/functions/FunctionsProvider";
import { FileTree } from "@common/features/functions/components/FileTree";
import { NentSwitcher } from "@common/elements/NentSwitcher";
import { MagnifyingGlassIcon } from "@radix-ui/react-icons";

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
          defaultOpen
          nestingLevel={0}
        />
      </div>
    </div>
  );
}
