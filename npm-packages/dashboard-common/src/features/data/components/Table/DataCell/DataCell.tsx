import { GenericId, Value } from "convex/values";
import { GenericDocument } from "convex/server";
import classNames from "classnames";
import React, { memo, useLayoutEffect, useRef, useState } from "react";
import { useClickAway } from "react-use";
import { areEqual } from "react-window";
import {
  useFloating,
  autoUpdate,
  offset as offsetMiddleware,
  flip,
  shift,
} from "@floating-ui/react";
import { Column } from "@tanstack/react-table";
import { DotsVerticalIcon, Link2Icon } from "@radix-ui/react-icons";
import { Portal } from "@headlessui/react";
import { useTableDensity } from "@common/features/data/lib/useTableDensity";
import { CopiedPopper } from "@common/elements/CopiedPopper";

import { AuthorizeEditsConfirmationDialog } from "@common/elements/AuthorizeEditsConfirmationDialog";

import { KeyboardShortcut } from "@ui/KeyboardShortcut";
import { DataDetail } from "@common/features/data/components/Table/DataCell/DataDetail";
import {
  CELL_EDITOR_OVERHANG,
  CellEditor,
} from "@common/features/data/components/Table/DataCell/CellEditor";
import { DataCellValue } from "@common/features/data/components/Table/DataCell/DataCellValue";

import type { usePatchDocumentField } from "@common/features/data/components/Table/utils/usePatchDocumentField";
import { arrowKeyHandler } from "@common/features/data/components/Table/utils/arrowKeyHandler";
import {
  ActionHotkeysProps,
  OpenContextMenu,
  useActionHotkeys,
  useCellActions,
} from "@common/features/data/components/Table/DataCell/utils/cellActions";
import { usePasteListener } from "@common/features/data/components/Table/DataCell/utils/usePasteListener";
import { useTrackCellChanges } from "@common/features/data/components/Table/DataCell/utils/useTrackCellChanges";
import { useValidator } from "@common/features/data/components/Table/DataCell/utils/useValidator";
import { useContextMenuHandlers } from "@common/features/data/lib/useContextMenuTrigger";
import { SchemaJson } from "@common/lib/format";
import { stringifyValue } from "@common/lib/stringifyValue";
import { buttonClasses } from "@ui/Button";
import { Loading } from "@ui/Loading";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import { useNents } from "@common/lib/useNents";
import { getReferencedTableName } from "@common/lib/utils";
import { ReadonlyCode } from "@common/elements/ReadonlyCode";
import { Tooltip } from "@ui/Tooltip";
import { cn } from "@ui/cn";

export type DataCellProps = {
  value: Value;
  document: GenericDocument;
  column: Column<GenericDocument, unknown>;
  resizeHandler?: (event: unknown) => void;
  editDocument: () => void;
  areEditsAuthorized: boolean;
  authorizeEdits?: () => void;
  rowId: GenericId<string>;
  didRowChange: boolean;
  width?: string;
  inferIsDate: boolean;
  patchDocument: ReturnType<typeof usePatchDocumentField>;
  tableName: string;
  onOpenContextMenu: OpenContextMenu;
  onCloseContextMenu: () => void;
  canManageTable: boolean;
  activeSchema: SchemaJson | null;
  isContextMenuOpen: boolean;
};

export const DataCell = memo(DataCellImpl, areEqual);

