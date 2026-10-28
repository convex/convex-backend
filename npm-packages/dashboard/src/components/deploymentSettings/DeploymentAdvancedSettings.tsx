import { useState, useCallback, useMemo } from "react";
import {
  useCurrentDeployment,
  useModifyDeploymentSettings,
} from "api/deployments";
import { useCurrentProject } from "api/projects";
import { useCurrentTeam, useTeamEntitlements } from "api/teams";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
} from "api/roles";
import { deploymentResource } from "lib/permissions";
import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { Checkbox } from "@ui/Checkbox";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { ExclamationTriangleIcon } from "@radix-ui/react-icons";
import { Tooltip } from "@ui/Tooltip";
import { HelpTooltip } from "@ui/HelpTooltip";
import { LiveTimestampDistance } from "@common/elements/TimestampDistance";
import { cn } from "@ui/cn";
import type { DeploymentType } from "@convex-dev/platform/managementApi";
import { THIRTY_MINUTES_MS, toDateTimeLocalValue } from "@common/lib/format";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { DeploymentReference } from "./DeploymentReference";
import { DEPLOYMENT_SETTINGS_SECTIONS } from "lib/sectionAnchors";

type TriStateValue = boolean | null;

function getDefaultSendLogsToClient(deploymentType: DeploymentType): boolean {
  return deploymentType === "dev" || deploymentType === "preview";
}

function getDefaultDashboardEditConfirmation(
  deploymentType: DeploymentType,
): boolean {
  return deploymentType === "prod";
}

function triStateLabel(defaultValue: boolean): string {
  return defaultValue ? "(enabled)" : "(disabled)";
}

function isSecurityWarningDeployment(deploymentType: DeploymentType): boolean {
  return deploymentType === "prod" || deploymentType === "custom";
}

function triStateDisplayValue(value: TriStateValue): string {
  if (value === null) return "Default";
  return value ? "Enabled" : "Disabled";
}

function TriStateRadioGroup({
  name,
  value,
  onChange,
  defaultForType,
  defaultTooltip,
  disabled,
}: {
  name: string;
  value: TriStateValue;
  onChange: (value: TriStateValue) => void;
  defaultForType: boolean;
  defaultTooltip?: string;
  disabled: boolean;
}) {
  const options: { optionValue: TriStateValue; optionLabel: string }[] = [
    {
      optionValue: null,
      optionLabel: `Default ${triStateLabel(defaultForType)}`,
    },
    { optionValue: true, optionLabel: "Enabled" },
    { optionValue: false, optionLabel: "Disabled" },
  ];

  return (
    <div
      role="radiogroup"
      aria-label={name}
      className={cn(
        "flex flex-col gap-1.5",
        disabled && "cursor-not-allowed opacity-50",
      )}
    >
      {options.map(({ optionValue, optionLabel }, index) => {
        const isSelected = value === optionValue;
        return (
          <div
            key={String(optionValue)}
            role="radio"
            aria-checked={isSelected}
            tabIndex={disabled ? -1 : 0}
            onClick={() => {
              if (!disabled) onChange(optionValue);
            }}
            onKeyDown={(e) => {
              if (disabled) return;
              if (e.key === " " || e.key === "Enter") {
                e.preventDefault();
                onChange(optionValue);
              }
              if (e.key === "ArrowDown" || e.key === "ArrowRight") {
                e.preventDefault();
                const next = options[(index + 1) % options.length];
                onChange(next.optionValue);
                (
                  e.currentTarget.parentElement?.children[
                    (index + 1) % options.length
                  ] as HTMLElement
                )?.focus();
              }
              if (e.key === "ArrowUp" || e.key === "ArrowLeft") {
                e.preventDefault();
                const prev =
                  options[(index - 1 + options.length) % options.length];
                onChange(prev.optionValue);
                (
                  e.currentTarget.parentElement?.children[
                    (index - 1 + options.length) % options.length
                  ] as HTMLElement
                )?.focus();
              }
            }}
            className={cn(
              "flex items-center gap-2 text-sm",
              disabled ? "cursor-not-allowed" : "cursor-pointer",
              "rounded-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-util-accent",
            )}
          >
            <div
              className={cn(
                "flex size-4 shrink-0 items-center justify-center rounded-full border",
                isSelected
                  ? "border-util-accent bg-util-accent"
                  : "border-border-transparent bg-background-secondary",
              )}
            >
              {isSelected && <div className="size-1.5 rounded-full bg-white" />}
            </div>
            {optionLabel}
            {optionValue === null && defaultTooltip && (
              <HelpTooltip tipSide="right">{defaultTooltip}</HelpTooltip>
            )}
          </div>
        );
      })}
    </div>
  );
}

