import {
  ClipboardCopyIcon,
  EnterFullScreenIcon,
  ExternalLinkIcon,
  FileIcon,
  FileTextIcon,
  MixerHorizontalIcon,
  Pencil1Icon,
  ResetIcon,
  StopwatchIcon,
  TrashIcon,
} from "@radix-ui/react-icons";
import React, { useCallback, useContext, useState } from "react";
import { GenericDocument } from "convex/server";
import { Value, convexToJson } from "convex/values";
import {
  Filter,
  FilterByBuiltin,
  FilterByType,
  typeOf,
} from "system-udfs/convex/_system/frontend/lib/filters";
import { UrlObject } from "url";
import { Key } from "@ui/KeyboardShortcut";
import type { Option } from "@ui/Combobox";
import { ContextMenu } from "@common/features/data/components/ContextMenu";
import { PopupState } from "@common/features/data/lib/useToolPopup";
import {
  ActionHotkeysProps,
  OpenContextMenu,
  TableContextMenuState,
  useActionHotkeys,
} from "@common/features/data/components/Table/DataCell/utils/cellActions";
import { stringifyValue } from "@common/lib/stringifyValue";
import { useNents } from "@common/lib/useNents";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";
import { SchemaJson } from "@common/lib/format";
import { documentValidatorForTable } from "@common/features/data/components/Table/utils/validators";

export function useTableContextMenuState(): {
  contextMenuState: TableContextMenuState | null;
  openContextMenu: OpenContextMenu;
  closeContextMenu: () => void;
} {
  const [contextMenuState, setContextMenuState] =
    useState<TableContextMenuState | null>(null);

  const openContextMenu: OpenContextMenu = useCallback(
    (newTarget, rowId, cell) => {
      setContextMenuState({
        target: newTarget,
        selectedCell: cell === null ? null : { rowId, ...cell },
      });
    },
    [],
  );

  const closeContextMenu = useCallback(() => {
    setContextMenuState(null);
  }, []);

  return {
    contextMenuState,
    openContextMenu,
    closeContextMenu,
  };
}

export type TableContextMenuProps = {
  data: GenericDocument[];
  state: TableContextMenuState | null;
  close: () => void;
  deleteRows: (rowIds: Set<string>) => Promise<void>;
  setPopup: PopupState["setPopup"];
  onAddDraftFilter: (newFilter: Filter) => void;
  defaultDocument: GenericDocument;
  resetColumns: () => void;
  canManageTable: boolean;
  isProtectedDeployment: boolean;
  activeSchema: SchemaJson | null;
  tableName: string;
};

export function TableContextMenu({
  data,
  state,
  close,
  deleteRows,
  setPopup,
  onAddDraftFilter,
  defaultDocument,
  resetColumns,
  canManageTable,
  isProtectedDeployment,
  activeSchema,
  tableName,
}: TableContextMenuProps) {
  const { selectedNent } = useNents();
  const isInUnmountedComponent = !!(
    selectedNent && selectedNent.state !== "active"
  );

  const { captureMessage } = useContext(DeploymentInfoContext);

  const disableEditDoc = !canManageTable || isInUnmountedComponent;
  const disableEdit =
    state?.selectedCell?.column.startsWith("_") || disableEditDoc;

  const createActionHandler =
    (
      action:
        | "edit"
        | "copy"
        | "view"
        | "editDoc"
        | "viewDoc"
        | "copyDoc"
        | "goToRef",
    ) =>
    () => {
      if (!state?.selectedCell?.callbacks?.[action]) return;

      if (action === "editDoc" && disableEditDoc) return;
      if (action === "edit" && disableEdit) return;

      if (action === "editDoc" || action === "viewDoc") {
        const selectedRowId = state.selectedCell?.rowId;
        const document = data.find((row) => row._id === selectedRowId);
        if (!document) {
          captureMessage(
            "Can’t find the right-clicked document in data",
            "error",
          );
          return;
        }
      }
      state.selectedCell.callbacks[action]();
      close();
    };
  const editCb = createActionHandler("edit");
  const editDocCb = createActionHandler("editDoc");
  const viewDocCb = createActionHandler("viewDoc");
  const copyDocCb = createActionHandler("copyDoc");
  const goToDocCb = createActionHandler("goToRef");
  const copyCb = createActionHandler("copy");
  const viewCb = createActionHandler("view");

  return (
    <ContextMenu target={state ? state.target : null} onClose={close}>
      {state && (
        <div data-testid="table-context-menu">
          {/* only load in the hotkeys while the context menu is open */}
          <ActionHotkeys
            copyCb={copyCb}
            copyDocCb={copyDocCb}
            viewCb={viewCb}
            viewDocCb={viewDocCb}
            editCb={editCb}
            editDocCb={editDocCb}
            goToDocCb={goToDocCb}
          />

          {/* actions you can take on a specific cell */}
          <CellActions
            state={state}
            copyCb={copyCb}
            viewCb={viewCb}
            editCb={editCb}
            disableEdit={disableEdit}
            canManageTable={canManageTable}
            isInUnmountedComponent={isInUnmountedComponent}
            onAddDraftFilter={onAddDraftFilter}
            defaultDocument={defaultDocument}
          />

          {/* actions you can take on a specific document */}
          <DocumentActions
            state={state}
            setPopup={setPopup}
            deleteRows={deleteRows}
            disableEditDoc={disableEditDoc}
            canManageTable={canManageTable}
            isInUnmountedComponent={isInUnmountedComponent}
            editDocCb={editDocCb}
            viewDocCb={viewDocCb}
            copyDocCb={copyDocCb}
            isProtectedDeployment={isProtectedDeployment}
          />

          {/* actions you can take on the header */}
          {state.selectedCell && !state.selectedCell.rowId && (
            <>
              <hr className="my-1" />
              <ColumnSchemaAction
                activeSchema={activeSchema}
                tableName={tableName}
                columnName={state.selectedCell.column}
                setPopup={setPopup}
                close={close}
              />
              <ContextMenu.Item
                icon={<ResetIcon aria-hidden="true" />}
                label="Reset column positions and widths"
                tipSide="right"
                action={resetColumns}
              />
            </>
          )}
        </div>
      )}
    </ContextMenu>
  );
}