function DataCellImpl({
  value,
  column,
  resizeHandler,
  authorizeEdits,
  areEditsAuthorized,
  width,
  rowId,
  document,
  didRowChange,
  inferIsDate = false,
  patchDocument,
  tableName,
  onOpenContextMenu,
  onCloseContextMenu,
  canManageTable,
  activeSchema,
  editDocument,
  isContextMenuOpen,
}: DataCellProps) {
  const cellRef = useRef<HTMLDivElement | null>(null);
  const cellButtonRef = useRef<HTMLButtonElement>(null);

  // Derive all the information needed to render the cell
  const columnName = column.id;
  const stringValue = typeof value === "string" ? value : stringifyValue(value);
  const [isHoveringCell, setIsHoveringCell] = useState(false);
  const [isFocused, setIsFocused] = useState(false);
  const isSystemField = columnName?.startsWith("_");
  const isEditable = !isSystemField && canManageTable;
  const isDateField = columnName === "_creationTime";

  // State for showing various modals and popovers
  const [showAuthorizeEditsModal, setShowAuthorizeEditsModal] = useState(false);
  const [showEditor, setShowEditor] = useState(false);
  const [pastedValue, setPastedValue] = useState<Value>();
  const [showDetail, setShowDetail] = useState(false);
  const [showDocumentDetail, setShowDocumentDetail] = useState(false);

  // Mega hook to generate all the actions
  // that can be performed on a cell
  // To keep the component code clean
  const {
    didJustCopy,
    idReferenceLink,
    copyValue,
    copyDocument,
    editValue,
    goToDoc,
    viewValue,
    viewDocument,
    contextMenuCallback,
  } = useCellActions({
    onOpenContextMenu,
    columnName,
    rowId,
    value,
    document,
    areEditsAuthorized,
    authorizeEdits,
    canManageTable,
    setPastedValue,
    setShowEditor,
    setShowAuthorizeEditsModal,
    setShowDetail,
    setShowDocumentDetail,
    editDocument,
  });

  const { shouldSurfaceValidatorErrors, allowTopLevelUndefined, validator } =
    useValidator(activeSchema, tableName, columnName);

  const didValueJustChange = useTrackCellChanges({
    value,
    didRowChange,
  });

  const { densityValues } = useTableDensity();

  // Right-click context menu. Wired as React handlers on the cell (rather than
  // a per-cell native listener) so it costs nothing extra per windowed cell and
  // works on the first right-click without needing the cell hovered first.
  const { onContextMenu, onMouseUp: onContextMenuMouseUp } =
    useContextMenuHandlers(contextMenuCallback, onCloseContextMenu);

  // Controls the copied value popper that shows up when a value is copied
  const [copiedPopperElement, setCopiedPopperElement] =
    useState<HTMLDivElement | null>(null);

  const closeEditor = () => {
    setShowEditor(false);
    cellButtonRef.current?.focus();
    setPastedValue(undefined);
  };

  return (
    <>
      {/* The keyboard shortcuts and paste handler only do anything while the
          cell holds focus, so mount them only then. This both keeps hundreds of
          windowed cells from each registering ~16 listeners (which pinned the
          CPU while scrolling, especially with many columns) and guarantees that
          only the single focused cell can respond to a keypress — otherwise
          arrowing to a new cell and pressing Enter could open two editors. */}
      {isFocused && (
        <CellKeyboardInteractions
          cellRef={cellRef}
          columnName={columnName}
          editValue={editValue}
          allowTopLevelUndefined={allowTopLevelUndefined}
          actions={{
            copyCb: copyValue,
            copyDocCb: copyDocument,
            viewCb: viewValue,
            viewDocCb: viewDocument,
            editCb: editValue,
            editDocCb: editDocument,
            goToDocCb: goToDoc,
            openContextMenu: () => {
              if (cellRef.current) {
                contextMenuCallback({
                  x: cellRef.current.getBoundingClientRect().right,
                  y: cellRef.current.getBoundingClientRect().top,
                });
              }
            },
          }}
        />
      )}
      {/* TODO: Can we get rid of this wrapping div? */}
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- right-click handler on the cell wrapper; the cell's interactive controls are the inner button(s) */}
      <div
        ref={(r) => {
          cellRef.current = r;
        }}
        className="relative flex size-full items-center hover:bg-background-tertiary/75"
        style={{ width }}
        onMouseEnter={() => setIsHoveringCell(true)}
        onMouseLeave={() => setIsHoveringCell(false)}
        onContextMenu={onContextMenu}
        onMouseUp={onContextMenuMouseUp}
        // Track focus on the wrapper (not just the main button) so the cell's
        // hotkeys/paste stay mounted while focus moves between its own controls
        // (e.g. the context-menu button). onFocus/onBlur bubble, and we only
        // clear isFocused when focus actually leaves the cell.
        onFocus={() => setIsFocused(true)}
        onBlur={(e) => {
          if (!cellRef.current?.contains(e.relatedTarget as Node | null)) {
            setIsFocused(false);
          }
        }}
      >
        {/* We do not use Button here because it's expensive and this table needs to be fast */}
        {/* eslint-disable-next-line react/forbid-elements */}
        <button
          data-testid="cell-editor-button"
          ref={cellButtonRef}
          className={classNames(
            // Show a border on the right side while animating to prevent the background highlight
            // from overlapping other cells
            didValueJustChange && "animate-highlight border-r",
            "font-mono text-xs text-content-primary",
            "w-full h-full flex items-center focus:outline-hidden",
            "focus:ring-1 focus:ring-border-selected text-left",
            isContextMenuOpen && "ring-1 ring-border-selected",
            !isEditable && "cursor-default",
          )}
          style={{
            padding: `${densityValues.paddingY}px ${densityValues.paddingX}px`,
          }}
          role={isEditable ? "button" : undefined}
          type="button"
          tabIndex={0}
          onKeyDown={arrowKeyHandler(cellRef)}
          onDoubleClick={clickHandler(isEditable, cellRef, editValue)}
        >
          {idReferenceLink !== undefined && columnName !== "_id" && (
            <Tooltip
              tip={<DocumentPreview id={value} />}
              contentClassName="bg-background-secondary animate-fadeInFromLoading"
              maxWidthClassName="max-w-[22rem]"
              delayDuration={250}
              asChild
            >
              <Link2Icon className="mr-2 flex-none text-content-secondary" />
            </Tooltip>
          )}
          <DataCellValue
            {...{
              isDateField,
              inferIsDate,
              value,
              isHovered: isHoveringCell,
              isReference: idReferenceLink !== undefined,
              detailHeader: columnName,
              stringValue,
            }}
          />
          {column.getCanResize() && resizeHandler && (
            // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- mouse/touch-driven column resize handle
            <div
              role="separator"
              onMouseDown={resizeHandler}
              onTouchStart={resizeHandler}
              className="absolute top-0 right-0 inline-block h-full cursor-col-resize touch-none select-none"
              style={{
                width: densityValues.paddingX,
              }}
            />
          )}
        </button>
        {(isHoveringCell || isFocused) && (
          // eslint-disable-next-line react/forbid-elements
          <button
            data-testid="cell-context-menu-button"
            type="button"
            onClick={() =>
              cellRef.current &&
              contextMenuCallback({
                x: cellRef.current.getBoundingClientRect().right,
                y: cellRef.current.getBoundingClientRect().top,
              })
            }
            className={cn(
              buttonClasses({ size: "xs", variant: "neutral" }),
              "absolute z-20 shadow-xs hover:bg-background-tertiary",
              isFocused && "focused",
              "animate-none",
            )}
            style={{
              right:
                (cellRef.current?.clientWidth || 0) < 64
                  ? -16
                  : densityValues.paddingX - 4,
            }}
          >
            {isHoveringCell && <DotsVerticalIcon />}
            {!isHoveringCell && (
              <KeyboardShortcut
                value={["CtrlOrCmd", "Return"]}
                className="text-xs text-content-secondary"
              />
            )}
          </button>
        )}
      </div>
      {/* Show a side panel to view the value of the current cell */}
      {showDetail && (
        <DataDetail
          value={value}
          header={
            <div className="flex items-center gap-1" data-testid="cell-detail">
              Viewing
              <span className="mr-2 font-mono">{columnName}</span>
              <span className="rounded-sm border p-1 font-mono text-xs">
                Document: {rowId}
              </span>
            </div>
          }
          onClose={() => setShowDetail(false)}
        />
      )}
      {/* Show a side panel to view the entire document */}
      {showDocumentDetail && (
        <DataDetail
          value={document}
          header={
            <div
              className="flex items-center gap-1"
              data-testid="cell-detail-document"
            >
              Viewing
              <span className="rounded-sm border p-1 font-mono text-xs">
                Document: {rowId}
              </span>
            </div>
          }
          onClose={() => setShowDocumentDetail(false)}
        />
      )}
      {/* Show the value editor popper right on top of the cell */}
      {showEditor && (
        <CellEditorPopper
          cellRef={cellRef}
          width={width}
          value={value}
          pastedValue={pastedValue}
          validator={validator}
          shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
          allowTopLevelUndefined={allowTopLevelUndefined}
          inferIsDate={inferIsDate}
          onClose={closeEditor}
          onSave={async (v) => {
            if (v !== undefined) {
              await patchDocument(tableName, rowId, columnName, v);
            }
          }}
        />
      )}
      {/* Show confirmation dialog in production */}
      {showAuthorizeEditsModal && (
        <AuthorizeEditsConfirmationDialog
          onClose={() => {
            setShowAuthorizeEditsModal(false);
          }}
          onConfirm={async () => {
            authorizeEdits?.();
            setShowAuthorizeEditsModal(false);
            setShowEditor(true);
          }}
        />
      )}
      {/* Show the popper when a value is copied */}
      <CopiedPopper
        referenceElement={cellRef.current}
        copiedPopperElement={copiedPopperElement}
        setCopiedPopperElement={setCopiedPopperElement}
        show={didJustCopy !== null}
        message={
          didJustCopy
            ? `Copied ${didJustCopy === "value" ? columnName : "document"}`
            : "Copied"
        }
        offset={[densityValues.paddingX - 4, 4]}
      />
    </>
  );
}

