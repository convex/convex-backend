import { Tooltip } from "@ui/Tooltip";
import { cn } from "@ui/cn";
import type { SsoConnectionResponse, SsoDomainState } from "generatedApi";

type StatusTone = "success" | "warning" | "error" | "neutral";

const TONE_CLASSES: Record<StatusTone, string> = {
  success: "bg-background-success text-content-primary",
  warning: "bg-background-warning text-content-warning",
  error: "bg-background-error text-content-error",
  neutral: "text-content-secondary",
};

export function StatusBadge({
  label,
  tone,
  tip,
}: {
  label: string;
  tone: StatusTone;
  tip?: React.ReactNode;
}) {
  const badge = (
    <span
      className={cn(
        "rounded-full border px-2 py-0.5 text-xs",
        TONE_CLASSES[tone],
      )}
    >
      {label}
    </span>
  );
  return tip ? (
    <Tooltip tip={tip} side="right">
      {badge}
    </Tooltip>
  ) : (
    badge
  );
}

const DOT_CLASSES: Record<StatusTone, string> = {
  success: "bg-util-success",
  warning: "bg-util-warning",
  error: "bg-util-error",
  neutral: "bg-neutral-8 dark:bg-neutral-4",
};

// The in-table counterpart to the pill, matching the invoices table: a pill
// per row turns a list into a column of lozenges, while a dot carries the same
// tone at the weight of the text beside it.
export function StatusDot({
  label,
  tone,
  tip,
}: {
  label: string;
  tone: StatusTone;
  tip?: React.ReactNode;
}) {
  const status = (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 text-xs font-medium whitespace-nowrap",
        tone === "error" ? "text-content-error" : "text-content-primary",
      )}
    >
      <span
        className={cn("size-1.5 shrink-0 rounded-full", DOT_CLASSES[tone])}
      />
      {label}
    </span>
  );
  return tip ? (
    <Tooltip tip={tip} side="right">
      {status}
    </Tooltip>
  ) : (
    status
  );
}

export function DomainStatusBadge({ state }: { state: SsoDomainState }) {
  if (state === "verified" || state === "legacyVerified") {
    return (
      <StatusDot
        label="Verified"
        tone="success"
        tip="This domain has been verified and may be used with SSO and Directory Sync."
      />
    );
  }
  if (state === "pending") {
    return (
      <StatusDot
        label="Pending"
        tone="warning"
        tip="This domain has not yet completed verification. Check its status with “Add a domain”."
      />
    );
  }
  return (
    <StatusDot
      label="Failed"
      tone="error"
      tip="Verification failed for this domain. Check its status with “Add a domain”."
    />
  );
}

// WorkOS keeps a connection around in a draft/inactive state until the IdP
// side is finished, so `active` is what decides whether members can log in
// through it; `state` only explains why it isn't active yet.
export function ConnectionStatusBadge({
  connection,
}: {
  connection: SsoConnectionResponse;
}) {
  return connection.active ? (
    <StatusBadge
      label="Active"
      tone="success"
      tip="Team members can log in through this identity provider."
    />
  ) : (
    <StatusBadge
      label="Inactive"
      tone="warning"
      tip={`This connection is not active yet (${connection.state}). Finish configuring it with your identity provider.`}
    />
  );
}

export function DirectoryStatusBadge({ state }: { state: string }) {
  switch (state) {
    case "linked":
      return (
        <StatusBadge
          label="Linked"
          tone="success"
          tip="Your identity provider is provisioning team members through this directory."
        />
      );
    case "invalid_credentials":
      return (
        <StatusBadge
          label="Invalid credentials"
          tone="error"
          tip="Your identity provider rejected the directory credentials. Reconnect the directory to resume provisioning."
        />
      );
    default:
      return (
        <StatusBadge
          label="Unlinked"
          tone="warning"
          tip="This directory is not connected to your identity provider yet."
        />
      );
  }
}
