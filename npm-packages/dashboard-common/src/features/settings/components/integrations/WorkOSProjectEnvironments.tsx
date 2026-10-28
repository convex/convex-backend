import { useState, useContext, useEffect } from "react";
import { Button } from "@ui/Button";
import { Loading } from "@ui/Loading";
import { TrashIcon, ExternalLinkIcon } from "@radix-ui/react-icons";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { toast } from "@common/lib/utils";
import { WorkOSCredentialsSection } from "./WorkOSCredentialsSection";
import { WorkOSEnvironmentInfo } from "./WorkOSEnvironmentInfo";

// Validate environment name following backend rules
function validateEnvironmentName(name: string): string | null {
  if (!name || name.trim().length === 0) {
    return "Environment name is required";
  }
  if (name.length > 64) {
    return "Environment name must be at most 64 characters";
  }
  if (name !== name.trim()) {
    return "Environment name cannot start or end with whitespace";
  }
  // Check if name contains only letters, numbers, spaces, hyphens, and underscores
  if (!/^[\w\s-]+$/.test(name)) {
    return "Environment name can only contain letters, numbers, spaces, hyphens, and underscores";
  }
  return null;
}

type ProjectEnvironment = {
  workosEnvironmentId: string;
  workosEnvironmentName: string;
  workosClientId: string;
  userEnvironmentName: string;
  isProduction: boolean;
};

function EnvironmentListItem({
  environment,
  onDelete,
  onShowCredentials,
  isShowingCredentials,
  apiKey,
  isInUse,
}: {
  environment: ProjectEnvironment;
  onDelete: (clientId: string) => void;
  onShowCredentials: (clientId: string) => void;
  isShowingCredentials: boolean;
  apiKey?: string;
  isInUse: boolean;
}) {
  const [isDeleting, setIsDeleting] = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);

  const handleDelete = async () => {
    setIsDeleting(true);
    try {
      await onDelete(environment.workosClientId);
    } catch (error) {
      console.error("Failed to delete environment:", error);
    } finally {
      setIsDeleting(false);
      setShowDeleteConfirm(false);
    }
  };

  const workosUrl = `https://dashboard.workos.com/${environment.workosEnvironmentId}/authentication`;

  return (
    <>
      <div className="flex items-center justify-between px-4 py-1.5">
        <div className="flex items-center gap-1.5">
          <span className="text-sm font-medium text-content-primary">
            {environment.userEnvironmentName}
            {isInUse && (
              <span className="ml-1.5 text-xs text-content-secondary">
                (in use)
              </span>
            )}
          </span>
          <WorkOSEnvironmentInfo
            environment={{
              workosEnvironmentId: environment.workosEnvironmentId,
              workosEnvironmentName: environment.workosEnvironmentName,
              workosClientId: environment.workosClientId,
              isProduction: environment.isProduction,
            }}
          />
          <a
            href={workosUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="text-content-tertiary hover:text-content-secondary"
          >
            <ExternalLinkIcon className="size-3.5" />
          </a>
        </div>
        <div className="flex items-center gap-1">
          <Button
            size="xs"
            variant="neutral"
            onClick={() => onShowCredentials(environment.workosClientId)}
          >
            {isShowingCredentials ? "Hide" : "Show"} Credentials
          </Button>
          <Button
            size="xs"
            variant="danger"
            onClick={() => setShowDeleteConfirm(!showDeleteConfirm)}
          >
            <TrashIcon className="size-4" />
          </Button>
        </div>
      </div>
      {isShowingCredentials && !showDeleteConfirm && (
        <div className="px-4 pb-2">
          <WorkOSCredentialsSection
            clientId={environment.workosClientId}
            apiKey={apiKey}
            isProduction={false}
          />
        </div>
      )}
      {showDeleteConfirm && (
        <div className="px-4 pb-2">
          <p className="mb-2 text-xs text-content-secondary">
            This shared environment may be being used by other deployments in
            this project. Deleting it will remove the environment from WorkOS
            and delete all data.
          </p>
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="danger"
              onClick={handleDelete}
              loading={isDeleting}
            >
              Delete
            </Button>
            <Button
              size="sm"
              variant="neutral"
              onClick={() => setShowDeleteConfirm(false)}
              disabled={isDeleting}
            >
              Cancel
            </Button>
          </div>
        </div>
      )}
    </>
  );
}

