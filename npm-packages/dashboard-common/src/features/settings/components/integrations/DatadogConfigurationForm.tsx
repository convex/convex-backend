import { Link } from "@ui/Link";
import * as Yup from "yup";
import { useFormik } from "formik";
import { DatadogSiteLocation } from "system-udfs/convex/_system/frontend/common";
import { EyeNoneIcon, EyeOpenIcon } from "@radix-ui/react-icons";
import { useContext, useState } from "react";
import { TextInput } from "@ui/TextInput";
import { Button } from "@ui/Button";
import { Combobox, Option } from "@ui/Combobox";
import type { LogTopic } from "@convex-dev/platform/deploymentApi";
import {
  integrationUsingLegacyFormat,
  LogIntegration,
  topicsValidationSchema,
} from "@common/lib/integrationHelpers";
import { LogTopicsSelector } from "./LogTopicsSelector";
import {
  useCreateLogStream,
  useUpdateLogStream,
  useDeleteLogStream,
} from "@common/lib/integrationsApi";
import { toast } from "@common/lib/utils";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

const siteLocationOptions: Option<DatadogSiteLocation>[] = [
  { value: "US1", label: "US1" },
  { value: "US3", label: "US3" },
  { value: "US5", label: "US5" },
  { value: "EU", label: "EU1" },
  { value: "US1_FED", label: "US1-FED" },
  { value: "AP1", label: "AP1" },
];

const datadogValidationSchema = Yup.object().shape({
  siteLocation: Yup.string().required("Site location is required"),
  ddApiKey: Yup.string().required("API key is required"),
  ddTags: Yup.string(),
  topics: topicsValidationSchema,
});

export function DatadogConfigurationForm({
  integration,
  onClose,
  onAddedIntegration,
}: {
  onClose: () => void;
  integration: Extract<LogIntegration, { kind: "datadog" }>;
  onAddedIntegration?: () => void;
}) {
  const createLogStream = useCreateLogStream();
  const updateLogStream = useUpdateLogStream();
  const deleteLogStream = useDeleteLogStream();
  const existingConfig = integration.existing?.config ?? null;
  const logStreamId = integration.existing?._id;
  const isUsingLegacyFormat = integrationUsingLegacyFormat(existingConfig);

  const { useCurrentProject } = useContext(DeploymentInfoContext);
  const project = useCurrentProject();

  const isNewIntegration = existingConfig === null || !logStreamId;

  const [showApiKey, setShowApiKey] = useState(false);

  const formState = useFormik<{
    siteLocation: DatadogSiteLocation;
    ddApiKey: string;
    ddTags: string;
    service: string | null;
    version: "1" | "2";
    topics: LogTopic[] | null;
  }>({
    initialValues: {
      siteLocation: existingConfig?.siteLocation ?? "US1",
      ddApiKey: existingConfig?.ddApiKey ?? "",
      ddTags: existingConfig?.ddTags.join(",") ?? "",
      service:
        (existingConfig ? existingConfig.service : project?.name) ?? null,
      version: existingConfig !== null ? (existingConfig.version ?? "1") : "2",
      topics: existingConfig?.topics ?? null,
    },
    onSubmit: async (values, helpers) => {
      helpers.setStatus(undefined);
      try {
        const isUpgradingToV2 = isUsingLegacyFormat && values.version === "2";
        const ddTags = values.ddTags.split(",").filter((v) => v !== "");

        if (isNewIntegration || isUpgradingToV2) {
          // If upgrading from v1 to v2, delete the old log stream first
          if (isUpgradingToV2 && logStreamId) {
            await deleteLogStream(logStreamId);
          }
          // Create new integration (either truly new, or upgrading v1 to v2)
          await createLogStream({
            logStreamType: "datadog",
            siteLocation: values.siteLocation,
            ddApiKey: values.ddApiKey,
            ddTags,
            service: values.service,
            topics: values.topics ?? undefined,
          });
          onAddedIntegration?.();
          toast(
            "success",
            isUpgradingToV2
              ? "Updated Datadog integration"
              : "Created Datadog integration",
          );
        } else {
          // Update existing integration without changing version
          await updateLogStream(logStreamId, {
            logStreamType: "datadog",
            siteLocation: values.siteLocation,
            ddApiKey: values.ddApiKey,
            ddTags,
            service: values.service,
            topics: values.topics,
          });
          toast("success", "Updated Datadog integration");
        }
        onClose();
      } catch (e) {
        helpers.setStatus({
          error: e instanceof Error ? e.message : "Failed to save integration.",
        });
      }
    },
    validationSchema: datadogValidationSchema,
  });
  return (
    <form
      onSubmit={formState.handleSubmit}
      className="flex min-h-0 flex-1 flex-col"
    >
      <div className="scrollbar flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-6 pb-4">
        {isUsingLegacyFormat && (
          <>
            <div className="flex flex-col gap-1">
              Event Format
              <div className="text-xs text-content-secondary">
                Format for events sent in this stream.{" "}
                <Link
                  href="https://docs.convex.dev/production/integrations/log-streams/legacy-event-schema"
                  target="_blank"
                >
                  Learn more
                </Link>
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
              allowCustomValue={false}
              buttonClasses="w-full bg-inherit"
            />
          </>
        )}
        <div className="flex flex-col gap-1">
          <Combobox
            label="Site Location"
            labelHidden={false}
            options={siteLocationOptions}
            selectedOption={formState.values.siteLocation}
            setSelectedOption={async (loc) => {
              await formState.setFieldValue("siteLocation", loc);
            }}
            allowCustomValue={false}
            buttonClasses="w-full bg-inherit"
          />
          <div className="max-w-prose text-xs text-content-secondary">
            Location of your Datadog deployment.{" "}
            <Link
              href="https://docs.datadoghq.com/getting_started/site/"
              target="_blank"
            >
              Learn more
            </Link>
          </div>
        </div>
        <TextInput
          label="API Key"
          value={formState.values.ddApiKey}
          type={showApiKey ? "text" : "password"}
          onChange={formState.handleChange}
          id="ddApiKey"
          className="max-w-full"
          placeholder="xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
          action={() => setShowApiKey(!showApiKey)}
          Icon={showApiKey ? EyeNoneIcon : EyeOpenIcon}
          error={formState.errors.ddApiKey}
          description="API key is used to authenticate with Datadog."
        />
        <TextInput
          value={formState.values.service || undefined}
          onChange={formState.handleChange}
          placeholder={project?.name}
          label="Service"
          id="service"
          error={formState.errors.service}
          description="Service name used as a special tag in Datadog."
        />
        <TextInput
          value={formState.values.ddTags}
          onChange={formState.handleChange}
          id="ddTags"
          label="Tags"
          error={formState.errors.ddTags}
          description={
            <div className="text-xs text-content-secondary">
              Optional comma-separated list of tags. These are sent to Datadog
              in each log event via the <code>ddtags</code> field.{" "}
              <Link
                href="https://docs.datadoghq.com/getting_started/tagging/"
                target="_blank"
              >
                Learn more
              </Link>
            </div>
          }
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