type SaveFn = (args: {
  sendLogsToClient?: boolean | null;
  dashboardEditConfirmation?: boolean | null;
  expiresAt?: number | null;
  reference?: string;
}) => Promise<unknown>;

export function DeploymentAdvancedSettings() {
  const deployment = useCurrentDeployment();
  const project = useCurrentProject();
  const team = useCurrentTeam();
  const entitlements = useTeamEntitlements(team?.id);
  const isAdmin = useHasProjectAdminPermissions(project?.id);
  const resource =
    project && deployment && deployment.kind === "cloud"
      ? deploymentResource(project, {
          id: deployment.id,
          deploymentType: deployment.deploymentType,
          creator: deployment.creator ?? null,
        })
      : undefined;
  const canUpdateReferenceCustom = useHasCustomRolePermission(
    team?.id,
    "deployment:updateReference",
    resource,
    false,
  );
  const canUpdateSendLogsCustom = useHasCustomRolePermission(
    team?.id,
    "deployment:updateSendLogsToClient",
    resource,
    false,
  );
  const canUpdateEditConfirmCustom = useHasCustomRolePermission(
    team?.id,
    "deployment:updateDashboardEditConfirmation",
    resource,
    false,
  );
  const canUpdateExpiresAtCustom = useHasCustomRolePermission(
    team?.id,
    "deployment:updateExpiresAt",
    resource,
    false,
  );
  const modifySettings = useModifyDeploymentSettings({
    deploymentName: deployment?.name,
    projectId: project?.id,
  });

  if (deployment === undefined) return null;
  if (deployment.kind === "local") return null;

  const canUpdateReference = isAdmin || canUpdateReferenceCustom === true;
  const canUpdateSendLogs = isAdmin || canUpdateSendLogsCustom === true;
  const canUpdateEditConfirm = isAdmin || canUpdateEditConfirmCustom === true;
  const canUpdateExpiresAt = isAdmin || canUpdateExpiresAtCustom === true;
  const deploymentType = deployment.deploymentType;

  return (
    <>
      <DeploymentReference
        value={deployment.reference}
        canManage={canUpdateReference}
        disabledTip={permissionDeniedTip(
          "You do not have permission to edit the deployment reference.",
          "deployment:updateReference",
        )}
        onUpdate={(reference) => modifySettings({ reference })}
      />
      <TriStateSettingSheet
        id={DEPLOYMENT_SETTINGS_SECTIONS.sendLogsToClient.id}
        title="Send Logs to Client"
        description="Whether function logs and errors are sent to the calling client."
        value={deployment.sendLogsToClient ?? null}
        deploymentType={deploymentType}
        defaultForType={getDefaultSendLogsToClient(deploymentType)}
        defaultTooltip={
          getDefaultSendLogsToClient(deploymentType)
            ? "Logs are enabled by default for dev and preview deployments to aid debugging."
            : "Logs are disabled by default for production deployments to avoid exposing sensitive information."
        }
        getWarning={(value, defaultValue) => {
          if (!isSecurityWarningDeployment(deploymentType) || value === null) {
            return null;
          }
          if (value === true && !defaultValue) {
            return "Sending logs to the client may expose sensitive information in production.";
          }
          if (value === false && defaultValue) {
            return "Disabling client logs will make debugging more difficult.";
          }
          return null;
        }}
        fieldName="sendLogsToClient"
        disabled={!canUpdateSendLogs}
        disabledTip={permissionDeniedTip(
          "You do not have permission to change this setting.",
          "deployment:updateSendLogsToClient",
        )}
        onSave={modifySettings}
      />
      <TriStateSettingSheet
        id={DEPLOYMENT_SETTINGS_SECTIONS.dashboardEditConfirmation.id}
        title="Dashboard Edit Confirmation"
        description="Whether the dashboard requires confirmation before allowing edits."
        value={deployment.dashboardEditConfirmation ?? null}
        deploymentType={deploymentType}
        defaultForType={getDefaultDashboardEditConfirmation(deploymentType)}
        defaultTooltip={
          getDefaultDashboardEditConfirmation(deploymentType)
            ? "Edit confirmation is enabled by default for production deployments to prevent accidental changes."
            : "Edit confirmation is disabled by default for dev and preview deployments for faster iteration."
        }
        getWarning={(value, defaultValue) => {
          if (!isSecurityWarningDeployment(deploymentType) || value === null) {
            return null;
          }
          if (value === false && defaultValue) {
            return "Disabling edit confirmation may lead to accidental changes in production.";
          }
          return null;
        }}
        fieldName="dashboardEditConfirmation"
        disabled={!canUpdateEditConfirm}
        disabledTip={permissionDeniedTip(
          "You do not have permission to change this setting.",
          "deployment:updateDashboardEditConfirmation",
        )}
        onSave={modifySettings}
      />
      <DeploymentExpirySheet
        expiresAt={deployment.expiresAt ?? null}
        deploymentType={deploymentType}
        previewRetentionDays={entitlements?.previewDeploymentRetentionDays}
        disabled={
          !canUpdateExpiresAt
            ? permissionDeniedTip(
                "You do not have permission to edit deployment expiry.",
                "deployment:updateExpiresAt",
              )
            : deploymentType === "prod"
              ? "Production deployments cannot be set to expire."
              : undefined
        }
        onSave={modifySettings}
      />
    </>
  );
}

