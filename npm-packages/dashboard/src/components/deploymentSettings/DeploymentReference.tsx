import { ReactNode, useCallback, useId, useState } from "react";
import { useFormik } from "formik";
import * as Yup from "yup";
import { Button } from "@ui/Button";
import { TextInput } from "@ui/TextInput";
import { Pencil1Icon } from "@radix-ui/react-icons";
import * as Sentry from "@sentry/nextjs";
import { Sheet } from "@ui/Sheet";
import { CopyButton } from "@common/elements/CopyButton";
import { DEPLOYMENT_SETTINGS_SECTIONS } from "lib/sectionAnchors";

const referenceValidationSchema = Yup.object().shape({
  reference: Yup.string()
    .required("Reference is required")
    .min(3, "References must be at least 3 characters")
    .max(100, "References must be at most 100 characters")
    .matches(
      /^[a-z0-9/-]+$/,
      "References can only contain lowercase letters, numbers, hyphens, and slashes",
    )
    .test(
      "not-deployment-name-format",
      'References can\'t look like "word-word-123" — that format is reserved for automatically-generated deployment names. Try something like dev/my-feature or staging instead.',
      (value) => {
        if (!value) return true;
        // Check if it matches the deployment name pattern: word-word-number
        return !/^[a-z]+-[a-z]+-\d+$/.test(value);
      },
    )
    .test(
      "not-local-prefix",
      "References cannot start with 'local-' or 'local/'",
      (value) => {
        if (!value) return true;
        const valueLower = value.toLowerCase();
        return (
          !valueLower.startsWith("local-") && !valueLower.startsWith("local/")
        );
      },
    )
    .test(
      "not-reserved",
      '"${value}" is reserved as a deployment alias and can\'t be used as a reference.',
      (value) => {
        if (!value) return true;
        const valueLower = value.toLowerCase();
        const reserved = [
          "prod",
          "dev",
          "cloud",
          "local",
          "default",
          "name",
          "new",
          "existing",
          "deployment",
          "preview",
        ];
        return !reserved.includes(valueLower);
      },
    ),
});

export function DeploymentReference({
  value,
  onUpdate,
  canManage,
  disabledTip,
}: {
  value: string;
  onUpdate: (reference: string) => Promise<void>;
  canManage: boolean;
  disabledTip?: ReactNode;
}) {
  const textFieldId = useId();

  const [isEditing, setIsEditing] = useState(false);

  const formState = useFormik({
    initialValues: {
      reference: value,
    },
    validationSchema: referenceValidationSchema,
    onSubmit: async (values) => {
      if (values.reference === undefined) {
        Sentry.captureMessage(
          "Unexpectedly submitting DeploymentReference with an undefined value",
          "error",
        );
        return;
      }

      await onUpdate(values.reference);
      setIsEditing(false);
    },
    enableReinitialize: true,
  });

  const handleCancel = useCallback(() => {
    formState.resetForm();
    setIsEditing(false);
  }, [formState]);

  return (
    <Sheet id={DEPLOYMENT_SETTINGS_SECTIONS.deploymentReference.id}>
      <h4 className="mb-2">Deployment Reference</h4>
      <p className="mb-4 text-xs text-content-secondary">
        You can use the reference to target this deployment from the CLI (e.g.{" "}
        <code>--deployment&nbsp;{value ?? "<reference>"}</code>).
      </p>

      <div className="flex flex-wrap items-start gap-x-2 gap-y-4 sm:flex-nowrap">
        {!isEditing ? (
          <>
            <TextInput
              id={textFieldId}
              label="Reference"
              labelHidden
              value={value}
              disabled
            />
            <CopyButton
              text={value ?? ""}
              disabled={value === undefined}
              size="sm"
            />
            <Button
              variant="neutral"
              onClick={() => setIsEditing(true)}
              disabled={!canManage}
              tip={
                canManage
                  ? undefined
                  : (disabledTip ??
                    "You do not have permission to edit the deployment reference.")
              }
              icon={<Pencil1Icon />}
              aria-label="Edit deployment reference"
            >
              Edit
            </Button>
          </>
        ) : (
          <form onSubmit={formState.handleSubmit} className="contents">
            <TextInput
              id={textFieldId}
              label="Reference"
              labelHidden
              error={
                (formState.touched.reference && formState.errors.reference) ||
                undefined
              }
              disabled={formState.isSubmitting}
              {...formState.getFieldProps("reference")}
            />
            <Button
              type="button"
              variant="neutral"
              onClick={handleCancel}
              disabled={formState.isSubmitting}
            >
              Undo Edit
            </Button>
            <Button
              type="submit"
              variant="primary"
              disabled={formState.isSubmitting || !formState.isValid}
              loading={formState.isSubmitting}
            >
              Save
            </Button>
          </form>
        )}
      </div>
    </Sheet>
  );
}