const operatorOptions: Readonly<
  Option<(FilterByType | FilterByBuiltin)["op"]>[]
> = [
  { value: "eq", label: "equals" },
  { value: "neq", label: "not equal" },
  { value: "gt", label: ">" },
  { value: "lt", label: "<" },
  { value: "gte", label: ">=" },
  { value: "lte", label: "<=" },
  { value: "type", label: "is type" },
  { value: "notype", label: "is not type" },
];

function showFilter(
  operator: (typeof operatorOptions)[number]["value"],
  value: Value | undefined,
  column: string,
) {
  // For falsy types, only provide direct comparisons
  if (value === undefined || value === null) {
    return operator === "type" || operator === "notype";
  }

  // Remove order filters where it doesn’t make sense (objects, arrays, booleans)
  if (
    (column === "_id" ||
      ((typeof value === "object" || typeof value === "boolean") &&
        !(value instanceof ArrayBuffer))) &&
    ["gt", "gte", "lt", "lte"].includes(operator)
  ) {
    return false;
  }

  if (column === "_creationTime" && ["eq", "neq"].includes(operator)) {
    return false;
  }

  if (
    ["_id", "_creationTime"].includes(column) &&
    (operator === "type" || operator === "notype")
  ) {
    // Remove type operators from the _id column
    return false;
  }

  return true;
}

function ActionHotkeys({
  copyCb,
  copyDocCb,
  viewCb,
  viewDocCb,
  editCb,
  editDocCb,
  goToDocCb,
}: ActionHotkeysProps) {
  useActionHotkeys({
    copyCb,
    copyDocCb,
    viewCb,
    viewDocCb,
    editCb,
    editDocCb,
    goToDocCb,
  });
  return null;
}

