import * as Yup from "yup";
import { Link } from "@ui/Link";
import { useFormik } from "formik";
import { Button } from "@ui/Button";
import { TextInput } from "@ui/TextInput";
import {
  useCreateLogStream,
  useRotateWebhookSecret,
  useUpdateLogStream,
} from "@common/lib/integrationsApi";
import { Combobox } from "@ui/Combobox";
import {
  ClipboardCopyIcon,
  EyeNoneIcon,
  EyeOpenIcon,
} from "@radix-ui/react-icons";
import { useState } from "react";
import { copyTextToClipboard, toast } from "@common/lib/utils";
import { Snippet } from "@common/elements/Snippet";
import type { LogTopic } from "@convex-dev/platform/deploymentApi";
import {
  LogIntegration,
  topicsValidationSchema,
} from "@common/lib/integrationHelpers";
import { LogTopicsSelector } from "./LogTopicsSelector";

const webhookValidationSchema = Yup.object().shape({
  url: Yup.string().url().required("URL required"),
  format: Yup.string().oneOf(["json", "jsonl"]).required("Format required"),
  topics: topicsValidationSchema,
});

function HmacSecretDisplay({
  hmacSecret,
  initialShowSecret = false,
}: {
  hmacSecret: string;
  initialShowSecret?: boolean;
}) {
  const [showSecret, setShowSecret] = useState(initialShowSecret);

  const maskedSecret = "••••••••••••••••••••••••••••••••";
  const displayValue = showSecret ? hmacSecret : maskedSecret;

  return (
    <>
      <span className="text-left text-sm text-content-primary">
        HMAC Secret
      </span>
      <div className="flex flex-col gap-2">
        <div className="flex flex-1 flex-row items-center gap-2">
          <Snippet value={displayValue} monospace className="flex-1" />
          <Button
            tip={showSecret ? "Hide secret" : "Show secret"}
            type="button"
            onClick={() => setShowSecret(!showSecret)}
            size="xs"
            variant="neutral"
            icon={showSecret ? <EyeNoneIcon /> : <EyeOpenIcon />}
          />
          <Button
            tip="Copy value"
            type="button"
            onClick={async () => {
              await copyTextToClipboard(hmacSecret);
              toast("success", "HMAC secret copied to clipboard");
            }}
            size="xs"
            variant="neutral"
            icon={<ClipboardCopyIcon />}
          />
        </div>
        <p className="text-sm text-content-secondary">
          Use this secret to verify webhook signatures.{" "}
          {!initialShowSecret &&
            "Regenerating the secret will immediately invalidate the old secret. "}
          <Link
            href="https://docs.convex.dev/production/integrations/log-streams/#webhook"
            target="_blank"
            rel="noopener noreferrer"
          >
            Learn more about securing webhooks.
          </Link>
        </p>
      </div>
    </>
  );
}

