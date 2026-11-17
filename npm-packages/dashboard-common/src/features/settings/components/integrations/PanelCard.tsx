import classNames from "classnames";
import {
  ExportIntegrationType,
  ImportIntegrationType,
} from "system-udfs/convex/_system/frontend/common";
import { ExternalLinkIcon } from "@radix-ui/react-icons";
import {
  Dialog,
  DialogPanel,
  DialogTitle,
  Transition,
  TransitionChild,
} from "@headlessui/react";
import { Button } from "@ui/Button";
import { Modal } from "@ui/Modal";
import { ClosePanelButton } from "@ui/ClosePanelButton";
import { Tooltip } from "@ui/Tooltip";
import Link from "next/link";
import {
  IntegrationUnavailableReason,
  LogIntegration,
  ExceptionReportingIntegration,
  AuthIntegration,
  AnalyticsIntegration,
  integrationToLogo,
  STREAMING_EXPORT_DESCRIPTION,
  STREAMING_IMPORT_DESCRIPTION,
  LOG_STREAMS_DESCRIPTION,
  ANALYTICS_EXPORT_DESCRIPTION,
  AUTHENTICATION_DESCRIPTION,
} from "@common/lib/integrationHelpers";
import { useState, useCallback } from "react";
import { FivetranSyncStatus } from "./FivetranSyncProgress";
import { IntegrationTitle } from "./IntegrationTitle";
import { IntegrationOverflowMenu } from "./IntegrationOverflowMenu";
import { IntegrationStatus } from "./IntegrationStatus";
import { S3ExportStatus } from "./S3ExportStatus";
import { AxiomConfigurationForm } from "./AxiomConfigurationForm";
import { DatadogConfigurationForm } from "./DatadogConfigurationForm";
import { SentryConfigurationForm } from "./SentryConfigurationForm";
import { WebhookConfigurationForm } from "./WebhookConfigurationForm";
import { PostHogLogsConfigurationForm } from "./PostHogLogsConfigurationForm";
import { PostHogErrorTrackingConfigurationForm } from "./PostHogErrorTrackingConfigurationForm";
import { S3ExportConfigurationForm } from "./S3ExportConfigurationForm";
import { WorkOSConfigurationForm } from "./WorkOSConfigurationForm";
import { WorkOSIntegrationStatus } from "./WorkOSIntegrationStatus";
import { WorkOSIntegrationOverflowMenu } from "./WorkOSIntegrationOverflowMenu";

export type PanelCardProps = {
  className?: string;
  integration:
    | LogIntegration
    | ExceptionReportingIntegration
    | AnalyticsIntegration
    | AuthIntegration
    | { kind: ExportIntegrationType }
    | { kind: ImportIntegrationType };
  unavailableReason: IntegrationUnavailableReason | null;
  teamSlug?: string;
  onAddedIntegration?: (kind: string) => void;
  /** When true, disable the configure/delete/+ actions and surface
   *  `writeDisabledTip` on them. The card itself is still rendered so
   *  members without write access can see what's available. */
  writeDisabled?: boolean;
  writeDisabledTip?: React.ReactNode;
  configureDisabled?: boolean;
  configureDisabledTip?: React.ReactNode;
};

function ProBadge({ teamSlug }: { teamSlug?: string }) {
  const badge = (
    <span className="cursor-pointer rounded-sm bg-util-accent px-1.5 py-0.5 text-xs font-semibold tracking-wider text-white uppercase">
      Pro
    </span>
  );

  if (!teamSlug) {
    return <Tooltip tip="Only available on the Pro plan">{badge}</Tooltip>;
  }

  return (
    <Tooltip tip="Only available on the Pro plan">
      <Link href={`/${teamSlug}/settings/billing`}>{badge}</Link>
    </Tooltip>
  );
}

