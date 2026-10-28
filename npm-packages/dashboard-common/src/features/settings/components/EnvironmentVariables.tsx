import { Form, Formik, FormikTouched, getIn, useFormikContext } from "formik";

import {
  ClipboardCopyIcon,
  EyeNoneIcon,
  EyeOpenIcon,
  MinusCircledIcon,
  Pencil2Icon,
  PlusCircledIcon,
  ResetIcon,
} from "@radix-ui/react-icons";

import classNames from "classnames";
import React, {
  ClipboardEventHandler,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { z } from "zod";
import { Spinner } from "@ui/Spinner";
import { Callout } from "@ui/Callout";
import { Button } from "@ui/Button";
import { copyTextToClipboard, toast } from "@common/lib/utils";
import { TextInput } from "@ui/TextInput";
import { cn } from "@ui/cn";
import cloneDeep from "lodash/cloneDeep";
import { formatEnvValueForDotfile } from "./formatEnvValueForDotfile";

export const ENVIRONMENT_VARIABLES_ROW_CLASSES =
  "grid grid-cols-[minmax(0,1fr)_6.5rem] gap-x-4 gap-y-2 py-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_6.5rem]";
export const ENVIRONMENT_VARIABLE_NAME_COLUMN = "col-span-2 md:col-span-1";

const ERROR_ENV_VAR_NOT_UNIQUE = "Environment variable name is not unique";

export type BaseEnvironmentVariable = { name: string; value: string };

type FormState<T extends BaseEnvironmentVariable> = {
  editedVars: {
    oldEnvVar: T;
    newEnvVar: T;
  }[];
  newVars: T[];
  deletedVars: T[];
};

// This is used for showing both deployment environment variables and project level environment variables
export function EnvironmentVariables<T extends BaseEnvironmentVariable>({
  environmentVariables,
  updateEnvironmentVariables,
  initialFormValues,
  hasAdminPermissions,
  disabledTip,
  canCreate,
  canEdit,
  canDelete,
  disabledTipForCreate,
  disabledTipForEdit,
  disabledTipForDelete,
  onEnvironmentVariablesAdded,
  renderDisplayExtra,
  renderEditExtra,
  validateNameUniqueness = defaultValidateNameUniqueness,
  initEnvVar,
  envVarKey,
}: {
  environmentVariables: Array<T> | undefined;
  initialFormValues?: Array<T>;
  updateEnvironmentVariables: (
    creations: T[],
    modifications: { oldEnvVar: T; newEnvVar: T }[],
    deletions: T[],
  ) => Promise<void>;
  /** Default gate for create/edit/delete; per-action props below override
   *  it when the underlying actions need distinct grants (e.g. project
   *  defaults split into `:create` / `:update` / `:delete`). */
  hasAdminPermissions: boolean;
  /** Overrides the default disabled-button tips when the member can't
   *  manage env vars. Pass a `permissionDeniedTip(...)` result here to
   *  surface the missing role action; omit for the project-defaults
   *  caller (which falls back to a plain string). */
  disabledTip?: React.ReactNode;
  canCreate?: boolean;
  canEdit?: boolean;
  canDelete?: boolean;
  disabledTipForCreate?: React.ReactNode;
  disabledTipForEdit?: React.ReactNode;
  disabledTipForDelete?: React.ReactNode;
  onEnvironmentVariablesAdded?: (count: number) => void;
  /** Render content below the name when displaying (not editing) */
  renderDisplayExtra?: (props: { envVar: T }) => React.ReactNode;
  /** Render content below the form fields when editing */
  renderEditExtra?: (props: {
    formKey: string; // e.g. "newVars[0]"
    envVar: T;
  }) => React.ReactNode;
  /**
   * Override uniqueness validation
   *
   * By default, duplicate names are always errors.
   *
   * DefaultEnvironmentVariables overrides this to allow duplicates
   * with non-overlapping deployment types.
   */
  validateNameUniqueness?: (
    allVariables: Array<{ name: string; formKey: string; envVar: T }>,
    environmentVariables: Array<T> | undefined,
  ) => Record<string, string>;
  /** Creates a new env var object (useful for adding extra fields) */
  initEnvVar: (envVar: { name: string; value: string }) => T;
  /** Key used to uniquely identify a saved row */
  envVarKey?: (envVar: T) => string;
}) {
  const initialValues: FormState<T> = {
    editedVars: [],
    newVars: initialFormValues ?? [],
    deletedVars: [],
  };
  const initialTouched = {
    newVars: initialValues.newVars.map(({ name, value }) =>
      name !== "" || value !== ""
        ? ({ name: true, value: true } as FormikTouched<T>)
        : ({} as FormikTouched<T>),
    ),
  } as FormikTouched<FormState<T>>;

  return (
    <Formik
      enableReinitialize
      initialValues={initialValues}
      initialTouched={initialTouched}
      onSubmit={async (values, helpers) => {
        await updateEnvironmentVariables(
          values.newVars,
          values.editedVars,
          values.deletedVars,
        );
        const createdCount = values.newVars.length;
        if (createdCount > 0) {
          onEnvironmentVariablesAdded?.(createdCount);
        }
        helpers.resetForm({});
      }}
      validate={(values) => {
        const errors: Record<string, string> = {};

        // Collect all variables for uniqueness validation
        const uneditedVars =
          environmentVariables?.filter(
            (v) =>
              !values.editedVars.some((edited) => edited.oldEnvVar === v) &&
              !values.deletedVars.some((deleted) => deleted === v),
          ) ?? [];

        const allVariablesForUniqueness: Array<{
          name: string;
          formKey: string;
          envVar: T;
        }> = [
          ...uneditedVars.map((envVar, index) => ({
            name: envVar.name,
            formKey: `uneditedVars[${index}]`,
            envVar,
          })),
          ...values.editedVars.map((editedVar, index) => ({
            name: editedVar.newEnvVar.name,
            formKey: `editedVars[${index}].newEnvVar`,
            envVar: editedVar.newEnvVar,
          })),
          ...values.newVars.map((envVar, index) => ({
            name: envVar.name,
            formKey: `newVars[${index}]`,
            envVar,
          })),
        ];

        // Use pluggable uniqueness validation
        const uniquenessErrors = validateNameUniqueness(
          allVariablesForUniqueness,
          environmentVariables,
        );
        Object.assign(errors, uniquenessErrors);

        // Validate individual field values
        const variablesToValidate: {
          value: T;
          key: string;
        }[] = [
          ...values.editedVars.map((editedVar, index) => ({
            value: editedVar.newEnvVar,
            key: `editedVars[${index}].newEnvVar`,
          })),
          ...values.newVars.map((value, index) => ({
            value,
            key: `newVars[${index}]`,
          })),
        ];
        variablesToValidate.forEach(({ value, key }) => {
          try {
            EnvVarName.parse(value.name);
          } catch (err) {
            if (err instanceof z.ZodError) {
              errors[`${key}.name`] = err.issues[0].message;
            }
          }

          try {
            EnvVarValue.parse(value.value);
          } catch (err) {
            if (err instanceof z.ZodError) {
              errors[`${key}.value`] = err.issues[0].message;
            }
          }
        });

        return errors;
      }}
    >
      <EnvironmentVariablesForm
        environmentVariables={environmentVariables}
        canCreate={canCreate ?? hasAdminPermissions}
        canEdit={canEdit ?? hasAdminPermissions}
        canDelete={canDelete ?? hasAdminPermissions}
        disabledTipForCreate={disabledTipForCreate ?? disabledTip}
        disabledTipForEdit={disabledTipForEdit ?? disabledTip}
        disabledTipForDelete={disabledTipForDelete ?? disabledTip}
        renderDisplayExtra={renderDisplayExtra}
        renderEditExtra={renderEditExtra}
        initEnvVar={initEnvVar}
        envVarKey={envVarKey}
      />
    </Formik>
  );
}

function EnvironmentVariablesForm<T extends BaseEnvironmentVariable>({
  environmentVariables,
  canCreate,
  canEdit,
  canDelete,
  disabledTipForCreate,
  disabledTipForEdit,
  disabledTipForDelete,
  renderDisplayExtra,
  renderEditExtra,
  initEnvVar,
  envVarKey,
}: {
  environmentVariables: Array<T> | undefined;
  canCreate: boolean;
  canEdit: boolean;
  canDelete: boolean;
  disabledTipForCreate?: React.ReactNode;
  disabledTipForEdit?: React.ReactNode;
  disabledTipForDelete?: React.ReactNode;
  renderDisplayExtra?: (props: { envVar: T }) => React.ReactNode;
  renderEditExtra?: (props: { formKey: string; envVar: T }) => React.ReactNode;
  initEnvVar: (envVar: { name: string; value: string }) => T;
  envVarKey: ((envVar: T) => string) | undefined;
}) {
  const formState = useFormikContext<FormState<T>>();

  // Remove elements from editedVars/deletedVars that refer to variables that
  // don’t exist anymore. This can be caused by a realtime update
  // of `environmentVariables` (see CX-5439).
  const prevEnvironmentVariables = useRef<Array<T> | undefined>(undefined);
  useEffect(() => {
    if (
      !environmentVariables ||
      prevEnvironmentVariables.current === environmentVariables
    )
      return;
    prevEnvironmentVariables.current = environmentVariables;

    const oldEditedVars = formState.values.editedVars;
    const oldDeletedVars = formState.values.deletedVars;
    if (oldEditedVars.length === 0 && oldDeletedVars.length === 0) return;

    const existingEnvironmentVariables = new Set(
      environmentVariables.map(({ name }) => name),
    );

    const newEditedVars = oldEditedVars.filter((v) =>
      existingEnvironmentVariables.has(v.oldEnvVar.name),
    );
    if (newEditedVars.length !== oldEditedVars.length) {
      void formState.setFieldValue("editedVars", newEditedVars);
    }

    const newDeletedVars = oldDeletedVars.filter((v) =>
      existingEnvironmentVariables.has(v.name),
    );
    if (newDeletedVars.length !== oldDeletedVars.length) {
      void formState.setFieldValue("deletedVars", newDeletedVars);
    }
  }, [environmentVariables, formState]);

  return (
    <Form className="flex flex-col [--env-var-contents-height:2.125rem]">
      {environmentVariables === undefined ? (
        <Spinner />
      ) : (
        <>
          <div className="divide-y divide-border-transparent">
            {(environmentVariables.length > 0 ||
              formState.values.newVars.length > 0) && (
              <div
                className={classNames(
                  ENVIRONMENT_VARIABLES_ROW_CLASSES,
                  "hidden md:grid mb-0.5",
                )}
              >
                <div
                  className={`flex flex-col gap-1 ${ENVIRONMENT_VARIABLE_NAME_COLUMN}`}
                >
                  <span className="text-sm text-content-secondary">Name</span>
                </div>
                <div className="flex flex-col gap-1">
                  <span className="text-sm text-content-secondary">Value</span>
                </div>
              </div>
            )}
            {environmentVariables?.map((value) => (
              <EnvironmentVariableListItem
                key={envVarKey ? envVarKey(value) : value.name}
                environmentVariable={value}
                canEdit={canEdit}
                canDelete={canDelete}
                disabledTipForEdit={disabledTipForEdit}
                disabledTipForDelete={disabledTipForDelete}
                renderDisplayExtra={renderDisplayExtra}
                renderEditExtra={renderEditExtra}
              />
            ))}
          </div>

          <NewEnvVars
            existingEnvVars={environmentVariables}
            canCreate={canCreate}
            disabledTipForCreate={disabledTipForCreate}
            renderEditExtra={renderEditExtra}
            initEnvVar={initEnvVar}
          />
        </>
      )}
    </Form>
  );
}

// Default uniqueness validator - any duplicate name is an error
export function defaultValidateNameUniqueness<
  T extends BaseEnvironmentVariable,
>(
  allVariables: Array<{ name: string; formKey: string; envVar: T }>,
): Record<string, string> {
  const errors: Record<string, string> = {};
  const nameOccurrences = new Map<string, string[]>();

  allVariables.forEach(({ name, formKey }) => {
    const existing = nameOccurrences.get(name) || [];
    existing.push(formKey);
    nameOccurrences.set(name, existing);
  });

  nameOccurrences.forEach((formKeys) => {
    if (formKeys.length > 1) {
      formKeys.forEach((key) => {
        errors[`${key}.name`] = ERROR_ENV_VAR_NOT_UNIQUE;
      });
    }
  });

  return errors;
}

// Adapted from https://github.com/motdotla/dotenv/blob/cf4c56957974efb7238ecaba6f16e0afa895c194/lib/main.js#L12
const LINE =
  /(?:^|^)\s*(?:export\s+)?([\w.-]+)(?:\s*=\s*?|:\s+?)(\s*'(?:\\'|[^'])*'|\s*"(?:\\"|[^"])*"|\s*`(?:\\`|[^`])*`|[^#\r\n]+)?\s*(?:#.*)?(?:$|$)/gm;
function _parseEnvVars(input: string): Record<string, string> {
  const obj: Record<string, string> = {};

  // Convert line breaks to same format
  const lines = input.replace(/\r\n?/gm, "\n");

  let match = LINE.exec(lines);
  while (match !== null) {
    const key = match[1];

    // Default undefined or null to empty string
    let value = match[2] || "";

    // Remove whitespace
    value = value.trim();

    // Check if double quoted
    const maybeQuote = value[0];

    // Remove surrounding quotes
    value = value.replace(/^(['"`])([\s\S]*)\1$/gm, "$2");

    // Expand newlines if double quoted
    if (maybeQuote === '"') {
      value = value.replace(/\\n/g, "\n");
      value = value.replace(/\\r/g, "\r");
    }

    // Add to object
    obj[key] = value;
    match = LINE.exec(lines);
  }

  return obj;
}

export function parseEnvVars(
  input: string,
): Array<BaseEnvironmentVariable> | null {
  const parsedEnvVars = _parseEnvVars(input);
  // Ignore if the pasted string does not match an environment variable string
  if (Object.keys(parsedEnvVars).length === 0) {
    return null;
  }
  return Object.entries(parsedEnvVars).map(([name, value]) => ({
    name,
    value,
  }));
}

function DisplayEnvVar<T extends BaseEnvironmentVariable>({
  environmentVariable,
  onEdit,
  onDelete,
  canEdit,
  canDelete,
  disabledTipForEdit,
  disabledTipForDelete,
  renderDisplayExtra,
}: {
  environmentVariable: T;
  onEdit: () => void;
  onDelete: () => void;
  canEdit: boolean;
  canDelete: boolean;
  disabledTipForEdit?: React.ReactNode;
  disabledTipForDelete?: React.ReactNode;
  renderDisplayExtra?: (props: { envVar: T }) => React.ReactNode;
}) {
  const formState = useFormikContext<FormState<T>>();
  const [showValue, setShowValue] = useState(false);

  return (
    <div className={ENVIRONMENT_VARIABLES_ROW_CLASSES}>
      <div className={ENVIRONMENT_VARIABLE_NAME_COLUMN}>
        <EnvironmentVariableName
          environmentVariable={environmentVariable}
          renderDisplayExtra={renderDisplayExtra}
        />
      </div>
      <div className="flex min-h-(--env-var-contents-height) min-w-0 items-center gap-1 font-mono">
        <Button
          tip={showValue ? "Hide" : "Show"}
          aria-label={showValue ? "Hide" : "Show"}
          type="button"
          onClick={() => setShowValue(!showValue)}
          variant="neutral"
          inline
          size="sm"
          className="h-full"
          icon={showValue ? <EyeNoneIcon /> : <EyeOpenIcon />}
        />
        <div className="min-w-0 flex-1">
          {showValue ? (
            <pre className="animate-fadeInFromLoading cursor-not-allowed rounded-md border bg-background-primary p-1.5 px-2 break-all whitespace-pre-wrap text-content-primary">
              {environmentVariable.value}
            </pre>
          ) : (
            <span
              title="Hidden environment variable"
              className="block truncate text-xs text-content-primary"
            >
              •••••••••••••••
            </span>
          )}
        </div>
      </div>
      <div className="flex h-(--env-var-contents-height) justify-between gap-2">
        <Button
          tip={
            !canEdit
              ? (disabledTipForEdit ??
                "You do not have permission to edit environment variables.")
              : "Edit"
          }
          aria-label="Edit"
          type="button"
          onClick={() => onEdit()}
          variant="neutral"
          icon={<Pencil2Icon />}
          disabled={formState.isSubmitting || !canEdit}
        />
        <Button
          tip="Copy Name and Value"
          aria-label="Copy Name and Value"
          type="button"
          onClick={async () => {
            const { formatted, warning } = formatEnvValueForDotfile(
              environmentVariable.value,
            );
            await copyTextToClipboard(
              `${environmentVariable.name}=${formatted}`,
            );
            if (warning) {
              toast(
                "warning",
                <div className="space-y-1">
                  <div>
                    Environment variable copied to the clipboard with the
                    following warning:
                  </div>
                  <div>
                    <code>{environmentVariable.name}</code>: {warning}
                  </div>
                </div>,
              );
            } else {
              toast(
                "success",
                "Environment variable name and value copied to the clipboard.",
              );
            }
          }}
          variant="neutral"
          icon={<ClipboardCopyIcon />}
          disabled={formState.isSubmitting}
        />
        <Button
          tip={
            !canDelete
              ? (disabledTipForDelete ??
                "You do not have permission to delete environment variables.")
              : "Delete"
          }
          aria-label="Delete"
          type="button"
          onClick={() => onDelete()}
          variant="danger"
          icon={<MinusCircledIcon />}
          disabled={formState.isSubmitting || !canDelete}
        />
      </div>
    </div>
  );
}

function DeletedEnvVar<T extends BaseEnvironmentVariable>({
  environmentVariable,
  onCancelDelete,
  renderDisplayExtra,
}: {
  environmentVariable: T;
  onCancelDelete: () => void;
  renderDisplayExtra?: (props: { envVar: T }) => React.ReactNode;
}) {
  const formState = useFormikContext<FormState<T>>();

  return (
    <div className={ENVIRONMENT_VARIABLES_ROW_CLASSES}>
      <div
        className={`flex flex-col gap-1 ${ENVIRONMENT_VARIABLE_NAME_COLUMN}`}
      >
        <EnvironmentVariableName
          environmentVariable={environmentVariable}
          renderDisplayExtra={renderDisplayExtra}
        />
      </div>
      <div className="flex h-(--env-var-contents-height) items-center justify-center gap-1 rounded-md border bg-background-error text-content-error">
        <MinusCircledIcon /> Will be deleted
      </div>
      <Button
        variant="neutral"
        className="min-h-(--env-var-contents-height) w-full justify-center"
        size="sm"
        onClick={() => onCancelDelete()}
        disabled={formState.isSubmitting}
        icon={<ResetIcon />}
      >
        Restore
      </Button>
    </div>
  );
}

function EnvironmentVariableName<T extends BaseEnvironmentVariable>({
  environmentVariable,
  renderDisplayExtra,
}: {
  environmentVariable: T;
  renderDisplayExtra?: (props: { envVar: T }) => React.ReactNode;
}) {
  return (
    <div className="flex min-h-(--env-var-contents-height) flex-col justify-center md:col-span-1">
      <div className="font-mono font-semibold break-all whitespace-pre-wrap text-content-primary">
        {environmentVariable.name}
      </div>
      {renderDisplayExtra?.({ envVar: environmentVariable })}
    </div>
  );
}

const EnvVarName = z
  .string()
  .min(1, "Environment variable name is required.")
  .max(256, "Environment variable name cannot exceed 256 characters.")
  .refine(
    (n) => /^[a-zA-Z_]+[a-zA-Z0-9_]*$/.test(n),
    "Name must start with a letter and only contain letters, numbers, and underscores.",
  );

const EnvVarValue = z
  .string()
  .max(8192, "Environment variable value cannot be larger than 8KB");

function EditEnvVarForm<T extends BaseEnvironmentVariable>({
  editIndex,
  onCancelEdit,
  renderEditExtra,
}: {
  editIndex: number;
  onCancelEdit: () => void;
  renderEditExtra?: (props: { formKey: string; envVar: T }) => React.ReactNode;
}) {
  const nameId = useId();
  const valueId = useId();

  const formState = useFormikContext<FormState<T>>();
  const envVar = formState.values.editedVars[editIndex].newEnvVar;
  const { value } = envVar;
  const formKey = `editedVars[${editIndex}].newEnvVar`;

  return (
    <div className="animate-fadeInFromLoading">
      <div className={ENVIRONMENT_VARIABLES_ROW_CLASSES}>
        <label
          htmlFor={nameId}
          className={`flex flex-col gap-1 ${ENVIRONMENT_VARIABLE_NAME_COLUMN}`}
        >
          <EnvVarNameInput formKey={`${formKey}.name`} id={nameId} />
        </label>
        <label htmlFor={valueId} className="flex grow flex-col flex-wrap gap-1">
          <EnvVarValueInput formKey={`${formKey}.value`} id={valueId} />
        </label>
        <Button
          type="button"
          variant="neutral"
          className="h-fit min-h-(--env-var-contents-height) w-full justify-center"
          size="sm"
          onClick={() => onCancelEdit()}
          disabled={formState.isSubmitting}
          icon={<ResetIcon />}
        >
          Undo Edit
        </Button>
        {renderEditExtra && (
          <div className="col-span-full flex flex-col gap-1 py-1">
            {renderEditExtra?.({ formKey, envVar })}
          </div>
        )}
      </div>
      {value.length > 1 && value.startsWith('"') && value.endsWith('"') && (
        <Callout className="mb-2 w-full">
          Environment variables usually shouldn't be surrounded by quotes.
          Quotes are useful in shell syntax and .env files but shouldn't be
          included in the environment variable value.
        </Callout>
      )}
    </div>
  );
}

function EnvironmentVariableListItem<
  T extends { name: string; value: string },
>({
  environmentVariable,
  canEdit,
  canDelete,
  disabledTipForEdit,
  disabledTipForDelete,
  renderDisplayExtra,
  renderEditExtra,
}: {
  environmentVariable: T;
  canEdit: boolean;
  canDelete: boolean;
  disabledTipForEdit?: React.ReactNode;
  disabledTipForDelete?: React.ReactNode;
  renderDisplayExtra?: (props: { envVar: T }) => React.ReactNode;
  renderEditExtra?: (props: { formKey: string; envVar: T }) => React.ReactNode;
}) {
  const formState = useFormikContext<FormState<T>>();

  const editIndex = formState.values.editedVars.findIndex(
    (edit) => edit.oldEnvVar === environmentVariable,
  );
  const isEditing = editIndex !== -1;
  if (isEditing) {
    return (
      <EditEnvVarForm
        editIndex={editIndex}
        onCancelEdit={() => {
          const newEditedVars = [
            ...formState.values.editedVars.slice(0, editIndex),
            ...formState.values.editedVars.slice(editIndex + 1),
          ];
          void formState.setFieldValue("editedVars", newEditedVars);

          // https://github.com/jaredpalmer/formik/issues/2059#issuecomment-612733378
          setTimeout(() =>
            formState.setTouched({
              editedVars: newEditedVars.map(() => ({
                newEnvVar: { name: true, value: true },
              })),
            } as any),
          );
        }}
        renderEditExtra={renderEditExtra}
      />
    );
  }

  const deleteIndex = formState.values.deletedVars.findIndex(
    (deletedVar) => deletedVar === environmentVariable,
  );
  const isDeleting = deleteIndex !== -1;
  if (isDeleting) {
    return (
      <DeletedEnvVar
        environmentVariable={environmentVariable}
        onCancelDelete={() => {
          const newDeletedVars = [
            ...formState.values.deletedVars.slice(0, deleteIndex),
            ...formState.values.deletedVars.slice(deleteIndex + 1),
          ];
          void formState.setFieldValue("deletedVars", newDeletedVars);
        }}
        renderDisplayExtra={renderDisplayExtra}
      />
    );
  }

  return (
    <DisplayEnvVar
      canEdit={canEdit}
      canDelete={canDelete}
      disabledTipForEdit={disabledTipForEdit}
      disabledTipForDelete={disabledTipForDelete}
      environmentVariable={environmentVariable}
      onEdit={() => {
        void formState.setFieldValue("editedVars", [
          ...formState.values.editedVars,
          {
            oldEnvVar: environmentVariable,
            newEnvVar: cloneDeep(environmentVariable),
          },
        ]);
      }}
      onDelete={() => {
        void formState.setFieldValue("deletedVars", [
          ...formState.values.deletedVars,
          environmentVariable,
        ]);
      }}
      renderDisplayExtra={renderDisplayExtra}
    />
  );
}

function NewEnvVars<T extends BaseEnvironmentVariable>({
  existingEnvVars,
  canCreate,
  disabledTipForCreate,
  renderEditExtra,
  initEnvVar,
}: {
  existingEnvVars: Array<T>;
  canCreate: boolean;
  disabledTipForCreate?: React.ReactNode;
  renderEditExtra?: (props: { formKey: string; envVar: T }) => React.ReactNode;
  initEnvVar: (envVar: { name: string; value: string }) => T;
}) {
  const formState = useFormikContext<FormState<T>>();

  const handlePaste = (envVars: Array<BaseEnvironmentVariable>) => {
    const newVars = formState.values.newVars.filter(
      ({ name, value }) => name !== "" || value !== "",
    );

    const transformedEnvVars = envVars.map(({ name, value }) =>
      initEnvVar({ name, value }),
    );

    void formState.setFieldValue("newVars", transformedEnvVars, true);

    // https://github.com/jaredpalmer/formik/issues/2059#issuecomment-612733378
    setTimeout(() =>
      formState.setTouched({
        newVars: newVars.map(() => ({ name: true, value: true })),
      } as any),
    );
  };

  return (
    <div>
      {formState.values.newVars.length > 0 && (
        <>
          <div className="divide-y divide-border-transparent border-t">
            {formState.values.newVars.map((envVar, index) => (
              <NewEnvVar
                key={index}
                newVarIndex={index}
                envVar={envVar}
                onDelete={() => {
                  const newVars = [
                    ...formState.values.newVars.slice(0, index),
                    ...formState.values.newVars.slice(index + 1),
                  ];
                  void formState.setFieldValue("newVars", newVars);

                  // https://github.com/jaredpalmer/formik/issues/2059#issuecomment-612733378
                  setTimeout(() => {
                    void formState.setTouched({
                      newVars: newVars.map(() => ({ name: true, value: true })),
                    } as any);
                  });
                }}
                onPasteVariables={(input) => handlePaste(input)}
                isLastVariable={index === formState.values.newVars.length - 1}
                renderEditExtra={renderEditExtra}
              />
            ))}
          </div>

          <p className="pt-2 pb-4 text-content-secondary">
            Tip: Paste your .env file directly into here!
          </p>
        </>
      )}

      <div className="my-2 flex place-content-between">
        <div className="flex gap-2">
          <Button
            type="button"
            variant="neutral"
            onClick={() => {
              void formState.setFieldValue("newVars", [
                ...formState.values.newVars,
                initEnvVar({ name: "", value: "" }),
              ]);
            }}
            icon={<PlusCircledIcon />}
            disabled={!canCreate}
            tip={
              !canCreate
                ? (disabledTipForCreate ??
                  "You do not have permission to add new environment variables.")
                : undefined
            }
          >
            Add
          </Button>

          {existingEnvVars.length > 0 && (
            <Button
              type="button"
              variant="neutral"
              onClick={async () => {
                const formattedEnvVars = existingEnvVars.map(
                  ({ name, value }) => {
                    const { formatted, warning } =
                      formatEnvValueForDotfile(value);
                    return { name, formatted, warning };
                  },
                );
                await copyTextToClipboard(
                  formattedEnvVars
                    .map(({ name, formatted }) => `${name}=${formatted}`)
                    .join("\n"),
                );

                const warnings = formattedEnvVars.flatMap(
                  ({ name, warning }) => (warning ? [{ name, warning }] : []),
                );
                if (warnings.length > 0) {
                  toast(
                    "warning",
                    <div className="space-y-1">
                      <div>
                        Environment variables copied to the clipboard with the
                        following warnings:
                      </div>
                      <ul className="list-disc pl-4">
                        {warnings.map(({ name, warning }, index) => (
                          <li key={index}>
                            <code>{name}</code>: {warning}
                          </li>
                        ))}
                      </ul>
                    </div>,
                  );
                } else {
                  toast(
                    "success",
                    "Environment variables copied to the clipboard.",
                  );
                }
              }}
              icon={<ClipboardCopyIcon />}
            >
              Copy All
            </Button>
          )}
        </div>

        {(formState.values.editedVars.length > 0 ||
          formState.values.newVars.length > 0 ||
          formState.values.deletedVars.length > 0) && (
          <Button
            type="submit"
            disabled={formState.isSubmitting || !formState.isValid}
          >
            {formState.values.editedVars.length +
              formState.values.newVars.length +
              formState.values.deletedVars.length >=
            2
              ? "Save All"
              : "Save"}
          </Button>
        )}
      </div>
    </div>
  );
}

function NewEnvVar<T extends BaseEnvironmentVariable>({
  newVarIndex,
  envVar,
  onDelete,
  onPasteVariables,
  isLastVariable,
  renderEditExtra,
}: {
  newVarIndex: number;
  envVar: T;
  onDelete: () => void;
  onPasteVariables: (variables: Array<BaseEnvironmentVariable>) => void;
  isLastVariable: boolean;
  renderEditExtra?: (props: { formKey: string; envVar: T }) => React.ReactNode;
}) {
  const nameId = useId();
  const valueId = useId();
  const { value } = envVar;
  const formKey = `newVars[${newVarIndex}]`;

  return (
    <div className={ENVIRONMENT_VARIABLES_ROW_CLASSES}>
      <label
        htmlFor={nameId}
        className={`flex flex-col gap-1 ${ENVIRONMENT_VARIABLE_NAME_COLUMN}`}
      >
        <div>
          <EnvVarNameInput
            formKey={`${formKey}.name`}
            id={nameId}
            onPaste={(e) => {
              const variables = parseEnvVars(e.clipboardData.getData("text"));
              if (variables) {
                e.preventDefault();
                onPasteVariables(variables);
              }
            }}
            autoFocus={isLastVariable}
          />
        </div>
      </label>

      <label htmlFor={valueId} className="flex grow flex-col gap-1">
        <div>
          <EnvVarValueInput formKey={`${formKey}.value`} id={valueId} />
          {value.length > 1 && value.startsWith('"') && value.endsWith('"') && (
            <Callout>
              Environment variables usually shouldn't be surrounded by quotes.
              Quotes are useful in shell syntax and .env files but shouldn't be
              included in the environment variable value.
            </Callout>
          )}
        </div>
      </label>

      <Button
        tip="Remove"
        aria-label="Remove"
        type="button"
        onClick={() => {
          onDelete();
        }}
        className="min-h-(--env-var-contents-height) w-fit self-start"
        variant="neutral"
        size="sm"
        icon={<MinusCircledIcon />}
      />

      {renderEditExtra && (
        <div className="col-span-full flex flex-col gap-1 py-1 md:col-start-1 md:row-start-2">
          {renderEditExtra?.({ formKey, envVar })}
        </div>
      )}
    </div>
  );
}

function EnvVarNameInput({
  formKey,
  id,
  onPaste,
  autoFocus = false,
}: {
  formKey: string;
  id: string;
  onPaste?: ClipboardEventHandler;
  autoFocus?: boolean;
}) {
  const formState = useFormikContext();
  const error = (formState.errors as Record<string, string>)[formKey];
  const touched = getIn(formState.touched, formKey);

  return (
    <TextInput
      id={id}
      className="font-mono"
      label="Name"
      labelHidden
      disabled={formState.isSubmitting}
      {...formState.getFieldProps(formKey)}
      onPaste={onPaste}
      error={
        (touched || error === ERROR_ENV_VAR_NOT_UNIQUE) && error
          ? error
          : undefined
      }
      autoFocus={autoFocus}
    />
  );
}

function EnvVarValueInput({
  formKey,
  id,
  autoFocus = false,
}: {
  formKey: string;
  id: string;
  autoFocus?: boolean;
}) {
  const formState = useFormikContext();
  const error = (formState.errors as Record<string, string>)[formKey];
  const value = getIn(formState.values, formKey) as string;
  const touched = Boolean(getIn(formState.touched, formKey));

  const hasAnyWhitespace = /\s/.test(value);
  const hasLeadingOrTrailingWhitespace =
    value.length > 0 && value !== value.trim();
  const hasReturnCharacter = value.includes("\n");
  const emptyStringWarning =
    touched && value === "" ? "This value is an empty string." : "";

  // Build whitespace warning message
  let whitespaceWarning = "";
  if (hasLeadingOrTrailingWhitespace || hasReturnCharacter) {
    const hasLeading = value !== value.trimStart();
    const hasTrailing = value !== value.trimEnd();
    if (hasLeading && hasTrailing) {
      whitespaceWarning = "This value has leading and trailing whitespace.";
    } else if (hasLeading) {
      whitespaceWarning = "This value has leading whitespace.";
    } else if (hasTrailing) {
      whitespaceWarning = "This value has trailing whitespace.";
    } else {
      whitespaceWarning = "This value contains return characters.";
    }
  }

  // Build whitespace indicator overlay parts - one element per character
  const whitespaceOverlayParts: { text: string; className: string }[] = [];
  if (hasAnyWhitespace) {
    const trimmedStart = value.trimStart();
    const trimmedEnd = value.trimEnd();
    const leadingCount = value.length - trimmedStart.length;
    const trailingCount = value.length - trimmedEnd.length;

    for (let i = 0; i < value.length; i++) {
      const char = value[i];
      const isSpace = char === " ";
      const isLineReturn = char === "\n";
      const isLeading = i < leadingCount;
      const isTrailing = i >= value.length - trailingCount;
      const colorClass =
        (isSpace && (isLeading || isTrailing)) || isLineReturn
          ? "text-content-warning bg-background-warning/60"
          : isSpace
            ? "text-content-tertiary/50"
            : "text-transparent";
      const displayChar = isSpace ? "␣" : isLineReturn ? "↵\n" : char;

      whitespaceOverlayParts.push({ text: displayChar, className: colorClass });
    }
  }

  const { hint, hintStyle } =
    touched && error
      ? {
          hint: error,
          hintStyle: "error" as const,
        }
      : emptyStringWarning
        ? {
            hint: emptyStringWarning,
            hintStyle: "warning" as const,
          }
        : whitespaceWarning
          ? {
              hint: whitespaceWarning,
              hintStyle: "warning" as const,
            }
          : { hint: null, hintStyle: null };

  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Auto-resize textarea based on content
  useEffect(() => {
    const textarea = textareaRef.current;
    if (textarea) {
      textarea.style.height = "auto";
      textarea.style.height = `${textarea.scrollHeight}px`;
    }
  }, [value]);

  return (
    <>
      <div className="relative overflow-hidden">
        <textarea
          ref={textareaRef}
          id={id}
          aria-label="Value"
          className={cn(
            "block min-h-(--env-var-contents-height) w-full",
            "resize-none",
            "overflow-hidden rounded-md border bg-background-secondary",
            "px-2 py-1.5",
            "font-mono text-sm break-all whitespace-break-spaces placeholder-content-tertiary",
            "focus:outline-hidden",
            "disabled:cursor-not-allowed disabled:bg-background-tertiary disabled:text-content-secondary",
            hintStyle === "error"
              ? "focus:border-content-error"
              : hintStyle === "warning"
                ? "focus:border-content-warning"
                : "text-content-primary focus:border-border-selected",
          )}
          disabled={formState.isSubmitting}
          {...formState.getFieldProps(formKey)}
          autoComplete="off"
          spellCheck={false}
          autoFocus={autoFocus}
          rows={1}
        />
        {hasAnyWhitespace && (
          <pre
            className="pointer-events-none absolute inset-0 mt-0.5 ml-px max-h-full max-w-full animate-fadeInFromLoading overflow-hidden rounded-md p-1.5 px-2 font-mono break-all whitespace-pre-wrap"
            aria-hidden="true"
          >
            {whitespaceOverlayParts.map((part, i) => (
              <span key={i} className={part.className}>
                {part.text}
              </span>
            ))}
          </pre>
        )}
      </div>
      {hint && (
        <p
          className={cn(
            "mt-1 flex max-w-full animate-fadeInFromLoading gap-1 text-xs wrap-break-word",
            hintStyle === "error"
              ? "text-content-errorSecondary"
              : "text-content-warning",
          )}
          role="alert"
        >
          {hint}
        </p>
      )}
    </>
  );
}