// Mounts the cell's keyboard shortcuts and paste handler. Rendered as a
// separate component, only while the cell is focused, so its hooks can be
// conditionally mounted without breaking the rules of hooks — and so only the
// single focused cell's listeners exist at a time.
function CellKeyboardInteractions({
  cellRef,
  columnName,
  editValue,
  allowTopLevelUndefined,
  actions,
}: {
  cellRef: React.MutableRefObject<HTMLDivElement | null>;
  columnName: string;
  editValue: (value?: Value) => void;
  allowTopLevelUndefined: boolean;
  actions: ActionHotkeysProps;
}) {
  const hotkeyRefs = useActionHotkeys(actions);
  // The cell element already exists (it's rendered by the parent), so scope the
  // hotkeys to it imperatively rather than via a render-time ref.
  useLayoutEffect(() => {
    if (cellRef.current) {
      hotkeyRefs(cellRef.current);
    }
  });

  usePasteListener(cellRef, columnName, editValue, allowTopLevelUndefined);

  return null;
}

// The cell value editor popper. Extracted into its own component so the popper
// positioning and click-away listeners only exist while the editor is open,
// instead of for every rendered cell.
function CellEditorPopper({
  cellRef,
  width,
  value,
  pastedValue,
  validator,
  shouldSurfaceValidatorErrors,
  allowTopLevelUndefined,
  inferIsDate,
  onClose,
  onSave,
}: {
  cellRef: React.MutableRefObject<HTMLDivElement | null>;
  width?: string;
  value: Value;
  pastedValue: Value | undefined;
  validator: ReturnType<typeof useValidator>["validator"];
  shouldSurfaceValidatorErrors: boolean | undefined;
  allowTopLevelUndefined: boolean;
  inferIsDate: boolean;
  onClose: () => void;
  onSave: (value?: Value) => Promise<void>;
}) {
  const { densityValues } = useTableDensity();
  const [editorPopper, setEditorPopper] = useState<HTMLDivElement | null>(null);
  const { floatingStyles: editorStyles } = useFloating({
    placement: "bottom-start",
    middleware: [
      offsetMiddleware({
        mainAxis: -(densityValues.height + CELL_EDITOR_OVERHANG),
        alignmentAxis: -CELL_EDITOR_OVERHANG,
      }),
      flip(),
      shift(),
    ],
    whileElementsMounted: autoUpdate,
    elements: { reference: cellRef.current, floating: editorPopper },
  });

  // When you click away from the cell, close the editor if it is open
  useClickAway({ current: editorPopper }, onClose);

  return (
    <Portal>
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
      <div
        ref={setEditorPopper}
        style={{
          ...editorStyles,
          width: width && `calc(${width} + ${2 * CELL_EDITOR_OVERHANG}px)`,
        }}
        className="z-50 min-w-[24rem] animate-fadeInFromLoading"
        data-testid="cell-editor-popper"
        tabIndex={-1}
        onBlur={(e) => {
          if (e.relatedTarget === null) {
            onClose();
          }
        }}
        // for safari
        onKeyDown={async (e) => {
          if (e.key === "Escape") {
            onClose();
          }
        }}
      >
        <CellEditor
          validator={validator}
          shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
          allowTopLevelUndefined={allowTopLevelUndefined}
          inferIsDate={inferIsDate}
          onStopEditing={onClose}
          defaultValue={pastedValue}
          value={value}
          onSave={onSave}
        />
      </div>
    </Portal>
  );
}

