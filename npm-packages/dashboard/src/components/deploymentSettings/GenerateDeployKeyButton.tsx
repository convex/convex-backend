import { Button } from "@ui/Button";
import { Checkbox } from "@ui/Checkbox";
import { TextInput } from "@ui/TextInput";
import { CopyButton } from "@common/elements/CopyButton";
import { CopyTextButton } from "@common/elements/CopyTextButton";
import { ClosePanelButton } from "@ui/ClosePanelButton";
import { Callout } from "@ui/Callout";
import { Modal } from "@ui/Modal";
import {
  Dialog,
  DialogPanel,
  DialogTitle,
  Transition,
  TransitionChild,
} from "@headlessui/react";
import { useCallback, useState } from "react";
import { ExclamationTriangleIcon, PlusIcon } from "@radix-ui/react-icons";
import { DeploymentType as DeploymentTypeType } from "generatedApi";
import { PlatformCreateDeployKeyArgs } from "@convex-dev/platform/managementApi";
import { usePostHog } from "hooks/usePostHog";
import { HelpTooltip } from "@ui/HelpTooltip";
import {
  TokenExpirationSelector,
  TokenExpirationValue,
  resolveExpirationTime,
} from "components/TokenExpirationSelector";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { Link } from "@ui/Link";

export type DeployKeyGenerationDisabledReason =
  | "CannotManageDeployment"
  | "LocalDeployment"
  | "NoPermissionForPreview";

// The set of actions a deploy key can be scoped to, mirrored from the
// management API's `allowedActions` enum. Actions are rendered to users by
// their canonical name (e.g. `deployment:data:view`) rather than a prettified
// label.
export type DeployKeyAction = NonNullable<
  PlatformCreateDeployKeyArgs["allowedActions"]
>[number];

type ActionGroup = {
  label: string;
  actions: { key: DeployKeyAction; description: string }[];
};

export const ACTION_GROUPS: ActionGroup[] = [
  {
    label: "Deployment",
    actions: [
      {
        key: "deployment:deploy",
        description:
          "Allows deploying to this deployment. This includes updating code, the database schema, and auth configuration.",
      },
      {
        key: "deployment:pause",
        description:
          "Allows pausing this deployment, blocking all functions from running, including scheduled functions and Cron jobs.",
      },
      {
        key: "deployment:unpause",
        description:
          "Allows unpausing this deployment, re-enabling functions, scheduled functions, and Cron jobs.",
      },
    ],
  },
  {
    label: "Environment variables",
    actions: [
      {
        key: "deployment:env:view",
        description:
          "Allows viewing all environment variables configured for this deployment.",
      },
      {
        key: "deployment:env:write",
        description:
          "Allows creating, updating, and deleting all environment variables in this deployment.",
      },
    ],
  },
  {
    label: "Data",
    actions: [
      {
        key: "deployment:data:view",
        description:
          "Allows viewing all data stored in this deployment, including data in tables, the database schema, scheduled functions, and file storage.",
      },
      {
        key: "deployment:data:write",
        description:
          "Allows writing to all data in this deployment, including updating data in tables, uploading and deleting files, canceling scheduled jobs, and exporting data with streaming export.",
      },
    ],
  },
  {
    label: "Functions",
    actions: [
      {
        key: "deployment:functions:runInternalQueries",
        description:
          "Allows running internal queries defined in this deployment.",
      },
      {
        key: "deployment:functions:runInternalMutations",
        description:
          "Allows running internal mutations defined in this deployment.",
      },
      {
        key: "deployment:functions:runInternalActions",
        description:
          "Allows running internal actions defined in this deployment.",
      },
      {
        key: "deployment:functions:runTestQuery",
        description:
          "Allows running custom test queries against this deployment.",
      },
      {
        key: "deployment:functions:actAsUser",
        description:
          "Allows running functions assuming a specific user identity.",
      },
    ],
  },
  {
    label: "Monitoring",
    actions: [
      {
        key: "deployment:logs:view",
        description: "Allows viewing function execution logs.",
      },
      {
        key: "deployment:metrics:view",
        description: "Allows viewing application metrics.",
      },
      {
        key: "deployment:auditLog:view",
        description:
          "Allows viewing the deployment audit log, visible on the dashboard's history page.",
      },
    ],
  },
  {
    label: "Usage",
    actions: [
      {
        key: "deployment:usage:view",
        description: "Allows viewing usage metrics for this deployment.",
      },
      {
        key: "deployment:usageLimits:view",
        description:
          "Allows viewing the usage limits configured for this deployment.",
      },
      {
        key: "deployment:usageLimits:write",
        description:
          "Allows creating, updating, and deleting usage limits for this deployment.",
      },
    ],
  },
  {
    label: "Integrations",
    actions: [
      {
        key: "deployment:integrations:view",
        description:
          "Allows viewing integration configured for this deployment.",
      },
      {
        key: "deployment:integrations:write",
        description: "Allows configuring integrations for this deployment.",
      },
    ],
  },
  {
    label: "Backups",
    actions: [
      {
        key: "deployment:backups:view",
        description: "Not yet implemented.",
      },
      {
        key: "deployment:backups:create",
        description:
          "Allows exporting data with the Convex CLI. In the future, will also allow deploy keys to create cloud backups.",
      },
      {
        key: "deployment:backups:download",
        description: "Allows downloading previously generated backups.",
      },
      {
        key: "deployment:backups:delete",
        description: "Not yet implemented.",
      },
      {
        key: "deployment:backups:import",
        description:
          "Allows importing data with the Convex CLI and Streaming Import. In the future, will also allow deploy keys to restore from a cloud backup.",
      },
    ],
  },
];