function CellActions({
  state,
  copyCb,
  viewCb,
  editCb,
  disableEdit,
  canManageTable,
  isInUnmountedComponent,
  onAddDraftFilter,
  defaultDocument,
}: {
  state: TableContextMenuState;
  copyCb: () => void;
  viewCb: () => void;
  editCb: () => void;
  disableEdit: boolean;
  canManageTable: boolean;
  isInUnmountedComponent: boolean;
  onAddDraftFilter: (newFilter: Filter) => void;
  defaultDocument: GenericDocument;
}) {
  const filterAction = state.selectedCell?.rowId ? (
    <FilterWithSubmenu
      state={state}
      addDraftFilter={onAddDraftFilter}
      defaultDocument={defaultDocument}
    />
  ) : (
    state.selectedCell?.column !== "*select" && (
      <ContextMenu.Item
        key={state.selectedCell?.column}
        icon={<MixerHorizontalIcon aria-hidden="true" />}
        label={
          <div className="flex items-center gap-1">
            Filter by <code>{state.selectedCell?.column}</code>
          </div>
        }
        action={() => {
          const value =
            state.selectedCell && state.selectedCell.column in defaultDocument
              ? defaultDocument[state.selectedCell.column]
              : null;

          onAddDraftFilter({
            id: Math.random().toString(),
            field: state.selectedCell?.column,
            op: "eq",
            value: convexToJson(value),
            enabled: true,
          });
        }}
      />
    )
  );

  const isFileRef =
    state.selectedCell?.callbacks?.docRefLink?.pathname?.endsWith("/files");
  const isScheduledFunctionRef =
    state.selectedCell?.callbacks?.docRefLink?.pathname?.endsWith(
      "/schedules/functions",
    );

  const cellActions =
    state.selectedCell?.rowId && state.selectedCell.callbacks
      ? [
          state.selectedCell.callbacks.docRefLink !== undefined
            ? {
                action: state.selectedCell.callbacks
                  .docRefLink satisfies UrlObject,
                shortcut: ["CtrlOrCmd", "G"] satisfies Key[],
                icon: isFileRef ? (
                  <FileIcon aria-hidden="true" />
                ) : isScheduledFunctionRef ? (
                  <StopwatchIcon aria-hidden="true" />
                ) : (
                  <ExternalLinkIcon aria-hidden="true" />
                ),
                label: isFileRef
                  ? "Go to File"
                  : isScheduledFunctionRef
                    ? "Go to Scheduled Functions"
                    : "Go to Reference",
                disabled: false,
                tip: null,
                blankTarget: false,
              }
            : {
                action: viewCb,
                shortcut: ["Space"] satisfies Key[],
                icon: <EnterFullScreenIcon aria-hidden="true" />,
                label: (
                  <div className="flex items-center gap-1">
                    View <code>{state.selectedCell.column}</code>
                  </div>
                ),
                disabled: false,
                tip: null,
              },
          {
            action: copyCb,
            shortcut: ["CtrlOrCmd", "C"] satisfies Key[],
            icon: <ClipboardCopyIcon aria-hidden="true" />,
            label: (
              <div className="flex items-center gap-1">
                Copy <code>{state.selectedCell.column}</code>
              </div>
            ),
            disabled: false,
            tip: null,
          },
          {
            action: editCb,
            shortcut: ["Return"] satisfies Key[],
            icon: <Pencil1Icon aria-hidden="true" />,
            label: (
              <div className="flex items-center gap-1">
                Edit <code>{state.selectedCell.column}</code>
              </div>
            ),
            disabled: disableEdit,
            tip: isInUnmountedComponent ? (
              "Cannot edit documents in an unmounted component."
            ) : !canManageTable ? (
              <PermissionDeniedTip
                message="You do not have permission to edit data in this deployment."
                action="deployment:data:write"
              />
            ) : null,
          },
        ]
      : null;

  return (
    <>
      {cellActions?.map((action, idx) => (
        <ContextMenu.Item
          key={idx}
          icon={action.icon}
          label={action.label}
          action={action.action}
          shortcut={action.shortcut}
          disabled={action.disabled}
          tip={action.tip}
          tipSide="right"
          blankTarget={"blankTarget" in action ? action.blankTarget : undefined}
        />
      ))}
      {filterAction}
    </>
  );
}

function FilterWithSubmenu({
  state,
  addDraftFilter,
  defaultDocument,
}: {
  state: TableContextMenuState;
  addDraftFilter: (newFilter: Filter) => void;
  defaultDocument: GenericDocument;
}) {
  const { captureMessage } = useContext(DeploymentInfoContext);
  if (!state.selectedCell) {
    captureMessage("No selected cell in FilterWithSubmenu", "error");
    return null;
  }
  return (
    <ContextMenu.Submenu
      icon={<MixerHorizontalIcon aria-hidden="true" />}
      label={
        <div className="flex items-center gap-1">
          Filter by <code>{state.selectedCell?.column}</code>
        </div>
      }
      action={() => {
        const value =
          state.selectedCell && state.selectedCell.column in defaultDocument
            ? defaultDocument[state.selectedCell.column]
            : null;
        addDraftFilter({
          id: Math.random().toString(),
          field: state.selectedCell?.column,
          op: "eq",
          value: convexToJson(value),
        });
      }}
    >
      {operatorOptions.map(({ value: operator, label: operatorLabel }) => {
        const cell = state.selectedCell;

        if (!cell) return null;
        const selectedValue = cell.value;

        if (!showFilter(operator, selectedValue, cell.column)) {
          return null;
        }

        return operator === "type" || operator === "notype" ? (
          // Type operator
          <ContextMenu.Item
            key={operator}
            label={`${operatorLabel.replace(" type", "")} ${typeOf(selectedValue)}`}
            action={() => {
              addDraftFilter({
                id: Math.random().toString(),
                field: cell.column,
                op: operator,
                value: typeOf(selectedValue),
              });
            }}
          />
        ) : (
          // Value operator
          selectedValue !== null && selectedValue !== undefined && (
            <ContextMenu.Item
              key={operator}
              label={
                <>
                  {operatorLabel}{" "}
                  <code>
                    {cell.column === "_creationTime"
                      ? new Date(selectedValue as number).toLocaleString()
                      : stringifyValue(selectedValue)}
                  </code>
                </>
              }
              action={() => {
                addDraftFilter({
                  id: Math.random().toString(),
                  field: state.selectedCell?.column,
                  op: operator,
                  value:
                    selectedValue === undefined
                      ? undefined
                      : convexToJson(selectedValue),
                });
              }}
            />
          )
        );
      })}
    </ContextMenu.Submenu>
  );
}

