import { useCallback, useMemo } from "react";
import { ValidatorJSON, Value } from "convex/values";
import { GenericDocument } from "convex/server";
import isEqual from "lodash/isEqual";
import omitBy from "lodash/omitBy";
import { Link } from "@ui/Link";
import { createGlobalState } from "react-use";
import { JavascriptDocumentsForm } from "@common/features/data/components/Table/EditDocumentPanel/JavascriptDocumentsForm";
import { useNents } from "@common/lib/useNents";
import { DataPanel } from "@common/features/data/components/DataPanel";

export type EditDocumentPanelProps = {
  onClose: () => void;
  onSave(documents: GenericDocument[]): Promise<any>;
  defaultDocument: { [key: string]: Value };
  tableName: string;
  editing?: boolean;
  validator?: ValidatorJSON;
  shouldSurfaceValidatorErrors?: boolean;
};

type DocumentDraftKey = string;

export const useDocumentDrafts = createGlobalState<
  Record<DocumentDraftKey, GenericDocument[] | undefined>
>({});

// Even though we clear out drafts when editing documents,
// we still track their state separately so that they don't clear out
// the draft state of documents you were adding.
const getDocumentDraftKey = (
  componentId: string | null,
  tableName: string,
  editingId?: string,
) => `${editingId || "add"}-${componentId}-${tableName}`;

export function EditDocumentPanel({
  onClose,
  onSave,
  defaultDocument,
  tableName,
  editing = false,
  validator,
  shouldSurfaceValidatorErrors,
}: EditDocumentPanelProps) {
  const [drafts, setDrafts] = useDocumentDrafts();
  const defaultDocumentWithoutSystemFields = useMemo(
    () =>
      omitBy(
        defaultDocument,
        (v, key) => typeof key === "string" && key.startsWith("_"),
      ),
    [defaultDocument],
  );

  const componentId = useNents().selectedNent?.id ?? null;

  // Drafts are still used to keep track of state while the editor is open,
  // But they are cleared in edit mode when the editor is closed.
  // They are always cleared if the save button is clicked.
  const documents = drafts[
    getDocumentDraftKey(
      componentId,
      tableName,
      editing ? (defaultDocument._id as string) : undefined,
    )
  ] ?? [defaultDocumentWithoutSystemFields];
  const setDocuments = useCallback(
    (newDocuments?: GenericDocument[]) => {
      setDrafts((d) => ({
        ...d,
        [getDocumentDraftKey(
          componentId,
          tableName,
          editing ? (defaultDocument._id as string) : undefined,
        )]: newDocuments,
      }));
    },
    [componentId, defaultDocument._id, editing, setDrafts, tableName],
  );

  const saveAndClearDraft = async () => {
    await onSave(documents);
    onClose();
    setDocuments(undefined);
  };

  const isDirty = !isEqual(documents, [defaultDocumentWithoutSystemFields]);

  const docsSection = editing ? "editing-a-document" : "creating-documents";

  // We only need to confirm closing the dialog if the user is editing a document. When the user adds documents, we store them as a draft.
  const confirmClose = () => {
    if (editing && isDirty) {
      const shouldClose = window.confirm(
        `You have unsaved changes.
  Press "Cancel" to return to the editor, or "OK" to discard unsaved changes.`,
      );
      if (!shouldClose) {
        return false;
      }
      // If the user is editing a document, clear the drafts. It can be a bit confusing to see a dirty state when opening the edit document dialog
      // again. Also, using the single-cell editor is more convenient than this editor anyway.
      setDocuments(undefined);
    }
    return true;
  };

  return (
    <DataPanel
      data-testid="editDocumentPanel"
      fillHeight
      title={
        editing ? (
          <div className="flex flex-wrap items-baseline gap-2">
            Edit{" "}
            <code className="text-xs" aria-label="Document ID">
              {defaultDocument._id as undefined | string}
            </code>
          </div>
        ) : (
          `Add new documents to ${tableName}`
        )
      }
      onClose={onClose}
      onBeforeClose={confirmClose}
    >
      <div className="mb-4 px-4 text-xs text-content-primary sm:px-6">
        <Link
          passHref
          href={`https://docs.convex.dev/dashboard/deployments/data#${docsSection}`}
          target="_blank"
        >
          Learn more
        </Link>{" "}
        about editing documents.
      </div>
      <JavascriptDocumentsForm
        validator={validator}
        shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
        documents={documents}
        setDocuments={setDocuments}
        onSave={saveAndClearDraft}
        isDirty={isDirty}
        mode={editing ? "editDocument" : "addDocuments"}
      />
    </DataPanel>
  );
}
