import {
  ExceptionReportingIntegration,
  integrationUsingLegacyFormat,
} from "@common/lib/integrationHelpers";
import { Button } from "@ui/Button";
import { Combobox } from "@ui/Combobox";
import { TextInput } from "@ui/TextInput";
import { useFormik } from "formik";
import {
  useCreateLogStream,
  useUpdateLogStream,
  useDeleteLogStream,
} from "@common/lib/integrationsApi";
import { toast } from "@common/lib/utils";
import { Link } from "@ui/Link";
import * as Yup from "yup";

const sentryValidationSchema = Yup.object().shape({
  dsn: Yup.string().url().required("Sentry DSN is required"),
  tags: Yup.string()
    .test("is-valid-json", "Tags must be a valid JSON object", (value, ctx) => {
      if (!value) return true; // Allow empty value
      try {
        const parsed = JSON.parse(value);
        return (
          typeof parsed === "object" &&
          parsed !== null &&
          !Array.isArray(parsed)
        );
      } catch (e) {
        return ctx.createError({
          message: `Tags must be a valid JSON object: ${e}`,
        });
      }
    })
    .nullable(),
});

export function SentryConfigurationForm({
  onClose,
  integration,
  onAddedIntegration,
}: {
  onClose: () => void;
  integration: Extract<ExceptionReportingIntegration, { kind: "sentry" }>;
  onAddedIntegration?: () => void;
}) {
  const createLogStream = useCreateLogStream();
  const updateLogStream = useUpdateLogStream();
  const deleteLogStream = useDeleteLogStream();
  const existingConfig = integration.existing?.config ?? null;
  const logStreamId = integration.existing?._id;
  const isUsingLegacyFormat = integrationUsingLegacyFormat(existingConfig);

  const isNewIntegration = existingConfig === null || !logStreamId;

  const formState = useFormik<{
    dsn: string;
    tags: string | undefined;
    version: "1" | "2";
  }>({
    initialValues: {
      dsn: existingConfig?.dsn ?? "",
      tags: existingConfig?.tags
        ? JSON.stringify(existingConfig.tags)
        : undefined,
      version: existingConfig?.version ?? "2",
    },
    onSubmit: async (values, helpers) => {
      helpers.setStatus(undefined);
      try {
        const isUpgradingToV2 = isUsingLegacyFormat && values.version === "2";

        if (isNewIntegration || isUpgradingToV2) {
          // If upgrading from v1 to v2, delete the old log stream first
          if (isUpgradingToV2 && logStreamId) {
            await deleteLogStream(logStreamId);
          }
          // Create new integration (either truly new, or upgrading v1 to v2)
          await createLogStream({
            logStreamType: "sentry",
            dsn: values.dsn,
            tags: values.tags ? JSON.parse(values.tags) : undefined,
          });
          onAddedIntegration?.();
          toast(
            "success",
            isUpgradingToV2
              ? "Updated Sentry integration"
              : "Created Sentry integration",
          );
        } else {
          // Update existing integration without changing version
          await updateLogStream(logStreamId, {
            logStreamType: "sentry",
            dsn: values.dsn,
            tags: values.tags ? JSON.parse(values.tags) : undefined,
          });
          toast("success", "Updated Sentry integration");
        }
        onClose();
      } catch (e) {
        helpers.setStatus({
          error: e instanceof Error ? e.message : "Failed to save integration.",
        });
      }
    },
    validationSchema: sentryValidationSchema,
  });

  return (
    <form onSubmit={formState.handleSubmit} className="flex flex-col gap-3">
      {isUsingLegacyFormat && (
        <>
          <div className="flex flex-col gap-1">
            Event Format
            <div className="text-xs text-content-secondary">
              The current version uses the <code>stacktrace</code> instead of
              the <code>value</code> field to capture the stacktrace, enabling
              better Sentry grouping and source code integrations.
            </div>
          </div>
          <Combobox
            label="Select event format"
            options={[
              { value: "1", label: "Legacy" },
              { value: "2", label: "Current" },
            ]}
            selectedOption={formState.values.version}
            setSelectedOption={async (v) => {
              await formState.setFieldValue("version", v);
            }}
            disableSearch
            allowCustomValue={false}
            buttonClasses="w-full bg-inherit"
          />
        </>
      )}
      <TextInput
        value={formState.values.dsn}
        onChange={formState.handleChange}
        label="Sentry Data Source Name (DSN)"
        placeholder="https://xxxx@xxxx.ingest.sentry.io/xxxx"
        id="dsn"
        error={formState.errors.dsn}
        description={
          <>
            Sentry Data Source Name (DSN) to route exceptions to.{" "}
            <Link
              href="https://docs.sentry.io/product/sentry-basics/concepts/dsn-explainer/"
              target="_blank"
            >
              Learn more
            </Link>
          </>
        }
      />
      <TextInput
        value={formState.values.tags}
        onChange={formState.handleChange}
        label="Tags"
        placeholder='{"key": "value"}'
        id="tags"
        error={formState.errors.tags}
        description="Tags to add to all events routed to Sentry. Use JSON format."
      />
      <div className="flex items-center justify-end gap-3">
        {formState.status?.error && (
          <p className="text-sm text-content-errorSecondary" role="alert">
            {formState.status.error}
          </p>
        )}
        <Button
          variant="primary"
          type="submit"
          aria-label="save"
          disabled={!formState.dirty || formState.isSubmitting}
          loading={formState.isSubmitting}
        >
          Save
        </Button>
      </div>
    </form>
  );
}
