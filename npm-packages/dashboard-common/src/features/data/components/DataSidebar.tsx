import { MagnifyingGlassIcon, PlusIcon } from "@radix-ui/react-icons";
import { useMutation } from "convex/react";
import classNames from "classnames";
import { useContext, useState } from "react";
import udfs from "@common/udfs";
import { TextInput } from "@ui/TextInput";
import {
  isTableMissingFromSchema,
  useActiveSchema,
  validateConvexIdentifier,
} from "@common/features/data/lib/helpers";
import { TableTab } from "@common/features/data/components/TableTab";
import { TableMetadata } from "@common/lib/useTableMetadata";
import { NentSwitcher } from "@common/elements/NentSwitcher";
import { Button } from "@ui/Button";
import { useNents } from "@common/lib/useNents";
import { useIsNarrowScreen } from "@ui/useIsNarrowScreen";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { toast } from "@common/lib/utils";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";

export function DataSidebar({
  tableData,
  onSelectTable,
  onTableCreated,
}: {
  tableData: TableMetadata;
  onSelectTable?: () => void;
  onTableCreated?: () => void;
}) {
  const { name: selectedTable, tableNames } = tableData;

  const [searchQuery, setSearchQuery] = useState("");
  const searchQueryLowercase = searchQuery.toLowerCase();
  const schema = useActiveSchema();

  const isNarrow = useIsNarrowScreen();

  return (
    <div
      className={classNames(
        "flex w-full h-full flex-col bg-background-secondary scrollbar",
        "py-4",
      )}
    >
      {!isNarrow && (
        <div className="mb-2 flex flex-col px-3">
          <NentSwitcher className="mb-4" />
          <div className="flex w-full max-w-full flex-wrap items-center justify-between gap-2">
            <h5>Tables</h5>
          </div>
        </div>
      )}
      {tableNames.length > 0 && (
        <div className="flex items-center gap-1 border-b px-3 py-1.5">
          <MagnifyingGlassIcon className="text-content-secondary" />
          <input
            id="Search tables"
            placeholder="Search tables..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            type="search"
            className={classNames(
              "placeholder:text-content-tertiary truncate relative w-full text-left text-xs text-content-primary disabled:bg-background-tertiary disabled:text-content-secondary disabled:cursor-not-allowed",
              "focus:outline-hidden bg-background-secondary font-normal",
            )}
          />
        </div>
      )}
      <div className="scrollbar flex-1 overflow-auto px-3 py-1">
        <div className="flex flex-col gap-0.5">
          {tableNames
            .filter(
              (r) =>
                !searchQueryLowercase ||
                r.toLowerCase().includes(searchQueryLowercase),
            )
            // Case insensitive sort
            .sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()))
            .map((table) => (
              <TableTab
                key={table}
                table={table}
                isMissingFromSchema={isTableMissingFromSchema(table, schema)}
                selectedTable={selectedTable}
                onSelectTable={onSelectTable}
              />
            ))}
        </div>
        <CreateNewTable tableData={tableData} onTableCreated={onTableCreated} />
      </div>
    </div>
  );
}

export function CreateNewTable({
  tableData,
  onTableCreated,
}: {
  tableData: TableMetadata;
  onTableCreated?: () => void;
}) {
  const { tableNames, selectTable } = tableData;

  const createTable = useMutation(udfs.createTable.default);
  const [newTableName, setNewTableName] = useState<string>();
  const validationError = validateConvexIdentifier(
    newTableName || "",
    "Table name",
  );
  const { selectedNent } = useNents();

  const { useIsOperationAllowed } = useContext(PermissionsContext);

  const canCreateTable = useIsOperationAllowed("WriteData");

  return newTableName !== undefined ? (
    <form
      className="mt-1 inline"
      onSubmit={async (e) => {
        e.preventDefault();
        if (!newTableName) {
          return;
        }

        if (tableNames.includes(newTableName)) {
          toast("error", `Table "${newTableName}" already exists.`);
        }
        try {
          await createTable({
            table: newTableName,
            componentId: selectedNent?.id ?? null,
          });
          selectTable(newTableName);
          onTableCreated?.();
        } finally {
          setNewTableName(undefined);
        }
      }}
    >
      <TextInput
        id="Create Table"
        className="mt-1"
        labelHidden
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            setNewTableName(undefined);
          }
        }}
        autoFocus
        placeholder="Untitled table"
        value={newTableName}
        onChange={(e) => setNewTableName(e.target.value)}
        error={
          tableNames.includes(newTableName)
            ? `Table '${newTableName}' already exists.`
            : newTableName
              ? validationError
              : undefined
        }
      />
      <div className="float-right flex flex-wrap gap-1">
        <Button
          size="xs"
          aria-label="Cancel table creation"
          className="mt-1 w-fit text-xs"
          variant="neutral"
          onClick={() => setNewTableName(undefined)}
        >
          Cancel
        </Button>
        <Button
          size="xs"
          disabled={
            !newTableName ||
            !!validationError ||
            tableNames.includes(newTableName)
          }
          type="submit"
          aria-label={`Create table with name "${newTableName}"`}
          className="mt-1 w-fit text-xs"
        >
          Create
        </Button>
      </div>
    </form>
  ) : (
    <Button
      size="sm"
      className="mt-1 max-w-full"
      onClick={() => setNewTableName("")}
      icon={<PlusIcon aria-hidden="true" />}
      inline
      disabled={
        !canCreateTable || !!(selectedNent && selectedNent.state !== "active")
      }
      tip={
        selectedNent && selectedNent.state !== "active"
          ? "Cannot create tables in an unmounted component."
          : !canCreateTable && (
              <PermissionDeniedTip
                message="You do not have permission to create tables in this deployment."
                action="deployment:data:write"
              />
            )
      }
    >
      <span className="truncate">Create Table</span>
    </Button>
  );
}

export function DataSideBarSkeleton() {
  return <div className="size-full bg-background-secondary" />;
}
