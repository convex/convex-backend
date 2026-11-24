import { ChangeEvent, lazy, Suspense, useRef } from "react";
import classNames from "classnames";
import {
  useFunctionSearchTerm,
  useRootEntries,
} from "@common/lib/functions/FunctionsProvider";
import { NentSwitcher } from "@common/elements/NentSwitcher";
import { TextInput } from "@ui/TextInput";

// Loaded on first render so that `@pierre/trees`, about 70 kB gzipped, is in
// its own chunk, fetched only when the feature flag is on.
const FunctionTree = lazy(() =>
  import("@common/features/functions/components/FunctionTree").then(
    (module) => ({ default: module.FunctionTree }),
  ),
);

export function VirtualizedDirectorySidebar({
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
  const searchBox = useRef<HTMLDivElement>(null);

  return (
    <div
      className={classNames(
        "flex h-full w-full pt-4 flex-col bg-background-secondary overflow-hidden mb-2",
      )}
      ref={ref}
    >
      <div className="mb-2 flex flex-col px-3">
        <NentSwitcher className="mb-4" />
        <h5>Functions</h5>
      </div>
      <div ref={searchBox} className="px-3 pb-1">
        <TextInput
          id="Search functions"
          label="Search functions"
          placeholder="Search functions..."
          onChange={onChange}
          defaultValue={searchTerm}
          type="search"
        />
      </div>
      <div className="min-h-0 w-full flex-1 pt-1 pb-4">
        <Suspense fallback={null}>
          <FunctionTree
            tree={rootEntries}
            searchTerm={searchTerm}
            onChangeFunction={onChangeFunction}
            searchBox={searchBox}
          />
        </Suspense>
      </div>
    </div>
  );
}