function DocumentActions({
  state,
  setPopup,
  deleteRows,
  disableEditDoc,
  canManageTable,
  isInUnmountedComponent,
  editDocCb,
  viewDocCb,
  copyDocCb,
  isProtectedDeployment,
}: {
  state: TableContextMenuState;
  setPopup: PopupState["setPopup"];
  deleteRows: (rowIds: Set<string>) => Promise<void>;
  disableEditDoc: boolean;
  canManageTable: boolean;
  isInUnmountedComponent: boolean;
  editDocCb: () => void;
  viewDocCb: () => void;
  copyDocCb: () => void;
  isProtectedDeployment: boolean;
}) {
  if (!state?.selectedCell?.callbacks || !state?.selectedCell.callbacks)
    return null;
  const documentActions = [
    {
      icon: <EnterFullScreenIcon aria-hidden="true" />,
      label: "View Document",
      shortcut: ["Shift", "Space"] satisfies Key[],
      action: viewDocCb,
    },
    {
      icon: <ClipboardCopyIcon aria-hidden="true" />,
      label: "Copy Document",
      tipSide: "right",
      shortcut: ["Shift", "CtrlOrCmd", "C"] satisfies Key[],
      action: copyDocCb,
    },
    {
      icon: <Pencil1Icon aria-hidden="true" />,
      label: "Edit Document",
      shortcut: ["Shift", "Return"] satisfies Key[],
      disabled: disableEditDoc,
      tip: isInUnmountedComponent
        ? "Cannot edit documents in an unmounted component."
        : !canManageTable && (
            <PermissionDeniedTip
              message="You do not have permission to edit data in this deployment."
              action="deployment:data:write"
            />
          ),
      tipSide: "right",
      action: editDocCb,
    },
    {
      icon: <TrashIcon aria-hidden="true" />,
      label: "Delete Document",
      disabled: disableEditDoc,
      tip: isInUnmountedComponent
        ? "Cannot delete documents in an unmounted component."
        : !canManageTable && (
            <PermissionDeniedTip
              message="You do not have permission to delete data in this deployment."
              action="deployment:data:write"
            />
          ),
      tipSide: "right",
      danger: true,
      action: () => {
        if (!state.selectedCell?.rowId) return;
        if (isProtectedDeployment) {
          setPopup({
            type: "deleteRows",
            rowIds: new Set([state.selectedCell.rowId]),
          });
        } else {
          void deleteRows(new Set([state.selectedCell.rowId]));
        }
      },
    },
  ];

  return (
    <>
      <hr className="my-1" />
      {documentActions?.map((action, idx) => (
        <ContextMenu.Item
          key={idx}
          icon={action.icon}
          label={action.label}
          action={action.action}
          shortcut={action.shortcut || undefined}
          disabled={action.disabled}
          tip={action.tip}
          tipSide="right"
          variant={action.danger ? "danger" : "neutral"}
        />
      ))}
    </>
  );
}

// Header action to open a side panel with the schema for the clicked column.
// Only rendered when the column is actually defined in the table's schema.
function ColumnSchemaAction({
  activeSchema,
  tableName,
  columnName,
  setPopup,
  close,
}: {
  activeSchema: SchemaJson | null;
  tableName: string;
  columnName: string;
  setPopup: PopupState["setPopup"];
  close: () => void;
}) {
  const documentValidator =
    activeSchema && documentValidatorForTable(activeSchema, tableName);
  const hasFieldSchema =
    documentValidator?.type === "object" &&
    documentValidator.value[columnName] !== undefined;

  if (columnName === "*select" || !hasFieldSchema) {
    return null;
  }

  return (
    <ContextMenu.Item
      icon={<FileTextIcon aria-hidden="true" />}
      label={
        <div className="flex items-center gap-1">
          View schema for <code>{columnName}</code>
        </div>
      }
      tipSide="right"
      action={() => {
        setPopup({ type: "viewSchema", tableName, highlightField: columnName });
        close();
      }}
    />
  );
}
