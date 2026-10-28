import { MultiSelectCombobox, MultiSelectValue } from "@ui/MultiSelectCombobox";
import { NentNameOption } from "@common/elements/NentSwitcher";
import { functionIdentifierFromValue } from "@common/lib/functions/generateFileTree";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import { NENT_APP_PLACEHOLDER } from "@common/lib/useNents";

export function LogToolbar({
  functions,
  selectedFunctions,
  setSelectedFunctions,
  selectedLevels,
  setSelectedLevels,
  nents,
  selectedNents,
  setSelectedNents,
  firstItem,
  hideFunctionFilter = false,
}: {
  functions: string[];
  selectedFunctions: MultiSelectValue;
  setSelectedFunctions: (selectedFunctions: MultiSelectValue) => void;
  selectedLevels: MultiSelectValue;
  setSelectedLevels: (selectedLevels: MultiSelectValue) => void;
  nents?: string[];
  selectedNents: MultiSelectValue;
  setSelectedNents(newValue: MultiSelectValue): void;
  firstItem?: React.ReactNode;
  hideFunctionFilter?: boolean;
}) {
  // Transform functions for current nents
  const functionsForCurrentNents = functionsForSelectedNents(
    selectedNents,
    functions,
  );

  // Get the filtered selected functions based on current nents
  const selectedFunctionsFiltered =
    selectedFunctions === "all"
      ? "all"
      : // Use as string[] assertion since we know it's not "all" at this point
        (functionsForSelectedNents(
          selectedNents,
          selectedFunctions as string[],
        ) as string[]);

  return (
    <div className="flex w-full flex-wrap items-center justify-end gap-2">
      {firstItem}
      {nents && (
        <div className="min-w-38">
          <MultiSelectCombobox
            options={nents}
            selectedOptions={selectedNents}
            setSelectedOptions={selectNentOption({
              selectedNents,
              setSelectedNents,
              functions,
              selectedFunctions,
              setSelectedFunctions,
            })}
            unit="component"
            unitPlural="components"
            label="Components"
            labelHidden
            Option={NentNameOption}
          />
        </div>
      )}
      {!hideFunctionFilter && (
        <div className="min-w-38">
          <MultiSelectCombobox
            options={functionsForCurrentNents as string[]}
            selectedOptions={selectedFunctionsFiltered}
            processFilterOption={(option) => {
              const id = functionIdentifierFromValue(option);
              return id.componentPath
                ? `${id.componentPath}/${id.identifier}`
                : id.identifier;
            }}
            setSelectedOptions={setSelectedFunctions}
            unit="function"
            unitPlural="functions"
            label="Functions"
            labelHidden
            Option={FunctionNameOption}
          />
        </div>
      )}
      <div className="min-w-38">
        <MultiSelectCombobox
          options={["success", "failure", "DEBUG", "INFO", "WARN", "ERROR"]}
          selectedOptions={selectedLevels}
          setSelectedOptions={setSelectedLevels}
          disableSearch
          unit="log types"
          unitPlural="log types"
          label="Log Types"
          labelHidden
          Option={({ label }) => (
            <div className="flex w-full justify-between">
              {label === "INFO" ? "log / info " : label.toLowerCase()}{" "}
            </div>
          )}
        />
      </div>
    </div>
  );
}

export const selectNentOption =
  ({
    // Renamed to avoid unused variable warning
    selectedNents: _,
    functions,
    selectedFunctions,
    setSelectedFunctions,
    setSelectedNents,
  }: {
    selectedNents: MultiSelectValue;
    functions: string[];
    selectedFunctions: MultiSelectValue;
    setSelectedFunctions: (f: MultiSelectValue) => void;
    setSelectedNents: (f: MultiSelectValue) => void;
  }) =>
  (newNents: MultiSelectValue) => {
    if (newNents === "all") {
      // If all nents are selected, we also select all functions
      setSelectedFunctions("all");
      setSelectedNents(newNents);
      return;
    }

    const availableFunctions = functionsForSelectedNents(
      newNents,
      functions,
    ) as string[];

    // If all available functions are selected, use "all" state
    if (availableFunctions.length > 0) {
      if (selectedFunctions === "all") {
        setSelectedFunctions("all");
      } else if (Array.isArray(selectedFunctions)) {
        const allSelected = availableFunctions.every((f: string) =>
          selectedFunctions.includes(f),
        );

        if (allSelected) {
          setSelectedFunctions("all");
        } else {
          const retainedFunctions = functionsForSelectedNents(
            newNents,
            selectedFunctions,
          ) as string[];

          setSelectedFunctions(
            Array.from(new Set([...availableFunctions, ...retainedFunctions])),
          );
        }
      }
    }

    setSelectedNents(newNents);
  };

export const functionsForSelectedNents = (
  nents: MultiSelectValue,
  functions: string[],
): string[] | "all" => {
  const nentArray = nents === "all" ? [] : nents;
  const functionArray = Array.isArray(functions) ? functions : [];

  return functionArray.filter((f) => {
    const functionIdentifier = functionIdentifierFromValue(f);
    return (
      nentArray.length === 0 ||
      nentArray.some((nent) =>
        nent === NENT_APP_PLACEHOLDER
          ? functionIdentifier.componentPath === undefined
          : nent === functionIdentifier.componentPath,
      )
    );
  });
};