export function WebhookConfigurationForm({
  onClose,
  integration,
  onAddedIntegration,
}: {
  onClose: () => void;
  integration: Extract<LogIntegration, { kind: "webhook" }>;
  onAddedIntegration?: () => void;
}) {
  const createLogStream = useCreateLogStream();
  const rotateWebhookSecret = useRotateWebhookSecret();
  const updateLogStream = useUpdateLogStream();
  const [showSecretRevealScreen, setShowSecretRevealScreen] = useState(false);
  const [isRotatingSecret, setIsRotatingSecret] = useState(false);
  const [rotateSecretStatus, setRotateSecretStatus] = useState<{
    type: "success" | "error";
    message: string;
  } | null>(null);
  const existingIntegration = integration.existing?.config;
  const logStreamId = integration.existing?._id;
  const isHmacSecretLoading =
    showSecretRevealScreen && !existingIntegration?.hmacSecret;

  const isNewIntegration = existingIntegration === null || !logStreamId;

  const formState = useFormik<{
    url: string;
    format: "json" | "jsonl";
    topics: LogTopic[] | null;
  }>({
    initialValues: {
      url: existingIntegration?.url ?? "",
      format: existingIntegration?.format ?? "jsonl",
      topics: existingIntegration?.topics ?? null,
    },
    onSubmit: async (values, helpers) => {
      helpers.setStatus(undefined);
      try {
        if (isNewIntegration) {
          await createLogStream({
            logStreamType: "webhook",
            url: values.url,
            format: values.format,
            topics: values.topics ?? undefined,
          });
          onAddedIntegration?.();
          toast("success", "Created webhook integration");
          setShowSecretRevealScreen(true);
        } else {
          await updateLogStream(logStreamId, {
            logStreamType: "webhook",
            url: values.url,
            format: values.format,
            topics: values.topics,
          });
          toast("success", "Updated webhook integration");
          onClose();
        }
      } catch (e) {
        helpers.setStatus({
          error: e instanceof Error ? e.message : "Failed to save integration.",
        });
      }
    },
    validationSchema: webhookValidationSchema,
  });

  // Show the secret reveal screen
  if (showSecretRevealScreen && existingIntegration?.hmacSecret) {
    return (
      <>
        <div className="scrollbar flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 pb-4">
          <HmacSecretDisplay
            hmacSecret={existingIntegration.hmacSecret}
            initialShowSecret
          />
        </div>
        <div className="flex justify-end px-6 py-4">
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        </div>
      </>
    );
  }

  return (
    <form
      onSubmit={formState.handleSubmit}
      className="flex min-h-0 flex-1 flex-col"
    >
      <div className="scrollbar flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-6 pb-4">
        <TextInput
          value={formState.values.url}
          onChange={formState.handleChange}
          id="url"
          label="URL"
          placeholder="Enter a URL to send logs to"
          error={formState.errors.url}
        />
        <div className="flex flex-col gap-1">
          <Combobox
            label="Format"
            labelHidden={false}
            options={[
              {
                value: "jsonl",
                label: "JSONL (one object per line of request)",
              },
              { value: "json", label: "JSON (one array per request)" },
            ]}
            selectedOption={formState.values.format}
            setSelectedOption={async (v) => {
              await formState.setFieldValue("format", v);
            }}
            allowCustomValue={false}
            disableSearch
            buttonClasses="w-full bg-inherit"
          />
        </div>
        <LogTopicsSelector
          value={formState.values.topics}
          onChange={async (topics) => {
            await formState.setFieldValue("topics", topics);
          }}
          error={formState.errors.topics as string | undefined}
        />
        {existingIntegration?.hmacSecret && logStreamId && (
          <>
            <HmacSecretDisplay hmacSecret={existingIntegration?.hmacSecret} />
            <div className="flex items-center gap-2">
              <Button
                type="button"
                onClick={async () => {
                  setRotateSecretStatus(null);
                  setIsRotatingSecret(true);
                  try {
                    await rotateWebhookSecret(logStreamId);
                    setRotateSecretStatus({
                      type: "success",
                      message: "HMAC Secret regenerated",
                    });
                  } catch (e) {
                    setRotateSecretStatus({
                      type: "error",
                      message:
                        e instanceof Error
                          ? e.message
                          : "Failed to regenerate secret.",
                    });
                  } finally {
                    setIsRotatingSecret(false);
                  }
                }}
                variant="neutral"
                loading={isRotatingSecret}
                disabled={isRotatingSecret}
              >
                Regenerate secret
              </Button>
              {rotateSecretStatus && (
                <span
                  className={
                    rotateSecretStatus.type === "error"
                      ? "text-xs text-content-errorSecondary"
                      : "text-xs text-content-success"
                  }
                  role={
                    rotateSecretStatus.type === "error" ? "alert" : undefined
                  }
                >
                  {rotateSecretStatus.message}
                </span>
              )}
            </div>
          </>
        )}
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
          disabled={isHmacSecretLoading || formState.isSubmitting}
        >
          Cancel
        </Button>
        <Button
          variant="primary"
          type="submit"
          aria-label="save"
          disabled={
            !formState.dirty || isHmacSecretLoading || formState.isSubmitting
          }
          loading={isHmacSecretLoading || formState.isSubmitting}
        >
          Save
        </Button>
      </div>
    </form>
  );
}
