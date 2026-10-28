import { useMutation } from "convex/react";
import { Cursor, GenericDocument } from "convex/server";
import { ConvexError, ValidatorJSON } from "convex/values";
import { useContext, useState } from "react";
import udfs from "@common/udfs";
import { SchemaJson } from "system-udfs/convex/_system/frontend/lib/filters";
import { useNents } from "@common/lib/useNents";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { AuthorizeEditsConfirmationDialog } from "@common/elements/AuthorizeEditsConfirmationDialog";
import { ClearTableConfirmation } from "@common/features/data/components/DataToolbar/ClearTableConfirmation";
import { EditDocumentPanel } from "@common/features/data/components/Table/EditDocumentPanel/EditDocumentPanel";
import { EditFieldsPanel } from "@common/features/data/components/Table/EditDocumentPanel/EditFieldsPanel";
import { TableMetrics } from "@common/features/data/components/TableMetrics";
import { TableSchemaPanel } from "@common/features/data/components/TableSchemaPanel";
import { useDefaultDocument } from "@common/features/data/lib/useDefaultDocument";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { useRouter } from "next/router";
import { TableIndexesPanel } from "../components/TableIndexesPanel";

type PopupType =
  | { type: "addDocuments"; tableName: string }
  | { type: "editDocument"; document: Record<string, any>; tableName: string }
  | { type: "bulkEdit"; rowIds: Set<string> | "all"; tableName: string }
  | { type: "clearTable"; tableName: string }
  | { type: "deleteRows"; rowIds: Set<string> }
  | { type: "deleteTable"; tableName: string }
  | { type: "metrics"; tableName: string }
  | { type: "viewSchema"; tableName: string; highlightField?: string }
  | { type: "viewIndexes"; tableName: string };

export type PopupState = ReturnType<typeof useToolPopup>;