function TriStateSettingSheet({
  id,
  title,
  description,
  value: initialValue,
  deploymentType,
  defaultForType,
  defaultTooltip,
  getWarning,
  fieldName,
  disabled,
  disabledTip,
  onSave,
}: {
  id: string;
  title: string;
  description: string;
  value: TriStateValue;
  deploymentType: DeploymentType;
  defaultForType: boolean;
  defaultTooltip?: string;
  getWarning: (value: TriStateValue, defaultValue: boolean) => string | null;
  fieldName: "sendLogsToClient" | "dashboardEditConfirmation";
  disabled: boolean;
  disabledTip?: React.ReactNode;
  onSave: SaveFn;
}) {
  const [value, setValue] = useState<TriStateValue>(initialValue);
  const [isSaving, setIsSaving] = useState(false);
  const [pendingValue, setPendingValue] = useState<TriStateValue | undefined>(
    undefined,
  );

  const warningText = getWarning(value, defaultForType);
  const pendingWarningText =
    pendingValue !== undefined
      ? getWarning(pendingValue, defaultForType)
      : null;

  const handleChange = useCallback(
    (newValue: TriStateValue) => {
      if (newValue !== initialValue) {
        setPendingValue(newValue);
      } else {
        setValue(newValue);
      }
    },
    [initialValue],
  );

  const executeSave = useCallback(async () => {
    if (pendingValue === undefined) return;
    setIsSaving(true);
    try {
      await onSave({ [fieldName]: pendingValue });
      setValue(pendingValue);
    } finally {
      setIsSaving(false);
      setPendingValue(undefined);
    }
  }, [onSave, fieldName, pendingValue]);

  return (
    <Sheet id={id}>
      <h4 className="mb-2">{title}</h4>
      <p className="mb-4 text-xs text-content-secondary">{description}</p>
      <div className="flex flex-col gap-3">
        <Tooltip
          tip={disabled && disabledTip ? disabledTip : undefined}
          className="w-fit"
        >
          <TriStateRadioGroup
            name={fieldName}
            value={value}
            onChange={handleChange}
            defaultForType={defaultForType}
            defaultTooltip={defaultTooltip}
            disabled={disabled || isSaving}
          />
        </Tooltip>
        {warningText && (
          <div className="flex w-fit items-center gap-2 rounded-lg border bg-background-warning px-3 py-2 text-sm text-content-warning">
            <ExclamationTriangleIcon className="size-4 shrink-0" />
            <span>{warningText}</span>
          </div>
        )}
      </div>

      {pendingValue !== undefined && (
        <ConfirmationDialog
          onClose={() => setPendingValue(undefined)}
          onConfirm={executeSave}
          dialogTitle={`Confirm ${title} Change`}
          dialogBody={
            <div className="flex flex-col gap-3 text-sm">
              <p>
                You are modifying settings on a {deploymentType} deployment.
              </p>
              <div className="flex flex-col gap-2 rounded-md border bg-background-tertiary p-3">
                <div className="flex flex-col gap-0.5">
                  <span className="font-medium">{title}</span>
                  <span className="text-content-secondary">
                    {triStateDisplayValue(initialValue)} →{" "}
                    {triStateDisplayValue(pendingValue)}
                  </span>
                </div>
              </div>
              {pendingWarningText && (
                <div className="flex w-fit items-center gap-2 rounded-lg border bg-background-warning px-3 py-2 text-sm text-content-warning">
                  <ExclamationTriangleIcon className="size-4 shrink-0" />
                  <span>{pendingWarningText}</span>
                </div>
              )}
            </div>
          }
          confirmText="Save Changes"
          variant="danger"
        />
      )}
    </Sheet>
  );
}