const clickHandler =
  (
    isEditable: boolean,
    cellRef: React.MutableRefObject<HTMLDivElement | null>,
    editValue: () => void,
  ) =>
  () => {
    if (isEditable) {
      editValue();
      return;
    }

    const selection = window.getSelection();
    selection?.selectAllChildren(cellRef.current!);
  };

function DocumentPreview({ id }: { id: string | Value }) {
  // Safely convert id to string if it's not already
  const stringId = typeof id === "string" ? id : String(id);

  const componentId = useNents().selectedNent?.id ?? null;
  const tableMapping = useQuery(udfs.getTableMapping.default, {
    componentId,
  });
  const tableName = getReferencedTableName(tableMapping, stringId);

  const docs = useQuery(udfs.listById.default, {
    componentId,
    ids: [{ id: stringId, tableName: tableName ?? "" }],
  });

  if (!docs) {
    return <Loading className="h-8 w-80" />;
  }

  if (!docs?.[0]) {
    return <div>Document not found.</div>;
  }

  return (
    <div className="w-80">
      <ReadonlyCode
        disableLineNumbers
        code={stringifyValue(docs[0] ?? null, true, true)}
        path={`documentPreview-${stringId}`}
        height={{ type: "content", maxHeightRem: 20 }}
      />
    </div>
  );
}
