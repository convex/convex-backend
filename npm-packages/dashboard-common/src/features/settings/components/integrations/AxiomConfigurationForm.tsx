import { Link } from "@ui/Link";
import * as Yup from "yup";
import { FieldArray, FormikProvider, getIn, useFormik } from "formik";
import {
  EyeNoneIcon,
  EyeOpenIcon,
  PlusIcon,
  TrashIcon,
} from "@radix-ui/react-icons";
import { AxiomConfig } from "system-udfs/convex/_system/frontend/common";
import { Combobox } from "@ui/Combobox";
import { Button } from "@ui/Button";
import { TextInput } from "@ui/TextInput";
import type { LogTopic } from "@convex-dev/platform/deploymentApi";
import {
  integrationUsingLegacyFormat,
  LogIntegration,
  topicsValidationSchema,
} from "@common/lib/integrationHelpers";
import { useState } from "react";
import { LogTopicsSelector } from "./LogTopicsSelector";
import {
  useCreateLogStream,
  useUpdateLogStream,
  useDeleteLogStream,
} from "@common/lib/integrationsApi";
import { toast } from "@common/lib/utils";

const axiomValidationSchema = Yup.object().shape({
  datasetName: Yup.string().required("Dataset name is required"),
  apiKey: Yup.string().required("API key is required"),
  attributes: Yup.array().of(
    Yup.object().shape({
      key: Yup.string().required("Name is required"),
      value: Yup.string().required("Value is required"),
    }),
  ),
  ingestUrl: Yup.string(),
  topics: topicsValidationSchema,
});

type Unpacked<T> = T extends (infer U)[] ? U : never;

export function AxiomConfigurationForm({
  onClose,
  integration,
  onAddedIntegration,
}: {
  onClose: () => void;
  integration: Extract<LogIntegration, { kind: "axiom" }>;
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
    datasetName: string;
    apiKey: string;
    attributes: Unpacked<AxiomConfig["attributes"]>[];
    version: "1" | "2";
    ingestUrl: string;
    topics: LogTopic[] | null;
  }>({
    initialValues: {
      datasetName: existingConfig?.datasetName ?? "",
      apiKey: existingConfig?.apiKey ?? "",
      attributes: existingConfig?.attributes ?? [],
      version: existingConfig !== null ? (existingConfig.version ?? "1") : "2",
      ingestUrl: existingConfig?.ingestUrl ?? "https://api.axiom.co",
      topics: existingConfig?.topics ?? null,
    },
    onSubmit: async (values, helpers) => {
      helpers.setStatus(undefined);
      const isUpgradingToV2 = isUsingLegacyFormat && values.version === "2";

      try {
        if (isNewIntegration || isUpgradingToV2) {
          // If upgrading from v1 to v2, delete the old log stream first
          if (isUpgradingToV2 && logStreamId) {
            await deleteLogStream(logStreamId);
          }
          // Create new integration (either truly new, or upgrading v1 to v2)
          await createLogStream({
            logStreamType: "axiom",
            datasetName: values.datasetName,
            apiKey: values.apiKey,
            attributes: values.attributes,
            ingestUrl: values.ingestUrl,
            topics: values.topics ?? undefined,
          });
          onAddedIntegration?.();
          toast(
            "success",
            isUpgradingToV2
              ? "Updated Axiom integration"
              : "Created Axiom integration",
          );
        } else {
          // Update existing integration without changing version
          await updateLogStream(logStreamId, {
            logStreamType: "axiom",
            datasetName: values.datasetName,
            apiKey: values.apiKey,
            attributes: values.attributes,
            ingestUrl: values.ingestUrl,
            topics: values.topics,
          });
          toast("success", "Updated Axiom integration");
        }
        onClose();
      } catch (e) {
        helpers.setStatus({
          error: e instanceof Error ? e.message : "Failed to save integration.",
        });
      }
    },
    validationSchema: axiomValidationSchema,
  });

  const [showApiKey, setShowApiKey] = useState(false);

  const regionOptions = [
    { value: "https://api.axiom.co", label: "Default" },
    { value: "https://us-east-1.aws.edge.axiom.co", label: "US East 1 (AWS)" },
    {
      value: "https://eu-central-1.aws.edge.axiom.co",
      label: "EU Central 1 (AWS)",
    },
  ];

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
            label="Region"
            labelHidden={false}
            disableSearch
            options={regionOptions}
            selectedOption={formState.values.ingestUrl}
            setSelectedOption={async (v) => {
              await formState.setFieldValue("ingestUrl", v, false);
            }}
            allowCustomValue={false}
          />
          <p className="max-w-prose animate-fadeInFromLoading text-xs text-content-secondary">
            Select the region where your Axiom organization is located. This
            will determine the URL used to send events to Axiom.
          </p>
        </div>

        <TextInput
          value={formState.values.datasetName}
          onChange={formState.handleChange}
          label="Dataset Name"
          id="datasetName"
          error={formState.errors.datasetName}
          description="Name of the dataset in your Axiom organization. This needs to be an existing dataset, or the configuration will fail."
        />
        <TextInput
          label="API Key"
          value={formState.values.apiKey}
          type={showApiKey ? "text" : "password"}
          onChange={formState.handleChange}
          className="max-w-full"
          id="apiKey"
          placeholder="xxxx-xxxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
          action={() => setShowApiKey(!showApiKey)}
          Icon={showApiKey ? EyeNoneIcon : EyeOpenIcon}
          error={formState.errors.apiKey}
          description="API key is used to authenticate with Axiom."
        />
        <div className="flex flex-col gap-1">
          Attributes
          <div className="text-xs text-content-secondary">
            Optional list of attributes. These are extra fields and values sent
            to Axiom in each log event.{" "}
            <Link
              href="https://axiom.co/docs/send-data/ingest#ingest-api"
              target="_blank"
            >
              Learn more
            </Link>
          </div>
        </div>
        <FormikProvider value={formState}>
          <FieldArray
            name="attributes"
            render={(arrayHelpers) => (
              <>
                {formState.values.attributes.map(({ key, value }, idx) => (
                  <div
                    className="flex flex-row items-start justify-between gap-1"
                    key={idx}
                  >
                    <TextInput
                      className="w-full"
                      value={key}
                      labelHidden
                      placeholder="Name"
                      id={`attributes[${idx}].key`}
                      onChange={formState.handleChange}
                      error={getIn(formState.errors, `attributes[${idx}].key`)}
                    />
                    <TextInput
                      className="w-full"
                      labelHidden
                      value={value}
                      placeholder="Value"
                      id={`attributes[${idx}].value`}
                      onChange={formState.handleChange}
                      error={getIn(
                        formState.errors,
                        `attributes[${idx}].value`,
                      )}
                    />
                    <Button
                      onClick={() => arrayHelpers.remove(idx)}
                      variant="danger"
                      size="sm"
                      inline
                      className="mt-1 h-fit"
                      icon={<TrashIcon />}
                    />
                  </div>
                ))}
                <Button
                  variant="neutral"
                  className="ml-auto w-fit"
                  onClick={() => arrayHelpers.push({ key: "", value: "" })}
                >
                  <PlusIcon />
                  Add attribute
                </Button>
              </>
            )}
          />
        </FormikProvider>
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