export function DeploymentExpirySheet({
  expiresAt: initialExpiresAt,
  deploymentType,
  previewRetentionDays,
  disabled,
  onSave,
}: {
  expiresAt: number | null;
  deploymentType: DeploymentType;
  previewRetentionDays: number | undefined;
  disabled?: React.ReactNode;
  onSave: SaveFn;
}) {
  const [hasExpiry, setHasExpiry] = useState(initialExpiresAt !== null);
  const defaultExpiryMs =
    previewRetentionDays !== undefined
      ? previewRetentionDays * 24 * 60 * 60 * 1000
      : THIRTY_MINUTES_MS;
  const [expiryDate, setExpiryDate] = useState<Date>(
    initialExpiresAt !== null
      ? new Date(initialExpiresAt)
      : new Date(Date.now() + defaultExpiryMs),
  );
  const [isSaving, setIsSaving] = useState(false);
  const [showConfirmation, setShowConfirmation] = useState(false);

  const minExpiryDate = useMemo(
    () => new Date(Date.now() + THIRTY_MINUTES_MS),
    [],
  );
  const maxExpiryDate = useMemo(() => {
    if (previewRetentionDays === undefined) return undefined;
    return new Date(Date.now() + previewRetentionDays * 24 * 60 * 60 * 1000);
  }, [previewRetentionDays]);

  const isDirty =
    hasExpiry !== (initialExpiresAt !== null) ||
    (hasExpiry && expiryDate.getTime() !== initialExpiresAt);

  const isExpiryValid =
    !hasExpiry ||
    (expiryDate.getTime() >= minExpiryDate.getTime() &&
      (maxExpiryDate === undefined ||
        expiryDate.getTime() <= maxExpiryDate.getTime()));

  const executeSave = useCallback(async () => {
    setIsSaving(true);
    try {
      await onSave({ expiresAt: hasExpiry ? expiryDate.getTime() : null });
    } finally {
      setIsSaving(false);
    }
  }, [onSave, hasExpiry, expiryDate]);

  const handleSave = useCallback(() => {
    setShowConfirmation(true);
  }, []);

  return (
    <Sheet id={DEPLOYMENT_SETTINGS_SECTIONS.deploymentExpiry.id}>
      <h4 className="mb-2">Deployment Expiry</h4>
      <p className="mb-4 text-xs text-content-secondary">
        Set a time at which this deployment will be automatically deleted.
      </p>
      <div className="flex flex-col gap-3">
        <div>
          <Tooltip className="w-auto" tip={disabled}>
            <label
              className={cn(
                "flex items-center gap-2 text-sm",
                (!!disabled || isSaving) && "cursor-not-allowed opacity-50",
              )}
            >
              <Checkbox
                checked={hasExpiry}
                onChange={() => setHasExpiry(!hasExpiry)}
                disabled={!!disabled || isSaving}
              />
              <span>This deployment will expire at</span>
              {hasExpiry ? (
                <input
                  type="datetime-local"
                  value={toDateTimeLocalValue(expiryDate)}
                  min={toDateTimeLocalValue(minExpiryDate)}
                  max={
                    maxExpiryDate
                      ? toDateTimeLocalValue(maxExpiryDate)
                      : undefined
                  }
                  onChange={(e) => {
                    if (e.target.value) {
                      setExpiryDate(new Date(e.target.value));
                    }
                  }}
                  disabled={!!disabled || isSaving}
                  className={cn(
                    "w-fit rounded-md border bg-background-secondary px-3 py-1.5 text-sm text-content-primary",
                    "focus:border-border-selected focus:outline-none",
                    "disabled:cursor-not-allowed disabled:opacity-50",
                  )}
                />
              ) : (
                <span
                  className={cn(
                    "w-fit rounded-md border bg-background-secondary px-3 py-1.5 text-sm text-content-secondary",
                    "cursor-not-allowed",
                  )}
                >
                  Never
                </span>
              )}
              {hasExpiry && <LiveTimestampDistance date={expiryDate} />}
            </label>
          </Tooltip>
        </div>
        {hasExpiry && !isExpiryValid && (
          <div className="flex w-fit items-center gap-2 rounded-lg border bg-background-error px-3 py-2 text-sm text-content-error">
            <ExclamationTriangleIcon className="size-4 shrink-0" />
            <span>
              The expiry time must be between 30 minutes and{" "}
              {previewRetentionDays} days from now.
            </span>
          </div>
        )}
        <div className="flex justify-start">
          <Button
            variant={hasExpiry ? "danger" : "primary"}
            disabled={!isDirty || isSaving || !!disabled || !isExpiryValid}
            loading={isSaving}
            onClick={handleSave}
          >
            Save
          </Button>
        </div>
      </div>

      {showConfirmation && (
        <ConfirmationDialog
          onClose={() => setShowConfirmation(false)}
          onConfirm={executeSave}
          dialogTitle="Confirm Deployment Expiry Change"
          dialogBody={
            <div className="flex flex-col gap-3 text-sm">
              <p>
                You are modifying settings on a {deploymentType} deployment.
              </p>
              <div className="flex flex-col gap-2 rounded-md border bg-background-tertiary p-3">
                <div className="flex flex-col gap-0.5">
                  <span className="font-medium">Deployment Expiry</span>
                  <span className="text-content-secondary">
                    {initialExpiresAt !== null
                      ? new Date(initialExpiresAt).toLocaleString(undefined, {
                          timeZoneName: "short",
                        })
                      : "None"}{" "}
                    →{" "}
                    {hasExpiry
                      ? expiryDate.toLocaleString(undefined, {
                          timeZoneName: "short",
                        })
                      : "None"}
                  </span>
                </div>
              </div>
            </div>
          }
          confirmText="Save Changes"
          validationText={
            hasExpiry && isSecurityWarningDeployment(deploymentType)
              ? "Automatically delete this deployment in the future"
              : undefined
          }
          variant="danger"
        />
      )}
    </Sheet>
  );
}
