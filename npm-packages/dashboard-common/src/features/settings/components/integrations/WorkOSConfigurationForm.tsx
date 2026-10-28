import { useContext, useEffect, useMemo, useState } from "react";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import {
  DeploymentInfo,
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { Link } from "@ui/Link";
import { Loading } from "@ui/Loading";
import { HelpTooltip } from "@ui/HelpTooltip";
import {
  CheckIcon,
  ExclamationTriangleIcon,
  Cross2Icon,
  ExternalLinkIcon,
} from "@radix-ui/react-icons";
import { CopyTextButton } from "@common/elements/CopyTextButton";
import { EnvironmentVariable } from "system-udfs/convex/_system/frontend/common";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";
import { Callout } from "@ui/Callout";
import { Button } from "@ui/Button";
import { Combobox } from "@ui/Combobox";
import { toast } from "@common/lib/utils";
import { useUpdateEnvVars } from "@common/features/settings/lib/api";
import { WorkOSProjectEnvironments } from "./WorkOSProjectEnvironments";
import { WorkOSEnvironmentInfo } from "./WorkOSEnvironmentInfo";
import { WorkOSCredentialsSection } from "./WorkOSCredentialsSection";
import { EnvVarChange, EnvVarChangeRow } from "./WorkOSEnvVarChanges";

type WorkOSEnvVars = {
  clientId: string | null;
  environmentId: string | null;
  apiKey: string | null;
};

type ProvisionedEnvironment = {
  workosEnvironmentId: string;
  workosEnvironmentName: string;
  workosClientId: string;
  workosApiKey?: string; // Optional since backend might not always return it
  workosTeamId: string;
  isProduction: boolean;
};

type ProjectEnvironment = {
  workosEnvironmentId: string;
  workosEnvironmentName: string;
  workosClientId: string;
  userEnvironmentName: string;
  isProduction: boolean;
};

// Helper to extract error info from unknown errors (typically API errors with code/message)
function getErrorInfo(error: unknown): { code: string; message: string } {
  if (error instanceof Error) {
    return {
      code: (error as Error & { code?: string }).code ?? "",
      message: error.message,
    };
  }
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return {
      code: "code" in error && typeof error.code === "string" ? error.code : "",
      message: error.message,
    };
  }
  return { code: "", message: String(error) };
}

function getWorkOSNotSupportedReason(
  deployment: ReturnType<DeploymentInfo["useCurrentDeployment"]>,
): string | undefined {
  if (!deployment) {
    return undefined;
  }
  const { deploymentType, kind } = deployment;

  // Only local dev deployments are not supported
  if (deploymentType === "dev" && kind === "local") {
    return "Automatic environment creation for local deployments is not supported.";
  }

  // Cloud dev, prod, and preview are all supported
  return undefined;
}

function useRelevantEnvVars(): {
  canViewEnv: boolean;
  workosEnvVars: WorkOSEnvVars | null;
} {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewEnv = useIsOperationAllowed("ViewEnvironmentVariables");

  // Fetch deployment-level environment variables
  const environmentVariables: undefined | Array<EnvironmentVariable> = useQuery(
    udfs.listEnvironmentVariables.default,
    canViewEnv ? {} : "skip",
  );

  // Extract WorkOS environment variables
  const workosEnvVars = useMemo(() => {
    if (!environmentVariables) return null;

    const clientId = environmentVariables.find(
      (envVar) => envVar.name === "WORKOS_CLIENT_ID",
    )?.value;
    const environmentId = environmentVariables.find(
      (envVar) => envVar.name === "WORKOS_ENVIRONMENT_ID",
    )?.value;
    const apiKey = environmentVariables.find(
      (envVar) => envVar.name === "WORKOS_API_KEY",
    )?.value;

    return {
      clientId: clientId || null,
      environmentId: environmentId || null,
      apiKey: apiKey || null,
    };
  }, [environmentVariables]);

  return { canViewEnv, workosEnvVars };
}

function InviteTeamMemberSection({
  teamId,
  isExpanded,
  onToggle,
}: {
  teamId?: number;
  isExpanded: boolean;
  onToggle: () => void;
}) {
  const { workOSOperations } = useContext(DeploymentInfoContext);

  const [selectedEmail, setSelectedEmail] = useState<string>("");
  const [isInviting, setIsInviting] = useState(false);

  const eligibleEmailsData = workOSOperations.useWorkOSInvitationEligibleEmails(
    teamId?.toString(),
  );
  const inviteMutation = workOSOperations.useInviteWorkOSTeamMember();

  const hasNoEligibleEmails =
    !eligibleEmailsData || eligibleEmailsData.eligibleEmails.length === 0;

  // If we're expanded but data becomes empty (e.g., refresh), collapse back
  useEffect(() => {
    if (isExpanded && hasNoEligibleEmails) {
      onToggle();
    }
  }, [isExpanded, hasNoEligibleEmails, onToggle]);

  const handleInvite = async () => {
    if (!teamId || !selectedEmail) return;

    setIsInviting(true);
    try {
      await inviteMutation({ teamId, email: selectedEmail });
      setSelectedEmail("");
      onToggle(); // Close the form after successful invitation
    } catch (error) {
      console.error("Failed to send WorkOS invitation:", error);
      // Check if this is the "already a member of another workspace" error
      const { code: errorCode, message: errorMessage } = getErrorInfo(error);
      if (
        errorCode === "WorkosUserAlreadyInWorkspace" ||
        errorMessage.includes("already a member of another team")
      ) {
        // This error indicates the user should already be able to log in
        toast(
          "error",
          `${selectedEmail} is already a member of another WorkOS workspace. Try another email address.`,
        );
      } else {
        // Show a generic error toast for other errors
        toast(
          "error",
          errorMessage || "Failed to send invitation. Please try again.",
        );
      }
    } finally {
      setIsInviting(false);
    }
  };

  if (hasNoEligibleEmails) {
    return null;
  }

  const options = eligibleEmailsData.eligibleEmails.map((email) => ({
    value: email,
    label: email,
  }));

  if (!isExpanded) {
    return (
      <Button size="sm" variant="neutral" onClick={onToggle}>
        Invite to WorkOS
      </Button>
    );
  }

  return (
    <div className="flex flex-col gap-2 rounded-sm border p-3">
      <div className="flex flex-col gap-1">
        <div className="text-sm font-semibold text-content-primary">
          Send invite email to WorkOS
        </div>
        <p className="text-xs text-content-secondary">
          Send or re-send an invitation email to one of your verified emails to
          access the WorkOS dashboard for this workspace (invitations expire
          after some time).
        </p>
        {eligibleEmailsData.adminEmail && (
          <p className="text-xs text-content-tertiary">
            Workspace Admin: {eligibleEmailsData.adminEmail}
          </p>
        )}
      </div>

      <div className="flex flex-col gap-2">
        <Combobox
          label="Select email"
          options={options}
          selectedOption={selectedEmail}
          setSelectedOption={(value) => setSelectedEmail(value || "")}
          disableSearch={options.length <= 5}
          allowCustomValue={false}
          buttonClasses="w-full bg-inherit"
        />

        <div className="flex gap-2">
          <Button
            size="sm"
            onClick={handleInvite}
            disabled={!selectedEmail}
            loading={isInviting}
          >
            Send Invite
          </Button>
          <Button
            size="sm"
            variant="neutral"
            onClick={() => {
              onToggle();
              setSelectedEmail("");
            }}
          >
            Cancel
          </Button>
        </div>
      </div>
    </div>
  );
}