export function WorkOSProjectEnvironments({
  projectId,
  deploymentType,
  workosClientId,
  hasLinkedWorkspace,
}: {
  projectId?: number;
  deploymentType?: string;
  workosClientId?: string | null;
  /** Whether the team has a linked WorkOS workspace. Used to determine if section should be shown. */
  hasLinkedWorkspace: boolean;
}) {
  const { workOSOperations, useCurrentTeam } = useContext(
    DeploymentInfoContext,
  );
  const team = useCurrentTeam();

  // All hooks must be called unconditionally before any early returns
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [newEnvironmentName, setNewEnvironmentName] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [expandedCredentialsClientId, setExpandedCredentialsClientId] =
    useState<string | null>(null);
  const [credentialsCache, setCredentialsCache] = useState<
    Record<string, string>
  >({});

  const environments = workOSOperations.useProjectWorkOSEnvironments(projectId);
  const provisionEnvironment =
    workOSOperations.useProvisionProjectWorkOSEnvironment(projectId);
  const deleteEnvironment =
    workOSOperations.useDeleteProjectWorkOSEnvironment(projectId);

  // Fetch individual environment when showing credentials
  const expandedEnvironment = workOSOperations.useGetProjectWorkOSEnvironment(
    projectId,
    expandedCredentialsClientId || undefined,
  );

  // Store fetched API key in cache
  useEffect(() => {
    if (
      expandedEnvironment?.workosApiKey &&
      expandedCredentialsClientId &&
      !credentialsCache[expandedCredentialsClientId]
    ) {
      setCredentialsCache((prev) => ({
        ...prev,
        [expandedCredentialsClientId]: expandedEnvironment.workosApiKey!,
      }));
    }
  }, [
    expandedEnvironment?.workosApiKey,
    expandedCredentialsClientId,
    credentialsCache,
  ]);

  // Don't render if we don't have a projectId
  if (!projectId) {
    return null;
  }

  const isLoading = environments === undefined;

  const handleCreate = async () => {
    const validationError = validateEnvironmentName(newEnvironmentName);
    if (validationError) {
      toast("error", validationError);
      return;
    }

    // Check if an environment with the same name already exists
    if (
      environments?.some(
        (env) => env.userEnvironmentName === newEnvironmentName,
      )
    ) {
      toast(
        "error",
        `An environment named '${newEnvironmentName}' already exists`,
      );
      return;
    }

    setIsCreating(true);
    try {
      const result = await provisionEnvironment({
        environmentName: newEnvironmentName,
      });
      // Store the API key from creation response
      if (result?.workosClientId && result?.workosApiKey) {
        setCredentialsCache((prev) => ({
          ...prev,
          [result.workosClientId]: result.workosApiKey,
        }));
      }
      setNewEnvironmentName("");
      setShowCreateForm(false);
    } catch (error) {
      console.error("Failed to create environment:", error);
      // Don't toast here - useBBMutation already shows error toast
    } finally {
      setIsCreating(false);
    }
  };

  const handleDelete = async (clientId: string) => {
    try {
      await deleteEnvironment(clientId);
      // Remove from credentials cache
      setCredentialsCache((prev) => {
        const newCache = { ...prev };
        delete newCache[clientId];
        return newCache;
      });
      // Clear expanded if this was the one being shown
      if (expandedCredentialsClientId === clientId) {
        setExpandedCredentialsClientId(null);
      }
      // Don't toast here - useBBMutation already shows success toast
    } catch (error) {
      console.error("Failed to delete environment:", error);
      // Don't toast here - useBBMutation already shows error toast
    }
  };

  const handleShowCredentials = (clientId: string) => {
    setExpandedCredentialsClientId(
      expandedCredentialsClientId === clientId ? null : clientId,
    );
  };

  if (isLoading) {
    return <Loading />;
  }

  const hasEnvironments = environments && environments.length > 0;

  // Hide the entire section if there are no environments AND no linked workspace
  if (!hasEnvironments && !hasLinkedWorkspace) {
    return null;
  }

  return (
    <div id="project-environments" className="flex flex-col gap-2">
      <div>
        <div className="text-sm font-semibold text-content-primary">
          Shared AuthKit Environments
        </div>
        <p className="mt-1 text-xs text-content-secondary">
          Shared AuthKit environments belong to a project rather than any
          specific deployment. They're a good fit for sharing between preview
          deployments.
        </p>
      </div>
      {hasEnvironments && (
        <div className="overflow-hidden rounded-sm border bg-background-secondary">
          {environments.map((env) => (
            <EnvironmentListItem
              key={env.workosClientId}
              environment={env}
              onDelete={handleDelete}
              onShowCredentials={handleShowCredentials}
              isShowingCredentials={
                expandedCredentialsClientId === env.workosClientId
              }
              apiKey={credentialsCache[env.workosClientId]}
              isInUse={workosClientId === env.workosClientId}
            />
          ))}
        </div>
      )}

      {!showCreateForm ? (
        <div>
          <Button
            size="sm"
            variant={deploymentType === "preview" ? "primary" : "neutral"}
            onClick={() => {
              setShowCreateForm(true);
              setNewEnvironmentName("Previews");
            }}
            disabled={!team}
          >
            Create Shared AuthKit Environment
          </Button>
        </div>
      ) : (
        <div className="flex flex-col gap-3 rounded-sm border bg-background-secondary p-4">
          <div className="flex flex-col gap-2">
            <label
              htmlFor="environment-name-input"
              className="text-sm font-semibold text-content-primary"
            >
              Environment Name
            </label>
            <input
              id="environment-name-input"
              type="text"
              value={newEnvironmentName}
              onChange={(e) => setNewEnvironmentName(e.target.value)}
              placeholder="e.g., Preview Deployments, Staging"
              className="w-full rounded-sm border bg-background-primary px-3 py-2 text-sm"
              maxLength={64}
              autoFocus
            />
            {(() => {
              const error = newEnvironmentName
                ? validateEnvironmentName(newEnvironmentName)
                : null;
              if (error) {
                return <p className="text-xs text-content-error">{error}</p>;
              }
              if (
                environments?.some(
                  (env) => env.userEnvironmentName === newEnvironmentName,
                )
              ) {
                return (
                  <p className="text-xs text-content-error">
                    An environment with this name already exists
                  </p>
                );
              }
              return null;
            })()}
          </div>
          <div className="flex gap-2">
            <Button
              size="sm"
              onClick={handleCreate}
              loading={isCreating}
              disabled={
                !newEnvironmentName.trim() ||
                !!validateEnvironmentName(newEnvironmentName) ||
                environments?.some(
                  (env) => env.userEnvironmentName === newEnvironmentName,
                )
              }
            >
              Create
            </Button>
            <Button
              size="sm"
              variant="neutral"
              onClick={() => {
                setShowCreateForm(false);
                setNewEnvironmentName("");
              }}
            >
              Cancel
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
