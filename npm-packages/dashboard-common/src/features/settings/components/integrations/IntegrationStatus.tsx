import { Tooltip } from "@ui/Tooltip";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { ExclamationTriangleIcon } from "@radix-ui/react-icons";
import {
  LogIntegration,
  ExceptionReportingIntegration,
  AnalyticsIntegration,
  integrationUsingLegacyFormat,
} from "@common/lib/integrationHelpers";
import { HealthIndicator } from "./HealthIndicator";

export function IntegrationStatus({
  integration,
}: {
  integration:
    | LogIntegration
    | ExceptionReportingIntegration
    | AnalyticsIntegration;
}) {
  return !integration.existing ? null : (
    <div className="flex flex-col gap-2">
      <div className="flex flex-col items-center">
        <div className="ml-auto flex gap-2">
          <HealthIndicator status={integration.existing.status} />
          {integrationUsingLegacyFormat(integration.existing.config) && (
            <Tooltip
              className="text-left text-xs text-content-warning"
              tip={
                integration.existing.config.type === "sentry"
                  ? "This integration is using the legacy exception format. Re-configure this integration to update the exception format."
                  : "This integration is using the legacy event format. Re-configure this integration to update the event format."
              }
            >
              <ExclamationTriangleIcon className="inline" />
            </Tooltip>
          )}
        </div>
        <p className="text-xs text-content-secondary">
          Created{" "}
          <TimestampDistance
            date={new Date(integration.existing._creationTime)}
          />
        </p>
      </div>
    </div>
  );
}