function ProvisionWorkOSTeamSection({
  teamId,
  onTeamCreated,
  hasClientIdConfigured,
}: {
  teamId?: number;
  onTeamCreated?: () => void;
  /** Whether WORKOS_CLIENT_ID environment variable is already set */
  hasClientIdConfigured: boolean;
}) {
  const { workOSOperations } = useContext(DeploymentInfoContext);

  const [showEmailSelection, setShowEmailSelection] = useState(false);
  const [selectedEmail, setSelectedEmail] = useState<string>("");
  const [isProvisioning, setIsProvisioning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const availableEmailsData = workOSOperations.useAvailableWorkOSTeamEmails();
  const provisionMutation = workOSOperations.useProvisionWorkOSTeam(
    teamId?.toString(),
  );

  const handleProvision = async () => {
    if (!teamId || !selectedEmail) return;

    setIsProvisioning(true);
    setError(null);
    try {
      await provisionMutation({ teamId, email: selectedEmail });
      setShowEmailSelection(false);
      setSelectedEmail("");
      if (onTeamCreated) {
        onTeamCreated();
      }
    } catch (e) {
      console.error("Failed to provision WorkOS team:", e);
      // Check for the "already exists" error
      const { code, message } = getErrorInfo(e);
      if (
        message.includes("already") ||
        code === "WorkosAccountAlreadyExistsWithThisEmail"
      ) {
        setError(
          `A WorkOS team already exists with ${selectedEmail}. Please select a different email address.`,
        );
      } else {
        setError("Failed to create WorkOS workspace. Please try again.");
      }
      setIsProvisioning(false);
    }
  };

  if (!availableEmailsData) {
    return <Loading />;
  }

  const hasAvailableEmails = availableEmailsData.availableEmails.length > 0;
  const hasUsedEmails = availableEmailsData.usedEmails.length > 0;

  if (!showEmailSelection) {
    return (
      <div className="flex flex-col gap-2">
        <p className="text-sm text-content-secondary">
          Create a WorkOS team for this team to let Convex to create new{" "}
          <Link href="https://workos.com/docs/authkit/authkit" target="_blank">
            AuthKit
          </Link>{" "}
          environments in that WorkOS workspace.
        </p>
        {hasClientIdConfigured ? (
          <p className="text-sm text-content-secondary">
            You can also use AuthKit without creating a new WorkOS team by{" "}
            <Link
              href="https://workos.com/docs/authkit/authkit"
              target="_blank"
            >
              configuring it manually
            </Link>
            .
          </p>
        ) : null}
        <div>
          <Button
            size="sm"
            variant="primary"
            onClick={() => setShowEmailSelection(true)}
            disabled={!hasAvailableEmails}
            tip={
              hasAvailableEmails
                ? undefined
                : "All your verified emails are already used for other WorkOS teams"
            }
          >
            Create WorkOS Workspace
          </Button>
        </div>
      </div>
    );
  }

  const allEmails = [
    ...availableEmailsData.availableEmails,
    ...availableEmailsData.usedEmails,
  ];

  const emailOptions = allEmails.map((email) => {
    const isUsed = availableEmailsData.usedEmails.includes(email);
    return {
      value: email,
      label: isUsed ? `${email} (already used for another WorkOS team)` : email,
    };
  });

  return (
    <div className="flex flex-col gap-3 rounded-sm border p-3">
      <div className="flex flex-col gap-1">
        <div className="text-sm font-semibold text-content-primary">
          Create WorkOS Workspace
        </div>
        <p className="text-xs text-content-secondary">
          Select one of your verified email addresses to use as the admin for
          the new WorkOS workspace.
        </p>
        {hasUsedEmails && (
          <p className="text-xs text-content-tertiary">
            {availableEmailsData.usedEmails.length} of your email
            {availableEmailsData.usedEmails.length > 1 ? "s are" : " is"}{" "}
            already used for other teams and cannot be selected.
          </p>
        )}
      </div>

      {error && (
        <Callout variant="error">
          <p className="text-xs">{error}</p>
        </Callout>
      )}

      <div className="flex flex-col gap-2">
        <Combobox
          label="Admin email address"
          options={emailOptions.filter(
            (opt) => !availableEmailsData.usedEmails.includes(opt.value),
          )}
          selectedOption={selectedEmail}
          setSelectedOption={(value) => setSelectedEmail(value || "")}
          disableSearch={emailOptions.length <= 5}
          allowCustomValue={false}
          buttonClasses="w-full bg-inherit"
        />

        <div className="flex gap-2">
          <Button
            size="sm"
            onClick={handleProvision}
            disabled={!selectedEmail}
            loading={isProvisioning}
          >
            Create Workspace
          </Button>
          <Button
            size="sm"
            variant="neutral"
            onClick={() => {
              setShowEmailSelection(false);
              setError(null);
              setSelectedEmail("");
            }}
          >
            Cancel
          </Button>
        </div>
      </div>

      <p className="text-xs text-content-tertiary">
        Need to use a different email?{" "}
        <Link href="/profile" target="_blank">
          Add and verify an email in your profile
        </Link>
      </p>
    </div>
  );
}

function WorkOSTeamSection({
  workosTeam,
  environment,
  teamId,
  showCongratulations = false,
  onTeamCreated,
  hasClientIdConfigured,
}: {
  workosTeam:
    | {
        workosTeamId: string;
        workosTeamName: string;
        workosAdminEmail: string;
      }
    | null
    | undefined;
  environment: {
    workosEnvironmentId: string;
    workosEnvironmentName: string;
    workosClientId: string;
    workosTeamId: string;
  } | null;
  teamId?: number;
  showCongratulations?: boolean;
  onTeamCreated?: () => void;
  /** Whether WORKOS_CLIENT_ID environment variable is already set */
  hasClientIdConfigured: boolean;
}) {
  const { workOSOperations } = useContext(DeploymentInfoContext);
  const [isDisconnecting, setIsDisconnecting] = useState(false);
  const [showSuccessMessage, setShowSuccessMessage] = useState(false);
  const [expandedAction, setExpandedAction] = useState<
    "invite" | "disconnect" | null
  >(null);

  // Sync showCongratulations prop to local state (prop changes when team is created)
  useEffect(() => {
    if (showCongratulations) {
      setShowSuccessMessage(true);
    }
  }, [showCongratulations]);

  const teamHealthData = workOSOperations.useWorkOSTeamHealth(
    teamId?.toString(),
  );
  const disconnectMutation = workOSOperations.useDisconnectWorkOSTeam(
    teamId?.toString(),
  );

  const handleDisconnect = async () => {
    if (!teamId || !workosTeam) return;

    setIsDisconnecting(true);
    try {
      await disconnectMutation({ teamId });
    } catch (error) {
      console.error("Failed to disconnect WorkOS team:", error);
      const { message } = getErrorInfo(error);
      toast(
        "error",
        message || "Failed to disconnect WorkOS workspace. Please try again.",
      );
    } finally {
      setIsDisconnecting(false);
      setExpandedAction(null);
    }
  };

  if (!workosTeam) {
    return (
      <ProvisionWorkOSTeamSection
        teamId={teamId}
        onTeamCreated={onTeamCreated}
        hasClientIdConfigured={hasClientIdConfigured}
      />
    );
  }

  const teamInfo = teamHealthData?.data?.teamProvisioned
    ? teamHealthData.data.teamInfo
    : null;
  const canProvisionProduction = teamInfo?.productionState === "active";
  const isLoadingHealth =
    teamHealthData && !teamHealthData.data && !teamHealthData.error;

  return (
    <div className="flex flex-col gap-3">
      {/* Success message for newly created WorkOS team */}
      {showSuccessMessage && !environment && (
        <Callout variant="success">
          <div className="flex flex-col gap-2">
            <div className="font-semibold">
              Congratulations! Your WorkOS workspace has been created.
            </div>
            <div className="text-sm">
              <p className="mb-2">Next steps:</p>
              <ol className="list-inside list-decimal space-y-1">
                <li>
                  (optional) Check your email {workosTeam.workosAdminEmail} for
                  an invitation to the WorkOS dashboard
                </li>
                <li>Create an AuthKit environment for your deployment above</li>
                <li>
                  Add an <code>authKit</code> section to your project's{" "}
                  <Link
                    href="https://docs.convex.dev/auth/authkit/auto-provision"
                    target="_blank"
                  >
                    convex.json
                  </Link>{" "}
                  to configure AuthKit environments automatically
                </li>
                <li>
                  For preview and production deployments only, copy your AuthKit
                  credentials to build environment variables in your hosting
                  platform (e.g. Vercel)
                </li>
              </ol>
            </div>
            <div>
              <Button
                size="xs"
                variant="neutral"
                onClick={() => setShowSuccessMessage(false)}
              >
                Dismiss
              </Button>
            </div>
          </div>
        </Callout>
      )}

      <div className="flex flex-col gap-2">
        <p className="text-sm text-content-primary">
          <span className="inline-flex items-center gap-1">
            <span className="font-mono">{workosTeam.workosTeamName}</span>
            <HelpTooltip>
              <div className="flex flex-col gap-2">
                <div>
                  <div className="text-xs font-semibold">WorkOS Team ID</div>
                  <CopyTextButton
                    text={workosTeam.workosTeamId}
                    className="font-mono text-xs"
                  />
                </div>
                <div>
                  <div className="text-xs font-semibold">Admin Email</div>
                  <div className="font-mono text-xs">
                    {workosTeam.workosAdminEmail}
                  </div>
                </div>
              </div>
            </HelpTooltip>
          </span>
        </p>

        <div className="flex flex-col gap-1 text-xs">
          {teamHealthData && (
            <>
              {teamInfo ? (
                <span className="inline-flex items-center gap-1 text-content-secondary">
                  <CheckIcon className="size-4" /> Convex has access to this
                  WorkOS workspace{" "}
                </span>
              ) : teamHealthData.error?.code === "WorkOSTeamNotFound" ? (
                <span className="inline-flex items-center gap-1 text-content-warning">
                  <ExclamationTriangleIcon className="size-4" />
                  This WorkOS team could not be found. Did you delete it? You
                  may want to click on “Disconnect Workspace”.
                </span>
              ) : teamHealthData.error?.code === "WorkOSTeamDeleted" ? (
                <span className="inline-flex items-center gap-1 text-content-warning">
                  <ExclamationTriangleIcon className="size-4" />
                  This WorkOS team has been deleted. If you want to use WorkOS,
                  click on “Disconnect Workspace”.
                </span>
              ) : teamHealthData.error?.code === "WorkOSAPIUnavailable" ? (
                <span className="inline-flex items-center gap-1 text-content-warning">
                  <ExclamationTriangleIcon className="size-4" /> WorkOS API
                  currently unavailable
                </span>
              ) : !teamHealthData.error && !teamHealthData.data ? (
                <span className="text-content-tertiary">
                  Checking accessibility...
                </span>
              ) : (
                <span className="inline-flex items-center gap-1 text-content-warning">
                  <ExclamationTriangleIcon className="size-4" /> Unable to
                  verify workspace
                </span>
              )}
            </>
          )}
          {isLoadingHealth ? (
            <span className="text-content-tertiary">
              Checking payment status...
            </span>
          ) : teamInfo ? (
            canProvisionProduction ? (
              <span className="inline-flex items-center gap-1 text-content-secondary">
                <CheckIcon className="size-4" /> Payment method configured
              </span>
            ) : (
              <span className="inline-flex items-center gap-1 text-content-secondary">
                <ExclamationTriangleIcon className="size-4 shrink-0 text-content-warning" />
                <span>
                  <Link
                    href="https://dashboard.workos.com/settings/billing"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    Add payment method in WorkOS
                  </Link>{" "}
                  to provision production AuthKit environments.
                </span>
              </span>
            )
          ) : null}
        </div>
      </div>

      <p className="text-sm text-content-secondary">
        Invite a Convex email address to access this WorkOS workspace.
      </p>

      {!expandedAction && (
        <div className="flex gap-2">
          <InviteTeamMemberSection
            teamId={teamId}
            isExpanded={false}
            onToggle={() => setExpandedAction("invite")}
          />
          <Button
            size="sm"
            variant="neutral"
            onClick={() => setExpandedAction("disconnect")}
          >
            Disconnect Workspace
          </Button>
        </div>
      )}

      {expandedAction === "invite" && (
        <InviteTeamMemberSection
          teamId={teamId}
          isExpanded
          onToggle={() => setExpandedAction(null)}
        />
      )}

      {expandedAction === "disconnect" && (
        <div className="flex flex-col gap-2 rounded-sm border border-content-error p-3">
          <div className="flex flex-col gap-1">
            <div className="text-sm font-semibold text-content-primary">
              Disconnect WorkOS Workspace
            </div>
            <p className="text-xs text-content-secondary">
              You can disassociate a WorkOS workspace from your team to revoke
              permission to create AuthKit environments in it from Convex.
              Existing WorkOS environments will continue to work.
            </p>
          </div>
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="danger"
              onClick={handleDisconnect}
              loading={isDisconnecting}
            >
              Disconnect
            </Button>
            <Button
              size="sm"
              variant="neutral"
              onClick={() => setExpandedAction(null)}
            >
              Cancel
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function ConsolidatedEnvironmentSection({
  environment,
  workosTeam,
  hasTeam,
  notSupportedReason,
  envVarsLink,
  workosEnvVars,
  canViewEnv,
  deploymentName,
  deployment,
  canProvisionProduction,
  projectEnvironments,
}: {
  environment: ProvisionedEnvironment | null;
  workosTeam:
    | {
        workosTeamId: string;
        workosTeamName: string;
        workosAdminEmail: string;
      }
    | null
    | undefined;
  hasTeam: boolean;
  notSupportedReason: string | undefined;
  envVarsLink?: string;
  workosEnvVars: WorkOSEnvVars;
  canViewEnv: boolean;
  deploymentName?: string;
  deployment: ReturnType<DeploymentInfo["useCurrentDeployment"]>;
  canProvisionProduction: boolean;
  projectEnvironments?: ProjectEnvironment[];
}) {
  const { workOSOperations } = useContext(DeploymentInfoContext);
  const updateEnvironmentVariables = useUpdateEnvVars();

  // Only check health if an environment is actually provisioned
  const envHealthData = workOSOperations.useWorkOSEnvironmentHealth(
    environment ? deploymentName : undefined,
  );
  const [provisioningType, setProvisioningType] = useState<
    "production" | "non-production" | null
  >(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [isSettingEnvVars, setIsSettingEnvVars] = useState(false);
  const [showSuccessMessage, setShowSuccessMessage] =
    useState<ProvisionedEnvironment | null>(null);
  const [showCredentials, setShowCredentials] = useState(false);
  const [showDeleteForm, setShowDeleteForm] = useState(false);
  // Create mutations with the current deploymentName
  const provisionMutation =
    workOSOperations.useProvisionWorkOSEnvironment(deploymentName);
  const deleteMutation =
    workOSOperations.useDeleteWorkOSEnvironment(deploymentName);

  // Check if env vars match a project-level (shared) environment
  const matchingProjectEnvironment = useMemo(() => {
    if (!workosEnvVars.clientId || !projectEnvironments) return null;
    return (
      projectEnvironments.find(
        (env) => env.workosClientId === workosEnvVars.clientId,
      ) ?? null
    );
  }, [workosEnvVars.clientId, projectEnvironments]);

  const handleProvisionEnvironment = async (isProduction: boolean) => {
    if (!deploymentName) return;

    setProvisioningType(isProduction ? "production" : "non-production");
    try {
      const result = await provisionMutation({
        deploymentName,
        isProduction,
      });

      // If we got an API key from provisioning, set it immediately
      if (result?.apiKey && result?.newlyProvisioned) {
        const changes = [
          { name: "WORKOS_CLIENT_ID", value: result.clientId },
          { name: "WORKOS_ENVIRONMENT_ID", value: result.environmentId },
          { name: "WORKOS_API_KEY", value: result.apiKey },
        ];

        try {
          await updateEnvironmentVariables(changes);
          // Only show success message for production deployments (they get built in Vercel)
          if (deployment?.deploymentType === "prod") {
            const newEnvironment: ProvisionedEnvironment = {
              workosEnvironmentId: result.environmentId,
              workosEnvironmentName:
                result.environmentName || "WorkOS Environment",
              workosClientId: result.clientId,
              workosApiKey: result.apiKey,
              workosTeamId: result.workosTeamId || "",
              isProduction,
            };
            setShowSuccessMessage(newEnvironment);
          } else {
            toast(
              "success",
              "WorkOS environment provisioned and variables configured",
            );
          }
        } catch (error) {
          console.error("Failed to set environment variables:", error);
          toast(
            "info",
            "Environment provisioned but variables need manual configuration",
          );
        }
      }

      // Success! The mutation will automatically revalidate the environment query
      setProvisioningType(null);
      setShowCreateForm(false);
    } catch (error) {
      const { message } = getErrorInfo(error);
      toast(
        "error",
        message || "Failed to provision environment. Please try again.",
      );
      setProvisioningType(null);
    }
  };

  const handleDeleteEnvironment = async () => {
    if (!deploymentName || !environment) return;

    setIsDeleting(true);
    try {
      await deleteMutation({ deploymentName });

      // Clear env vars that matched the deleted environment
      const envVarsToClear: { name: string; value: null }[] = [];
      if (workosEnvVars.clientId === environment.workosClientId) {
        envVarsToClear.push({ name: "WORKOS_CLIENT_ID", value: null });
      }
      if (workosEnvVars.environmentId === environment.workosEnvironmentId) {
        envVarsToClear.push({ name: "WORKOS_ENVIRONMENT_ID", value: null });
      }
      if (
        environment.workosApiKey &&
        workosEnvVars.apiKey === environment.workosApiKey
      ) {
        envVarsToClear.push({ name: "WORKOS_API_KEY", value: null });
      }

      if (envVarsToClear.length > 0) {
        try {
          await updateEnvironmentVariables(envVarsToClear);
        } catch (error) {
          console.error("Failed to clear environment variables:", error);
        }
      }
    } catch (error) {
      const errorMessage =
        error instanceof Error
          ? error.message
          : "Failed to delete environment. Please try again.";
      toast("error", errorMessage);
    } finally {
      setIsDeleting(false);
    }
  };

  const handleSetEnvironmentVariables = async () => {
    if (!environment) return;

    setIsSettingEnvVars(true);
    try {
      const changes = [
        { name: "WORKOS_CLIENT_ID", value: environment.workosClientId },
        {
          name: "WORKOS_ENVIRONMENT_ID",
          value: environment.workosEnvironmentId,
        },
        ...(environment.workosApiKey
          ? [{ name: "WORKOS_API_KEY", value: environment.workosApiKey }]
          : []),
      ];

      await updateEnvironmentVariables(changes);
      toast("success", "Environment variables have been set successfully");
    } catch (error) {
      const errorMessage =
        error instanceof Error
          ? error.message
          : "Failed to set environment variables. Please try again.";
      toast("error", errorMessage);
    } finally {
      setIsSettingEnvVars(false);
    }
  };

  const isProductionDeployment = deployment?.deploymentType === "prod";

  // Check if environment was created for a different team
  const teamMismatch =
    environment &&
    workosTeam &&
    environment.workosTeamId !== workosTeam.workosTeamId;

  // Check environment variable states
  const clientIdMatches =
    environment &&
    workosEnvVars.clientId &&
    workosEnvVars.clientId === environment.workosClientId;
  const environmentIdMatches =
    environment &&
    workosEnvVars.environmentId &&
    workosEnvVars.environmentId === environment.workosEnvironmentId;
  const apiKeyMatches =
    environment &&
    environment.workosApiKey &&
    workosEnvVars.apiKey &&
    workosEnvVars.apiKey === environment.workosApiKey;

  const clientIdMissing = environment && !workosEnvVars.clientId;
  const environmentIdMissing = environment && !workosEnvVars.environmentId;
  const apiKeyMissing = environment && !workosEnvVars.apiKey;

  const clientIdMismatch =
    environment && workosEnvVars.clientId && !clientIdMatches;
  const environmentIdMismatch =
    environment && workosEnvVars.environmentId && !environmentIdMatches;
  const apiKeyMismatch = environment && workosEnvVars.apiKey && !apiKeyMatches;

  return (
    <div className="flex flex-col gap-2">
      {/* Success message for newly created environment */}
      {showSuccessMessage && (
        <Callout variant="success">
          <div className="flex flex-col gap-3">
            <div>
              <div className="text-sm font-semibold">
                Environment Successfully Created!
              </div>
              <p className="mt-1 text-xs text-content-secondary">
                Your{" "}
                {showSuccessMessage.isProduction
                  ? "production"
                  : "non-production"}{" "}
                WorkOS environment has been provisioned and environment
                variables have been configured.
              </p>
            </div>
            <div>
              <p className="mb-2 text-xs text-content-secondary">
                Copy these environment variables to your build environment:
              </p>
              <WorkOSCredentialsSection
                clientId={showSuccessMessage.workosClientId}
                apiKey={showSuccessMessage.workosApiKey}
                isProduction={showSuccessMessage.isProduction}
              />
            </div>
            <Button
              size="xs"
              variant="neutral"
              onClick={() => setShowSuccessMessage(null)}
            >
              Dismiss
            </Button>
          </div>
        </Callout>
      )}

      {environment ? (
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <p className="text-sm text-content-primary">
              <span className="inline-flex items-center gap-1">
                <span>{environment.workosEnvironmentName}</span>
                <WorkOSEnvironmentInfo environment={environment} />
              </span>{" "}
              <Link
                href={`https://dashboard.workos.com/${environment.workosEnvironmentId}/authentication`}
                target="_blank"
                rel="noopener noreferrer"
              >
                View in WorkOS
              </Link>
            </p>
          </div>

          <div className="flex flex-col gap-1 text-xs">
            {envHealthData && (
              <>
                {envHealthData.data ? (
                  <span className="inline-flex items-center gap-1 text-content-secondary">
                    <CheckIcon className="size-4" /> Convex has access to this
                    environment
                  </span>
                ) : envHealthData.error?.code ===
                  "WorkOSEnvironmentNotFound" ? (
                  <span className="inline-flex items-center gap-1 text-content-error">
                    <Cross2Icon className="size-4" /> Environment not found in
                    WorkOS (may have been deleted)
                  </span>
                ) : envHealthData.error?.code === "WorkOSAPIUnavailable" ? (
                  <span className="inline-flex items-center gap-1 text-content-warning">
                    <ExclamationTriangleIcon className="size-4" /> WorkOS API
                    currently unavailable
                  </span>
                ) : !envHealthData.error ? (
                  <span className="text-content-tertiary">
                    Checking accessibility...
                  </span>
                ) : (
                  <span className="inline-flex items-center gap-1 text-content-warning">
                    <ExclamationTriangleIcon className="size-4" /> Unable to
                    verify environment
                  </span>
                )}
              </>
            )}
            {teamMismatch ? (
              <span className="inline-flex items-center gap-1 text-content-warning">
                <ExclamationTriangleIcon className="size-4" /> Created for a
                different workspace than{" "}
                <span className="font-mono">{workosTeam?.workosTeamName}</span>
              </span>
            ) : workosTeam ? (
              <span className="inline-flex items-center gap-1 text-content-secondary">
                <CheckIcon className="size-4" /> Created by WorkOS workspace
                linked to this Convex team,{" "}
                <span className="font-mono">{workosTeam.workosTeamName}</span>
              </span>
            ) : null}
            {(() => {
              if (!canViewEnv) {
                return (
                  <div className="flex gap-1 text-xs text-content-secondary">
                    Environment variable status unknown{" "}
                    <HelpTooltip>
                      <PermissionDeniedTip
                        message="You must be able to view environment variables to check environment variable configuration"
                        action="deployment:env:view"
                      />
                    </HelpTooltip>
                  </div>
                );
              }

              const changes: EnvVarChange[] = [];

              if (clientIdMissing || clientIdMismatch) {
                changes.push({
                  name: "WORKOS_CLIENT_ID",
                  currentValue: workosEnvVars.clientId,
                  newValue: environment.workosClientId,
                });
              }

              if (environmentIdMissing || environmentIdMismatch) {
                changes.push({
                  name: "WORKOS_ENVIRONMENT_ID",
                  currentValue: workosEnvVars.environmentId,
                  newValue: environment.workosEnvironmentId,
                });
              }

              if (
                environment.workosApiKey &&
                (apiKeyMissing || apiKeyMismatch)
              ) {
                changes.push({
                  name: "WORKOS_API_KEY",
                  currentValue: workosEnvVars.apiKey,
                  newValue: environment.workosApiKey,
                });
              }

              if (changes.length === 0) {
                return (
                  <span className="inline-flex items-center gap-1 text-content-secondary">
                    <CheckIcon className="size-4" />
                    <code>WORKOS_*</code> environment variables match the values
                    for the provisioned environment
                  </span>
                );
              }

              // Some variables need updating
              return (
                <div className="flex flex-col gap-2">
                  <span className="inline-flex items-center gap-1 text-content-warning">
                    <ExclamationTriangleIcon className="size-4" />
                    Environment variables need to be configured
                  </span>

                  <Callout variant="instructions">
                    <div className="flex w-full flex-col gap-3">
                      <div>
                        <p className="mb-2 text-sm font-semibold">
                          Update Deployment Environment Variables
                        </p>
                        <p className="text-xs text-content-secondary">
                          The following environment variables can be updated to
                          match the Convex-provisioned WorkOS environment for
                          this deployment:
                        </p>
                      </div>

                      <div className="flex flex-col gap-2">
                        {changes.map((change) => (
                          <EnvVarChangeRow key={change.name} change={change} />
                        ))}
                      </div>

                      <div className="flex gap-2">
                        <Button
                          size="sm"
                          onClick={handleSetEnvironmentVariables}
                          loading={isSettingEnvVars}
                        >
                          {changes.some((c) => c.currentValue !== null)
                            ? "Update Deployment Environment Variables"
                            : "Set Deployment Environment Variables"}
                        </Button>
                        {envVarsLink && (
                          <Button
                            size="sm"
                            variant="neutral"
                            href={envVarsLink}
                            target="_blank"
                          >
                            Configure Manually
                            <ExternalLinkIcon className="ml-1 size-3" />
                          </Button>
                        )}
                      </div>
                    </div>
                  </Callout>
                </div>
              );
            })()}
          </div>

          {environment &&
            !environment.isProduction &&
            (() => {
              const matchingVars = [
                clientIdMatches && "WORKOS_CLIENT_ID",
                environmentIdMatches && "WORKOS_ENVIRONMENT_ID",
                apiKeyMatches && "WORKOS_API_KEY",
              ].filter(Boolean) as string[];

              return (
                <>
                  {/* Show both buttons or neither */}
                  {!showCredentials && !showDeleteForm && (
                    <div className="flex gap-2">
                      <Button
                        size="sm"
                        variant="neutral"
                        onClick={() => setShowCredentials(true)}
                      >
                        Show Credentials
                      </Button>
                      <Button
                        size="sm"
                        variant="neutral"
                        onClick={() => setShowDeleteForm(true)}
                      >
                        Delete Provisioned Environment
                      </Button>
                    </div>
                  )}

                  {/* Show credentials */}
                  {showCredentials && (
                    <div className="flex flex-col gap-2">
                      <div>
                        <Button
                          size="sm"
                          variant="neutral"
                          onClick={() => setShowCredentials(false)}
                        >
                          Hide Credentials
                        </Button>
                      </div>
                      <WorkOSCredentialsSection
                        clientId={environment.workosClientId}
                        apiKey={environment.workosApiKey}
                        isProduction={environment.isProduction}
                      />
                    </div>
                  )}

                  {/* Show delete form */}
                  {showDeleteForm && (
                    <div className="flex flex-col gap-2 rounded-sm border border-content-error p-3">
                      <div className="flex flex-col gap-1">
                        <div className="text-sm font-semibold text-content-primary">
                          Delete WorkOS AuthKit environment{" "}
                          <span className="font-mono">
                            {environment.workosEnvironmentName}
                          </span>
                        </div>
                        <p className="text-xs text-content-secondary">
                          This will permanently delete this environment from
                          WorkOS and remove it from Convex. This action cannot
                          be undone.
                          {matchingVars.length > 0 && (
                            <>
                              {" "}
                              The following environment variables will also be
                              cleared:{" "}
                              <span className="font-mono">
                                {matchingVars.join(", ")}
                              </span>
                              .
                            </>
                          )}
                        </p>
                      </div>
                      <div className="flex gap-2">
                        <Button
                          size="sm"
                          variant="danger"
                          onClick={async () => {
                            await handleDeleteEnvironment();
                            setShowDeleteForm(false);
                          }}
                          loading={isDeleting}
                        >
                          Delete
                        </Button>
                        <Button
                          size="sm"
                          variant="neutral"
                          onClick={() => setShowDeleteForm(false)}
                        >
                          Cancel
                        </Button>
                      </div>
                    </div>
                  )}
                </>
              );
            })()}
        </div>
      ) : matchingProjectEnvironment ? (
        // Using a shared project-level environment
        <div className="flex flex-col gap-2">
          <p className="text-sm text-content-primary">
            Using shared environment:{" "}
            <span className="font-semibold">
              {matchingProjectEnvironment.userEnvironmentName}
            </span>
          </p>
          <div className="flex flex-col gap-1 text-xs">
            <span className="inline-flex items-center gap-1 text-content-secondary">
              <CheckIcon className="size-4" />
              <code>WORKOS_CLIENT_ID</code> is set to a shared AuthKit
              environment for this project
            </span>
            {workosEnvVars.apiKey ? (
              <span className="inline-flex items-center gap-1 text-content-secondary">
                <CheckIcon className="size-4" />
                <code>WORKOS_API_KEY</code> is set so <code>authKit</code>{" "}
                properties in convex.json will be configured by the Convex CLI
              </span>
            ) : (
              <span className="inline-flex items-center gap-1 text-content-warning">
                <ExclamationTriangleIcon className="size-4" />
                <code>WORKOS_API_KEY</code> is not set - automatic configuration
                via convex.json will not work
              </span>
            )}
          </div>
        </div>
      ) : workosEnvVars.clientId ? (
        <>
          <p className="text-sm text-content-secondary">
            <code className="text-xs font-semibold">WORKOS_CLIENT_ID</code> is
            set to an environment not managed by Convex.
          </p>
          {workosEnvVars.apiKey ? (
            <p className="text-sm text-content-secondary">
              <code className="text-xs font-semibold">WORKOS_API_KEY</code> is
              set so <code>authKit</code> properties like{" "}
              <code>redirectUris</code> in{" "}
              <Link
                href="https://docs.convex.dev/auth/authkit/auto-provision"
                target="_blank"
              >
                convex.json
              </Link>{" "}
              will be configured by the Convex CLI at code push.
            </p>
          ) : null}
        </>
      ) : (
        <p className="text-sm text-content-secondary">
          No environment provisioned
        </p>
      )}

      {/* Not Supported Notice */}
      {notSupportedReason && (
        <Callout variant="instructions">
          <div className="flex flex-col gap-2">
            <p className="text-sm">{notSupportedReason}</p>
            <p className="text-sm">
              You can{" "}
              <Link href="https://docs.convex.dev/auth/authkit" target="_blank">
                configure WorkOS AuthKit manually
              </Link>{" "}
              by setting{" "}
              <Link
                href={`${envVarsLink}?var=WORKOS_CLIENT_ID&var=WORKOS_API_KEY`}
              >
                environment variables
              </Link>
              .
            </p>
          </div>
        </Callout>
      )}

      {/* Actions - only show if not already provisioned and supported */}
      {!environment &&
        !notSupportedReason &&
        hasTeam &&
        (!showCreateForm ? (
          <div>
            <Button
              variant={
                workosEnvVars.clientId
                  ? "neutral" // Not primary when manually configured
                  : deployment?.deploymentType === "preview"
                    ? "neutral"
                    : "primary"
              }
              size="sm"
              onClick={() => setShowCreateForm(true)}
            >
              Create AuthKit Environment
            </Button>
          </div>
        ) : (
          <div className="flex flex-col gap-2 rounded-sm border p-3">
            <div className="flex flex-col gap-1">
              <p className="text-sm font-semibold text-content-primary">
                Create a new WorkOS environment for this deployment
              </p>
              <p className="text-xs text-content-secondary">
                This will also set or replace the{" "}
                <code className="text-xs">WORKOS_*</code> environment variables
                for this deployment.
              </p>
            </div>
            <div className="flex gap-2">
              {isProductionDeployment ? (
                <>
                  <Button
                    size="sm"
                    onClick={() => handleProvisionEnvironment(true)}
                    disabled={!canProvisionProduction}
                    loading={provisioningType === "production"}
                    tip={
                      canProvisionProduction
                        ? undefined
                        : "Add a payment method in WorkOS to create production environments"
                    }
                  >
                    Create Production Environment
                  </Button>
                  <Button
                    size="sm"
                    variant="neutral"
                    onClick={() => handleProvisionEnvironment(false)}
                    loading={provisioningType === "non-production"}
                  >
                    Create Staging Environment
                  </Button>
                </>
              ) : (
                <Button
                  size="sm"
                  onClick={() => handleProvisionEnvironment(false)}
                  loading={provisioningType === "non-production"}
                >
                  Create Environment
                </Button>
              )}
              <Button
                variant="neutral"
                size="sm"
                onClick={() => setShowCreateForm(false)}
                disabled={provisioningType !== null}
              >
                Cancel
              </Button>
            </div>
          </div>
        ))}
    </div>
  );
}

function ModalFooter() {
  return (
    <p className="text-sm text-content-primary">
      <Link
        href="https://docs.convex.dev/auth/authkit/auto-provision"
        target="_blank"
      >
        Learn more about automatic creation of WorkOS environments
      </Link>
    </p>
  );
}

export function WorkOSConfigurationForm() {
  const [showTeamCreatedSuccess, setShowTeamCreatedSuccess] = useState(false);

  const handleTeamCreated = () => {
    setShowTeamCreatedSuccess(true);
  };

  const {
    useCurrentDeployment,
    useCurrentTeam,
    useCurrentProject,
    deploymentsURI,
    workOSOperations,
  } = useContext(DeploymentInfoContext);

  const deployment = useCurrentDeployment();
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const { data: workosData, error: workosError } =
    workOSOperations.useDeploymentWorkOSEnvironment(deployment?.name);
  const teamHealthData = workOSOperations.useWorkOSTeamHealth(
    team?.id?.toString(),
  );
  // Fetch project-level environments to check if env vars match one
  const projectEnvironments = workOSOperations.useProjectWorkOSEnvironments(
    deployment?.projectId,
  ) as ProjectEnvironment[] | undefined;
  const notSupportedReason = getWorkOSNotSupportedReason(deployment);

  const { canViewEnv, workosEnvVars } = useRelevantEnvVars();

  const envVarsLink =
    team && project && deployment
      ? `${deploymentsURI}/settings/environment-variables`
      : undefined;

  if (workosError) {
    const { message } = getErrorInfo(workosError);
    return (
      <div className="flex flex-col gap-4">
        <Callout variant="error">
          <p className="text-xs">
            {message ||
              "Failed to load WorkOS configuration. Please try again."}
          </p>
        </Callout>
        <ModalFooter />
      </div>
    );
  }

  if (!workosData || (canViewEnv && !workosEnvVars)) {
    return (
      <div className="flex h-32 items-center justify-center">
        <Loading />
      </div>
    );
  }

  const { workosTeam, environment } = workosData;
  const resolvedEnvVars: WorkOSEnvVars = workosEnvVars ?? {
    clientId: null,
    environmentId: null,
    apiKey: null,
  };
  const hasTeam = workosTeam !== null && workosTeam !== undefined;
  const teamInfo = teamHealthData?.data?.teamProvisioned
    ? teamHealthData.data.teamInfo
    : null;
  const canProvisionProduction = teamInfo?.productionState === "active";

  // Hide everything if there's no team, no environment, and no config
  const showAnySections =
    hasTeam || environment !== null || resolvedEnvVars.clientId !== null;

  if (!showAnySections) {
    // Only show workspace section
    return (
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-2">
          <div className="text-sm font-semibold text-content-primary">
            WorkOS Workspace for Team
          </div>
          <WorkOSTeamSection
            workosTeam={workosTeam}
            environment={environment ?? null}
            teamId={team?.id}
            showCongratulations={showTeamCreatedSuccess}
            onTeamCreated={handleTeamCreated}
            hasClientIdConfigured={!!resolvedEnvVars.clientId}
          />
        </div>
        <ModalFooter />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      {/* Top section: Configured AuthKit Environment */}
      <div className="flex flex-col gap-2">
        <div className="text-sm font-semibold text-content-primary">
          Configured AuthKit Environment
        </div>

        <ConsolidatedEnvironmentSection
          environment={environment ?? null}
          workosTeam={workosTeam}
          hasTeam={hasTeam}
          notSupportedReason={notSupportedReason}
          envVarsLink={envVarsLink}
          workosEnvVars={resolvedEnvVars}
          canViewEnv={canViewEnv}
          deploymentName={deployment?.name}
          deployment={deployment}
          canProvisionProduction={canProvisionProduction}
          projectEnvironments={projectEnvironments}
        />
      </div>

      {/* Shared environments at the bottom */}
      {deployment?.projectId && (
        <WorkOSProjectEnvironments
          projectId={deployment.projectId}
          deploymentType={deployment?.deploymentType}
          workosClientId={resolvedEnvVars.clientId}
          hasLinkedWorkspace={!!workosTeam}
        />
      )}

      {/* Team workspace at the bottom */}
      <div className="flex flex-col gap-2">
        <div className="text-sm font-semibold text-content-primary">
          WorkOS Workspace for Team
        </div>

        <WorkOSTeamSection
          workosTeam={workosTeam}
          environment={environment ?? null}
          teamId={team?.id}
          showCongratulations={showTeamCreatedSuccess}
          onTeamCreated={handleTeamCreated}
          hasClientIdConfigured={!!resolvedEnvVars.clientId}
        />
      </div>

      <ModalFooter />
    </div>
  );
}
