import { GenericDocument } from "convex/server";
import { useCallback, useEffect, useRef, useState } from "react";

import { ValidatorJSON, Value } from "convex/values";
import isPlainObject from "lodash/isPlainObject";
import omitBy from "lodash/omitBy";
import { UNDEFINED_PLACEHOLDER } from "system-udfs/convex/_system/frontend/lib/values";
import { ObjectEditor } from "@common/elements/ObjectEditor/ObjectEditor";
import { Button } from "@ui/Button";

function isDocument(
  value: Value | undefined,
  allowMultipleDocuments: boolean,
): value is GenericDocument | GenericDocument[] {
  return (
    isPlainObject(value) ||
    (allowMultipleDocuments &&
      Array.isArray(value) &&
      value.length >= 1 &&
      value.every(isPlainObject))
  );
}

export function JavascriptDocumentsForm({
  documents,
  setDocuments,
  onSave,
  isDirty,
  validator,
  shouldSurfaceValidatorErrors = false,
  mode,
}: {
  validator?: ValidatorJSON;
  shouldSurfaceValidatorErrors?: boolean;
  documents: GenericDocument[];
  setDocuments(documents: GenericDocument[]): void;
  onSave(documents: GenericDocument[]): Promise<any>;
  isDirty: boolean;
  mode: "addDocuments" | "editDocument" | "patchDocuments";
}) {
  const [value, setValue] = useState<Value | undefined>(
    mode === "addDocuments" ? documents : documents[0],
  );
  const randomNumberRef = useRef<number>(Math.random());

  const onChange = useCallback(
    (newValue?: Value) => {
      const valueWithoutUndefined =
        mode === "patchDocuments" || Array.isArray(newValue)
          ? newValue
          : isPlainObject(newValue)
            ? (omitBy(
                newValue as object,
                (v) => v === UNDEFINED_PLACEHOLDER,
              ) as Value)
            : newValue;
      setValue(valueWithoutUndefined);
      if (isDocument(valueWithoutUndefined, mode === "addDocuments")) {
        setDocuments(
          Array.isArray(valueWithoutUndefined)
            ? valueWithoutUndefined
            : [valueWithoutUndefined],
        );
      }
    },
    [mode, setDocuments],
  );

  const [isInvalidObject, setIsInvalidObject] = useState(false);

  let validationError;
  if (isInvalidObject) {
    validationError = "Please fix the errors above to continue.";
  } else if (!isDocument(value, mode === "addDocuments")) {
    validationError =
      mode === "addDocuments"
        ? "Please enter a document or an array of documents to continue."
        : "Please enter a document to continue.";
  }
  const [isSaving, setIsSaving] = useState(false);
  const [submitErrorMessage, setSubmitErrorMessage] = useState<
    string | undefined
  >(undefined);
  const validationMessage = validationError ?? submitErrorMessage;

  useEffect(() => {
    setSubmitErrorMessage(undefined);
  }, [validationError, documents]);

  const disabled =
    validationError !== undefined ||
    isSaving ||
    ((mode === "editDocument" || mode === "patchDocuments") && !isDirty);

  const save = async () => {
    if (disabled) {
      return;
    }
    setSubmitErrorMessage(undefined);
    setIsSaving(true);
    try {
      await onSave(documents);
    } catch (e: any) {
      setSubmitErrorMessage(e.message);
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="flex grow flex-col overflow-y-hidden">
      <div className="mb-2 flex grow overflow-y-auto">
        <ObjectEditor
          fullHeight
          autoFocus
          saveAction={save}
          defaultValue={value}
          path={`document/${randomNumberRef.current}`}
          onChange={onChange}
          showLineNumbers
          className="rounded-none border-0 border-y py-2"
          showTableNames
          onError={(errors: string[]) => setIsInvalidObject(!!errors.length)}
          validator={validator}
          shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
          mode={mode}
        />
      </div>
      <div className="flex max-h-40 w-full grow bg-background-secondary px-4 py-2 sm:px-6">
        <div className="float-right flex w-full grow items-center justify-end gap-4 whitespace-pre-line">
          {validationMessage && (
            <p
              className="mt-1 scrollbar max-h-full overflow-y-auto text-xs wrap-break-word text-content-errorSecondary"
              role="alert"
            >
              {validationMessage}
            </p>
          )}
          <Button disabled={disabled} onClick={save} loading={isSaving}>
            {mode === "patchDocuments" ? "Apply" : "Save"}
          </Button>
        </div>
      </div>
    </div>
  );
}