export function useToolPopup({
  addDocuments,
  patchFields,
  clearSelectedRows,
  clearTable,
  deleteRows,
  deleteTable,
  isProd,
  numRows,
  tableName,
  areEditsAuthorized,
  authorizeEdits,
  activeSchema,
}: {
  addDocuments: (table: string, documents: GenericDocument[]) => Promise<void>;
  patchFields: (
    table: string,
    rowIds: Set<string> | "all",
    fields: GenericDocument,
  ) => Promise<void>;
  clearSelectedRows: () => void;
  clearTable: (cursor: Cursor | null) => Promise<{
    continueCursor: Cursor;
    deleted: number;
    hasMore: boolean;
  }>;
  deleteRows: (rowIds: Set<string>) => Promise<void>;
  deleteTable: () => Promise<void>;
  isProd: boolean;
  numRows?: number;
  tableName: string;
  areEditsAuthorized: boolean;
  authorizeEdits: (() => void) | undefined;
  activeSchema: SchemaJson | null;
}) {
  // Popover and menu state.
  const [popup, setPopup] = useState<PopupType>();

  const router = useRouter();
  const closePopup = () => {
    if (router.query.showIndexes === "true") {
      const { showIndexes: _, ...restOfQuery } = router.query;
      router.query.showIndexes = "false";
      void router.push(
        {
          pathname: router.pathname,
          query: restOfQuery,
        },
        undefined,
        { shallow: true },
      );
    }
    setPopup(undefined);
  };

  const defaultDocument = useDefaultDocument(tableName);

  const validator = activeSchema?.tables.find(
    (t) => t.tableName === tableName,
  )?.documentType;
  const shouldSurfaceSchemaValidatorErrors = activeSchema?.schemaValidation;

  let popupEl: React.ReactElement | null = null;
  switch (popup?.type) {
    case "addDocuments":
      popupEl = (
        <EditDocumentPanel
          data-testid="editDocumentPanel"
          tableName={popup.tableName}
          onClose={closePopup}
          onSave={(documents) => addDocuments(popup.tableName, documents)}
          defaultDocument={defaultDocument}
          validator={validator ?? undefined}
          shouldSurfaceValidatorErrors={shouldSurfaceSchemaValidatorErrors}
        />
      );
      break;
    case "editDocument":
      popupEl = !areEditsAuthorized ? (
        <AuthorizeEditsConfirmationDialog
          onClose={closePopup}
          onConfirm={async () => {
            authorizeEdits!();
          }}
        />
      ) : (
        <EditSingleDocumentPanel
          tableName={popup.tableName}
          onClose={closePopup}
          editingDocument={popup.document}
          validator={validator ?? undefined}
          shouldSurfaceValidatorErrors={shouldSurfaceSchemaValidatorErrors}
        />
      );
      break;
    case "bulkEdit":
      popupEl = !areEditsAuthorized ? (
        <AuthorizeEditsConfirmationDialog
          onClose={closePopup}
          onConfirm={async () => {
            authorizeEdits!();
          }}
        />
      ) : (
        <EditFieldsPanel
          tableName={popup.tableName}
          allRowsSelected={popup.rowIds === "all"}
          numRowsSelected={popup.rowIds === "all" ? 0 : popup.rowIds.size}
          onClose={closePopup}
          onSave={(fields) =>
            patchFields(popup.tableName, popup.rowIds, fields)
          }
          validator={validator ?? undefined}
          shouldSurfaceValidatorErrors={shouldSurfaceSchemaValidatorErrors}
        />
      );
      break;
    case "clearTable":
      popupEl = (
        <ClearTableConfirmation
          clearTable={clearTable}
          numRows={numRows}
          closePopup={closePopup}
          clearSelectedRows={clearSelectedRows}
          tableName={tableName}
          isProd={isProd}
        />
      );
      break;
    case "deleteRows":
      popupEl = (
        <ConfirmationDialog
          onClose={closePopup}
          onConfirm={() => deleteRows(popup.rowIds)}
          confirmText="Delete"
          dialogTitle={`Delete ${popup.rowIds.size.toLocaleString()} selected document${
            popup.rowIds.size > 1 ? "s" : ""
          }`}
          dialogBody="Are you sure you want to permanently delete these documents?"
        />
      );
      break;
    case "deleteTable":
      popupEl = (
        <ConfirmationDialog
          onClose={closePopup}
          onConfirm={deleteTable}
          validationText={
            isProd ? `Delete production table ${tableName}` : undefined
          }
          confirmText="Delete"
          dialogTitle="Delete table"
          dialogBody={`Are you sure you want to permanently delete the table ${tableName}?`}
          variant="danger"
        />
      );
      break;
    case "viewSchema":
      popupEl = (
        <TableSchemaPanel
          onClose={closePopup}
          tableName={tableName}
          highlightField={popup.highlightField}
        />
      );
      break;
    case "viewIndexes":
      popupEl = (
        <TableIndexesPanel onClose={closePopup} tableName={tableName} />
      );
      break;
    case "metrics":
      popupEl = <TableMetrics onClose={closePopup} tableName={tableName} />;
      break;
    default:
      popup satisfies undefined;
      break;
  }

  return { popupEl, popup, setPopup } as const;
}

function EditSingleDocumentPanel({
  editingDocument,
  onClose,
  tableName,
  validator,
  shouldSurfaceValidatorErrors,
}: {
  editingDocument: Record<string, any>;
  onClose: () => void;
  tableName: string;
  validator?: ValidatorJSON;
  shouldSurfaceValidatorErrors?: boolean;
}) {
  const replaceDocument = useMutation(udfs.replaceDocument.default);
  const { selectedNent } = useNents();
  const { captureMessage } = useContext(DeploymentInfoContext);

  return (
    <EditDocumentPanel
      data-testid="editDocumentPanel"
      editing
      tableName={tableName}
      onClose={onClose}
      onSave={async (documents) => {
        if (documents.length !== 1) {
          captureMessage(
            `Unexpected documents array with ${documents.length} elements`,
            "error",
          );
        }
        const [document] = documents;

        try {
          await replaceDocument({
            id: editingDocument._id,
            document,
            componentId: selectedNent?.id ?? null,
          });
        } catch (error: any) {
          if (error instanceof ConvexError) {
            throw new Error(error.data);
          }
          throw error;
        }
      }}
      defaultDocument={Object.fromEntries(
        Object.entries(editingDocument).filter(([_, v]) => v !== undefined),
      )}
      validator={validator}
      shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
    />
  );
}
