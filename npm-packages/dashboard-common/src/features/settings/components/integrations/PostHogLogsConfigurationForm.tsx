import type { LogTopic } from "@convex-dev/platform/deploymentApi";
import {
  LogIntegration,
  topicsValidationSchema,
} from "@common/lib/integrationHelpers";
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
import { LogTopicsSelector } from "./LogTopicsSelector";

const validationSchema = Yup.object().shape({
  apiKey: Yup.string().required("PostHog project token is required"),
  host: Yup.string().url("Must be a valid URL").nullable(),
  serviceName: Yup.string().nullable(),
  topics: topicsValidationSchema,
});

export function PostHogLogsConfigurationForm({
  onClose,
  integration,
  onAddedIntegration,
}: {
  onClose: () => void;
  integration: Extract<LogIntegration, { kind: "postHogLogs" }>;
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
    serviceName: string;
    topics: LogTopic[] | null;
  }>({
    initialValues: {
      apiKey: existingConfig?.apiKey ?? "",
      host: existingConfig?.host ?? "",
      serviceName: existingConfig?.serviceName ?? "",
      topics: existingConfig?.topics ?? null,
    },
    onSubmit: async (values, helpers) => {
      helpers.setStatus(undefined);
      try {
        const args = {
          logStreamType: "postHogLogs" as const,
          apiKey: values.apiKey,
          host: values.host || null,
          serviceName: values.serviceName || null,
        };

        if (isNewIntegration) {
          await createLogStream({
            ...args,
            topics: values.topics ?? undefined,
          });
          onAddedIntegration?.();
          toast("success", "Created PostHog Logs integration");
        } else {
          await updateLogStream(logStreamId, {
            ...args,
            topics: values.topics,
          });
          toast("success", "Updated PostHog Logs integration");
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
    <form
      onSubmit={formState.handleSubmit}
      className="flex min-h-0 flex-1 flex-col"
    >
      <div className="scrollbar flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-6 pb-4">
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
        <TextInput
          value={formState.values.serviceName}
          onChange={formState.handleChange}
          label="Service Name (optional)"
          placeholder="my-app"
          id="serviceName"
          error={formState.errors.serviceName}
          description="OTLP service name for log attribution. Defaults to your deployment name."
        />
        <LogTopicsSelector
          value={formState.values.topics}
          onChange={async (topics) => {
            await formState.setFieldValue("topics", topics);
          }}
          error={formState.errors.topics as string | undefined}
        />
      </div>
      <div className="flex items-center justify-end gap-2 px-6 py-4">
        {formState.status?.error && (
          <p className="text-sm text-content-errorSecondary" role="alert">
            {formState.status.error}
          </p>
        )}
        <Button
          variant="neutral"
          onClick={onClose}
          disabled={formState.isSubmitting}
        >
          Cancel
        </Button>
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