export function PanelCard({
  className,
  integration,
  unavailableReason,
  teamSlug,
  onAddedIntegration,
  writeDisabled = false,
  writeDisabledTip,
  configureDisabled,
  configureDisabledTip,
}: PanelCardProps) {
  const classes = classNames(
    "py-3 px-4",
    "items-center gap-2.5 transition-colors rounded-sm text-sm font-medium",
    "border",
    className,
  );
  const { logo } = integrationToLogo(integration.kind);

  const [isModalOpen, setIsModalOpen] = useState(false);

  const closeModal = useCallback(() => {
    setIsModalOpen(false);
  }, []);

  return (
    <div className={classes}>
      {integration.kind === "workos" && (
        <div className="flex flex-wrap items-center justify-between gap-2">
          {isModalOpen && renderForm(integration, closeModal)}
          <IntegrationTitle
            logo={logo}
            integrationKind={integration.kind}
            description={AUTHENTICATION_DESCRIPTION}
          />
          <div className="flex items-center gap-4">
            <WorkOSIntegrationStatus integration={integration} />
            <WorkOSIntegrationOverflowMenu
              integration={integration}
              onConfigure={() => setIsModalOpen(true)}
              disabled={writeDisabled}
              disabledTip={writeDisabledTip}
            />
          </div>
        </div>
      )}
      {integration.kind === "fivetran" && (
        <div className="flex flex-wrap items-center justify-between gap-2">
          <IntegrationTitle
            logo={logo}
            integrationKind={integration.kind}
            description={STREAMING_EXPORT_DESCRIPTION}
          />
          <div className="ml-auto">
            {/* The listing endpoint 403s without the entitlement, so a team
                that can't have a sync never asks for one. */}
            {unavailableReason === "MissingEntitlement" ? (
              <ProBadge teamSlug={teamSlug} />
            ) : (
              <FivetranSyncStatus
                setupHref={exportSetupLink(integration.kind)}
              />
            )}
          </div>
        </div>
      )}
      {integration.kind === "airbyte" && (
        <div className="flex flex-wrap items-center justify-between gap-2">
          <IntegrationTitle
            logo={logo}
            integrationKind={integration.kind}
            description={STREAMING_IMPORT_DESCRIPTION}
          />
          <div className="ml-auto">
            <Button
              href={importSetupLink(integration.kind)}
              target="_blank"
              className="flex items-center gap-2"
              inline
              variant="neutral"
            >
              <div>Get Started</div>
              <ExternalLinkIcon />
            </Button>
          </div>
        </div>
      )}
      {(integration.kind === "sentry" ||
        integration.kind === "axiom" ||
        integration.kind === "datadog" ||
        integration.kind === "webhook" ||
        integration.kind === "postHogLogs" ||
        integration.kind === "postHogErrorTracking" ||
        integration.kind === "s3Export") && (
        <div className="flex flex-wrap items-center justify-between gap-2">
          {isModalOpen &&
            renderForm(integration, closeModal, onAddedIntegration)}
          <div className="flex min-w-0 flex-col gap-1">
            <IntegrationTitle
              logo={logo}
              integrationKind={integration.kind}
              description={
                integration.kind === "s3Export"
                  ? ANALYTICS_EXPORT_DESCRIPTION
                  : LOG_STREAMS_DESCRIPTION
              }
            />
            {integration.kind === "s3Export" && integration.existing && (
              <p className="truncate font-mono text-xs text-content-secondary">
                s3://{integration.existing.config.bucket}/
                {integration.existing.config.prefix ?? ""} ·{" "}
                {integration.existing.config.region}
              </p>
            )}
          </div>
          <div className="flex items-center gap-4">
            {integration.kind === "s3Export" && integration.existing ? (
              <S3ExportStatus existing={integration.existing} />
            ) : (
              <IntegrationStatus integration={integration} />
            )}
            {unavailableReason === "MissingEntitlement" ? (
              <ProBadge teamSlug={teamSlug} />
            ) : (
              <IntegrationOverflowMenu
                integration={integration}
                onConfigure={() => setIsModalOpen(true)}
                disabled={writeDisabled}
                disabledTip={writeDisabledTip}
                configureDisabled={configureDisabled}
                configureDisabledTip={configureDisabledTip}
              />
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function exportSetupLink(kind: ExportIntegrationType): string {
  switch (kind) {
    case "fivetran":
      return "https://fivetran.com/integrations/convex";
    default: {
      kind satisfies never;
      return "";
    }
  }
}

function importSetupLink(kind: ImportIntegrationType): string {
  switch (kind) {
    case "airbyte":
      return "https://docs.airbyte.com/integrations/destinations/convex";
    default: {
      kind satisfies never;
      return "";
    }
  }
}

function renderForm(
  integration:
    | LogIntegration
    | ExceptionReportingIntegration
    | AnalyticsIntegration
    | AuthIntegration,
  closeModal: () => void,
  onAddedIntegration?: (kind: string) => void,
) {
  // If we have a callback and this is a new integration, pass it to the form.
  const addedIntegrationProp =
    onAddedIntegration && integration.existing === null
      ? { onAddedIntegration: () => onAddedIntegration(integration.kind) }
      : {};

  switch (integration.kind) {
    case "datadog": {
      return (
        <LogIntegrationSidePanel
          closeModal={closeModal}
          title="Configure Datadog"
          description="Configure your Convex deployment to route logs to Datadog to persist your logs and enable custom log queries and dashboards."
        >
          {(closePanel) => (
            <DatadogConfigurationForm
              integration={integration}
              onClose={closePanel}
              {...addedIntegrationProp}
            />
          )}
        </LogIntegrationSidePanel>
      );
    }
    case "axiom":
      return (
        <LogIntegrationSidePanel
          closeModal={closeModal}
          title="Configure Axiom"
          description="Configure your Convex deployment to route logs to Axiom to persist your logs and enable custom log queries and dashboards."
        >
          {(closePanel) => (
            <AxiomConfigurationForm
              integration={integration}
              onClose={closePanel}
              {...addedIntegrationProp}
            />
          )}
        </LogIntegrationSidePanel>
      );
    case "webhook":
      return (
        <LogIntegrationSidePanel
          closeModal={closeModal}
          title="Configure Webhook"
          description="Configure your Convex deployment to send JSON logs via POST requests."
        >
          {(closePanel) => (
            <WebhookConfigurationForm
              onClose={closePanel}
              integration={integration}
              {...addedIntegrationProp}
            />
          )}
        </LogIntegrationSidePanel>
      );

    case "sentry":
      return (
        <Modal onClose={closeModal} title="Configure Sentry">
          <div className="flex flex-col gap-4">
            <div className="max-w-prose text-xs text-pretty text-content-secondary">
              Configure your Convex deployment to route function execution
              exceptions to Sentry for visibility.
            </div>
            <SentryConfigurationForm
              integration={integration}
              onClose={closeModal}
              {...addedIntegrationProp}
            />
          </div>
        </Modal>
      );
    case "postHogLogs":
      return (
        <LogIntegrationSidePanel
          closeModal={closeModal}
          title="Configure PostHog Logs"
          description="Configure your Convex deployment to stream function logs to PostHog for querying and analysis."
        >
          {(closePanel) => (
            <PostHogLogsConfigurationForm
              integration={integration}
              onClose={closePanel}
              {...addedIntegrationProp}
            />
          )}
        </LogIntegrationSidePanel>
      );
    case "postHogErrorTracking":
      return (
        <Modal onClose={closeModal} title="Configure PostHog Error Tracking">
          <div className="flex flex-col gap-4">
            <div className="max-w-prose text-xs text-pretty text-content-secondary">
              Configure your Convex deployment to route function execution
              exceptions to PostHog Error Tracking for visibility and analysis.
            </div>
            <PostHogErrorTrackingConfigurationForm
              integration={integration}
              onClose={closeModal}
              {...addedIntegrationProp}
            />
          </div>
        </Modal>
      );
    case "s3Export":
      return (
        <LogIntegrationSidePanel
          closeModal={closeModal}
          title="Configure AWS S3"
          description="Export this deployment's change log to an S3 bucket you own."
        >
          {(closePanel) => (
            <S3ExportConfigurationForm
              integration={integration}
              onClose={closePanel}
              {...addedIntegrationProp}
            />
          )}
        </LogIntegrationSidePanel>
      );
    case "workos":
      return (
        <Modal onClose={closeModal} title="Configure WorkOS AuthKit">
          <WorkOSConfigurationForm />
        </Modal>
      );
    default: {
      integration satisfies never;
      return null;
    }
  }
}

function LogIntegrationSidePanel({
  title,
  description,
  closeModal,
  children,
}: {
  title: string;
  description: string;
  closeModal: () => void;
  children: (closePanel: () => void) => React.ReactNode;
}) {
  const [open, setOpen] = useState(true);
  const closePanel = useCallback(() => setOpen(false), []);

  return (
    <Transition show={open} appear afterLeave={closeModal}>
      <Dialog
        static
        as="div"
        className="fixed inset-0 z-50 overflow-hidden"
        open // Real openness status is controlled by Transition above
        onClose={closePanel}
      >
        <div className="absolute inset-0 overflow-hidden">
          <TransitionChild
            enter="ease-in-out duration-300"
            enterFrom="opacity-0"
            enterTo="opacity-100"
            leave="ease-in-out duration-300"
            leaveFrom="opacity-100"
            leaveTo="opacity-0"
          >
            <div className="absolute inset-0 bg-black/50 transition-opacity" />
          </TransitionChild>

          <div className="fixed inset-y-0 right-0 flex max-w-full pl-10">
            <TransitionChild
              enter="transform transition ease-in-out duration-200 sm:duration-300"
              enterFrom="translate-x-full"
              enterTo="translate-x-0"
              leave="transform transition ease-in-out duration-200 sm:duration-300"
              leaveFrom="translate-x-0"
              leaveTo="translate-x-full"
            >
              <DialogPanel className="w-screen max-w-2xl">
                <div className="flex h-full flex-col bg-background-secondary shadow-xl dark:border">
                  <div className="flex items-start justify-between px-6 pt-6 pb-4">
                    <div>
                      <DialogTitle as="h4">{title}</DialogTitle>
                      <p className="mt-1 max-w-prose text-sm text-content-secondary">
                        {description}
                      </p>
                    </div>
                    <ClosePanelButton onClose={closePanel} />
                  </div>
                  {children(closePanel)}
                </div>
              </DialogPanel>
            </TransitionChild>
          </div>
        </div>
      </Dialog>
    </Transition>
  );
}