export type CreateDeployKeyFormProps = {
  disabledReason: DeployKeyGenerationDisabledReason | null;
  getAdminKey: (
    name: string,
    allowedActions: DeployKeyAction[] | undefined,
    expiresAt: number | undefined,
  ) => Promise<{ ok: true; adminKey: string } | { ok: false; error: string }>;
  deploymentType: DeploymentTypeType;
  showCustomPermissions?: boolean;
};

export function CreateDeployKeyForm({
  disabledReason,
  getAdminKey,
  deploymentType,
  showCustomPermissions = true,
  onClose,
}: CreateDeployKeyFormProps & { onClose: () => void }) {
  const [open, setOpen] = useState(true);
  const closePanel = useCallback(() => setOpen(false), []);
  const [isLoading, setIsLoading] = useState(false);
  const [name, setName] = useState("");
  const [selectedActions, setSelectedActions] = useState<Set<DeployKeyAction>>(
    () => new Set(),
  );
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [panelClosed, setPanelClosed] = useState(false);
  const [expiration, setExpiration] = useState<TokenExpirationValue>(null);
  const [error, setError] = useState<string | null>(null);
  const { capture } = usePostHog();

  return (
    <>
      <Transition
        show={open}
        appear
        afterLeave={() => {
          if (createdKey === null) {
            onClose();
          } else {
            setPanelClosed(true);
          }
        }}
      >
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
              <div className="absolute inset-0 transition-opacity" />
            </TransitionChild>

            <div className="fixed inset-0 flex items-center justify-center p-4 md:inset-y-0 md:right-0 md:left-auto md:items-stretch md:justify-end md:p-0 md:pl-10">
              <TransitionChild
                enter="transform transition ease-in-out duration-200 md:duration-300"
                enterFrom="translate-y-2 opacity-0 md:translate-x-full md:translate-y-0"
                enterTo="translate-y-0 opacity-100 md:translate-x-0"
                leave="transform transition ease-in-out duration-200 md:duration-300"
                leaveFrom="translate-y-0 opacity-100 md:translate-x-0"
                leaveTo="translate-y-2 opacity-0 md:translate-x-full md:translate-y-0"
              >
                <DialogPanel className="w-full max-w-lg md:w-screen md:max-w-3xl">
                  <div
                    data-testid="create-deploy-key-panel"
                    className="flex max-h-[85vh] flex-col rounded-lg bg-background-secondary shadow-xl md:h-full md:max-h-none md:rounded-none dark:border"
                  >
                    <div className="flex items-center justify-between px-6 pt-6 pb-4">
                      <DialogTitle as="h4">Create Deploy Key</DialogTitle>
                      <ClosePanelButton onClose={closePanel} />
                    </div>

                    <form
                      className="flex min-h-0 flex-1 flex-col"
                      onSubmit={async (e) => {
                        e.preventDefault();
                        setIsLoading(true);
                        setError(null);
                        try {
                          const allowedActions = showCustomPermissions
                            ? Array.from(selectedActions)
                            : undefined;
                          const expiresAt = resolveExpirationTime(expiration);
                          const result = await getAdminKey(
                            name,
                            allowedActions,
                            expiresAt ?? undefined,
                          );
                          if (!result.ok) {
                            setError(result.error);
                            return;
                          }
                          setCreatedKey(result.adminKey);
                          closePanel();
                          capture("generated_deploy_key", {
                            type: deploymentType,
                          });
                        } finally {
                          setIsLoading(false);
                        }
                      }}
                    >
                      {/* `--scroll-fade-height` sizes the sticky fade gradient
                        at the bottom of this scroll area (see the trailing
                        sticky div below). */}
                      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-6 [--scroll-fade-height:3rem]">
                        <TextInput
                          label="Name"
                          id="name"
                          autoFocus
                          value={name}
                          placeholder="Enter a memorable name for your deploy key"
                          onChange={(event) => {
                            setName(event.target.value);
                          }}
                        />
                        <TokenExpirationSelector
                          value={expiration}
                          onChange={setExpiration}
                        />
                        {showCustomPermissions && (
                          <div className="mt-2 flex flex-col gap-3">
                            <p className="text-xs text-content-secondary">
                              Select the permissions this key needs.{" "}
                              <Link
                                href="https://docs.convex.dev/team-management/role-actions#data-plane-and-runtime"
                                target="_blank"
                              >
                                Learn more about permissions.
                              </Link>
                            </p>
                            <div className="flex items-center gap-1">
                              <Button
                                variant="neutral"
                                size="xs"
                                onClick={() => {
                                  const all = new Set(
                                    ACTION_GROUPS.flatMap((g) =>
                                      g.actions.map((a) => a.key),
                                    ),
                                  );
                                  setSelectedActions(all);
                                }}
                              >
                                Select all
                              </Button>
                              <Button
                                variant="neutral"
                                size="xs"
                                onClick={() => {
                                  setSelectedActions(new Set());
                                }}
                              >
                                Select none
                              </Button>
                            </div>
                            <div className="columns-1 gap-x-6 md:columns-2">
                              {ACTION_GROUPS.map((group) => (
                                <div
                                  key={group.label}
                                  className="mb-3 break-inside-avoid"
                                >
                                  <div className="mb-1 text-sm font-semibold text-content-secondary">
                                    {group.label}
                                  </div>
                                  <div className="flex flex-col gap-y-1">
                                    {group.actions.map((action) => (
                                      <label
                                        key={action.key}
                                        htmlFor={`action-${action.key}`}
                                        className="flex cursor-pointer items-center gap-2 rounded-sm p-1 text-xs hover:bg-background-secondary"
                                      >
                                        <Checkbox
                                          id={`action-${action.key}`}
                                          checked={selectedActions.has(
                                            action.key,
                                          )}
                                          onChange={() => {
                                            setSelectedActions((prev) => {
                                              const next = new Set(prev);
                                              if (next.has(action.key)) {
                                                next.delete(action.key);
                                              } else {
                                                next.add(action.key);
                                              }
                                              return next;
                                            });
                                          }}
                                        />
                                        <span className="font-mono">
                                          {action.key}
                                        </span>
                                        <HelpTooltip>
                                          {action.description}
                                        </HelpTooltip>
                                      </label>
                                    ))}
                                  </div>
                                </div>
                              ))}
                            </div>
                          </div>
                        )}
                        {error !== null && (
                          <Callout
                            variant="error"
                            className="text-xs wrap-break-word"
                          >
                            <ExclamationTriangleIcon className="mt-0.5 mr-1 min-w-4" />
                            {error}
                          </Callout>
                        )}
                        {/* Sticky fade that pins to the bottom of the scroll
                            area to hint there's more to scroll. As a sticky
                            child it lives in the content box (left of the
                            scrollbar, so it never covers it). */}
                        <div
                          aria-hidden
                          className="pointer-events-none sticky bottom-0 h-(--scroll-fade-height) shrink-0 bg-linear-to-b from-transparent to-background-secondary"
                        />
                      </div>
                      <div className="flex items-center justify-end gap-2 px-6 py-4">
                        {showCustomPermissions &&
                          selectedActions.size === 0 && (
                            <span className="text-xs text-content-errorSecondary">
                              Select at least one action
                            </span>
                          )}
                        <Button
                          variant="neutral"
                          onClick={closePanel}
                          disabled={isLoading}
                        >
                          Cancel
                        </Button>
                        <Button
                          className="w-fit"
                          type="submit"
                          disabled={
                            disabledReason !== null ||
                            name.trim() === "" ||
                            (showCustomPermissions &&
                              selectedActions.size === 0)
                          }
                          loading={isLoading}
                        >
                          Create
                        </Button>
                      </div>
                    </form>
                  </div>
                </DialogPanel>
              </TransitionChild>
            </div>
          </div>
        </Dialog>
      </Transition>
      {createdKey !== null && panelClosed && (
        <Modal
          title="Deploy Key Created"
          onClose={onClose}
          requireExplicitClose
        >
          <div className="flex flex-col gap-4">
            <p className="text-sm text-content-primary">
              Copy your new deploy key now. You won&apos;t be able to see it
              again.
            </p>
            <div className="flex items-center gap-2">
              <code className="min-w-0 flex-1 truncate rounded-sm bg-background-tertiary px-2 py-1 text-sm">
                {createdKey}
              </code>
              <CopyButton text={createdKey} />
            </div>
            <div className="flex justify-end">
              <Button onClick={onClose}>Done</Button>
            </div>
          </div>
        </Modal>
      )}
    </>
  );
}

