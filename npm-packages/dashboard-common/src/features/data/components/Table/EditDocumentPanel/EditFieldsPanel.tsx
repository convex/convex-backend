import { GenericDocument } from "convex/server";
import isEqual from "lodash/isEqual";
import { useState } from "react";
import { ValidatorJSON } from "convex/values";
import { JavascriptDocumentsForm } from "@common/features/data/components/Table/EditDocumentPanel/JavascriptDocumentsForm";
import { DataPanel } from "@common/features/data/components/DataPanel";

export function EditFieldsPanel({
  tableName,
  allRowsSelected,
  numRowsSelected,
  onClose,
  onSave,
  validator,
  shouldSurfaceValidatorErrors,
}: {
  tableName: string;
  allRowsSelected: boolean;
  numRowsSelected: number;
  onClose: () => void;
  onSave(documents: GenericDocument): Promise<any>;
  validator?: ValidatorJSON;
  shouldSurfaceValidatorErrors?: boolean;
}) {
  const [fields, setFields] = useState(() => [{}]);
  const isDirty = !isEqual(fields, [{}]);

  const confirmClose = () => {
    if (isDirty) {
      const shouldClose = window.confirm(
        `You have unsaved changes.
Press "Cancel" to return to the editor, or "OK" to discard unsaved changes.`,
      );
      if (!shouldClose) {
        return false;
      }
    }
    return true;
  };

  return (
    <DataPanel
      data-testid="editFieldsPanel"
      fillHeight
      title={`Bulk edit ${documentsLabel(numRowsSelected, allRowsSelected)} in ${tableName}`}
      onClose={onClose}
      onBeforeClose={confirmClose}
    >
      <div className="mb-2 px-4 text-xs text-content-primary sm:px-6">
        <p>You can:</p>
        <p>• Add new fields</p>
        <p>• Overwrite existing fields</p>
        <p>
          • Remove existing fields by setting them to{" "}
          <code className="rounded-sm bg-background-tertiary p-[2px] text-content-primary">
            undefined
          </code>
        </p>
      </div>
      <JavascriptDocumentsForm
        documents={fields}
        setDocuments={setFields}
        onSave={async (array) => {
          await onSave(array[0]);
          onClose();
        }}
        mode="patchDocuments"
        isDirty={isDirty}
        validator={validator}
        shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
      />
    </DataPanel>
  );
}

function documentsLabel(numDocuments: number, allRowsSelected: boolean) {
  return allRowsSelected && numDocuments !== 1
    ? "all documents"
    : numDocuments > 1
      ? `${numDocuments} documents`
      : "document";
}
