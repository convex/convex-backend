import { ExceptionReportingIntegration } from "@common/lib/integrationHelpers";
import { Button } from "@ui/Button";
import { Link } from "@ui/Link";
import { TextInput } from "@ui/TextInput";
import { useFormik } from "formik";
import {
  useCreateLogStream,
  useUpdateLogStream,
} from "@common/lib/integrationsApi";
import { toast } from "@common/lib/utils";
import * as Yup from "yup";

const validationSchema = Yup.object().shape({
  apiKey: Yup.string().required("PostHog project token is required"),
  host: Yup.string().url("Must be a valid URL").nullable(),
});

export function PostHogErrorTrackingConfigurationForm({
  onClose,
  integration,
  onAddedIntegration,
}: {
  onClose: () => void;
  integration: Extract<
    ExceptionReportingIntegration,
    { kind: "postHogErrorTracking" }
  >;
  onAddedIntegration?: () => void;
}) {
  const createLogStream = useCreateLogStream();
  const updateLogStream = useUpdateLogStream();
  const existingConfig = integration.existing?.config ?? null;
  const logStreamId = integration.existing?._id;

  const isNewIntegration = existingConfig === null || !logStreamId;

  const formState = useFormik<{
    apiKey: string;
    host: string;
  }>({
    initialValues: {
      apiKey: existingConfig?.apiKey ?? "",
      host: existingConfig?.host ?? "",
    },
    onSubmit: async (values, helpers) => {
      helpers.setStatus(undefined);
      try {
        const args = {
          logStreamType: "postHogErrorTracking" as const,
          apiKey: values.apiKey,
          host: values.host || null,
        };

        if (isNewIntegration) {
          await createLogStream(args);
          onAddedIntegration?.();
          toast("success", "Created PostHog Error Tracking integration");
        } else {
          await updateLogStream(logStreamId, args);
          toast("success", "Updated PostHog Error Tracking integration");
        }
        onClose();
      } catch (e) {
        helpers.setStatus({
          error: e instanceof Error ? e.message : "Failed to save integration.",
        });
      }
    },
    validationSchema,
  });

  return (
    <form onSubmit={formState.handleSubmit} className="flex flex-col gap-3">
      <TextInput
        value={formState.values.apiKey}
        onChange={formState.handleChange}
        label="Project Token"
        placeholder="phc_..."
        id="apiKey"
        error={formState.errors.apiKey}
        description={
          <>
            Your PostHog project token. Found in PostHog under{" "}
            <Link
              href="https://app.posthog.com/settings/project-details#variables"
              target="_blank"
              rel="noopener noreferrer"
            >
              Settings &gt; Project &gt; General
            </Link>
            .
          </>
        }
      />
      <TextInput
        value={formState.values.host}
        onChange={formState.handleChange}
        label="Host (optional)"
        placeholder="https://us.i.posthog.com"
        id="host"
        error={formState.errors.host}
        description="PostHog host URL (the endpoint path is added automatically). Defaults to US Cloud. Use https://eu.i.posthog.com for EU Cloud, or your self-hosted URL."
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