type GenerateDeployKeyButtonProps = {
  deploymentType: DeploymentTypeType;
  getAdminKey: () => Promise<{ ok: true; adminKey: string } | { ok: false }>;
  disabledReason: DeployKeyGenerationDisabledReason | null;
};

export function GenerateDeployKeyButton({
  deploymentType,
  getAdminKey,
  disabledReason,
}: GenerateDeployKeyButtonProps) {
  const [isLoading, setIsLoading] = useState(false);

  const [deployKey, setDeployKey] = useState<string | null>(null);

  return (
    <>
      {deployKey ? (
        <div className="flex max-w-lg flex-col gap-3">
          <Callout variant="instructions">
            This key enables reading and writing data to your deployment without
            needing to log in, so it should not be shared or committed to git.
          </Callout>
          <CopyTextButton
            text={deployKey}
            className="block max-w-120 truncate font-mono text-sm font-normal"
          />
        </div>
      ) : (
        <Button
          disabled={disabledReason !== null}
          tip={
            disabledReason === null
              ? undefined
              : DEPLOY_KEY_GENERATION_DISABLED_REASONS[disabledReason]
          }
          onClick={async () => {
            setIsLoading(true);
            try {
              if (deployKey === null) {
                const result = await getAdminKey();
                if (!result.ok) {
                  return;
                }
                setDeployKey(result.adminKey);
              }
            } finally {
              setIsLoading(false);
            }
          }}
          className="my-2 mr-auto"
          loading={isLoading}
          icon={<PlusIcon />}
        >
          {getGenerateButtonText(deploymentType)}
        </Button>
      )}
    </>
  );
}

// Map of disabled-reason → tooltip body. Permission-driven reasons use
// `permissionDeniedTip` so custom-role members see the specific missing
// action surfaced inline.
export const DEPLOY_KEY_GENERATION_DISABLED_REASONS: Record<
  DeployKeyGenerationDisabledReason,
  React.ReactNode
> = {
  CannotManageDeployment: permissionDeniedTip(
    "You do not have permission to generate a deploy key for this deployment.",
    "deployment:token:create",
  ),
  LocalDeployment: "You cannot generate deploy keys for a local deployment.",
  NoPermissionForPreview: permissionDeniedTip(
    "You do not have permission to generate preview deploy keys for this project.",
    "project:token:create",
  ),
};

export function getGenerateButtonText(_deploymentType: DeploymentTypeType) {
  return "Create Deploy Key";
}
