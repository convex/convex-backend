import React, { useState } from "react";
import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { TextInput } from "@ui/TextInput";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { OauthAppResponse } from "generatedApi";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { OAUTH_APPLICATION_RESOURCE } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import {
  PlusIcon,
  EyeNoneIcon,
  EyeOpenIcon,
  DotsVerticalIcon,
  QuestionMarkCircledIcon,
  PersonIcon,
} from "@radix-ui/react-icons";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import {
  useTeamOauthApps,
  useUpdateOauthApp,
  useRegisterOauthApp,
  useDeleteOauthApp,
  useRegenerateOauthClientSecret,
} from "api/oauth";
import { Modal } from "@ui/Modal";
import { Formik } from "formik";
import * as Yup from "yup";
import { Loading, LoadingTransition } from "@ui/Loading";
import { CopyTextButton } from "dashboard-common/src/elements/CopyTextButton";
import { Tooltip } from "@ui/Tooltip";
import { HelpTooltip } from "@ui/HelpTooltip";
import { Menu, MenuItem } from "@ui/Menu";
import { cn } from "@ui/cn";
import { toast } from "@common/lib/utils";

import { captureException, captureMessage } from "@sentry/nextjs";
import { useProfile } from "api/profile";
import { Link } from "@ui/Link";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { formatNumberCompact } from "@common/lib/format";

// Utility function to validate URLs without side effects
function isValidOauthRedirectUri(uri: string): boolean {
  try {
    const url = new URL(uri);
    // Only allow http and https
    if (!["http:", "https:"].includes(url.protocol)) return false;
    // Disallow fragments
    if (url.hash && url.hash !== "") return false;
    // Must have a hostname
    if (!url.hostname) return false;
    // Allow localhost and private IPs (no extra check needed)
    return true;
  } catch {
    return false;
  }
}

// Shared validation schema for both create and edit
const OAUTH_APP_SCHEMA = Yup.object({
  appName: Yup.string()
    .min(3, "App name must be at least 3 characters")
    .max(128, "App name must be at most 128 characters")
    .required("App name is required"),
  redirectUris: Yup.string()
    .required("At least one redirect URI is required")
    .test(
      "uris",
      "Must provide between 1 and 20 comma-delimited URIs",
      (value) => {
        if (!value) return false;
        const uris = value
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean);
        return uris.length >= 1 && uris.length <= 20;
      },
    )
    .test("valid-urls", function validateUrls(value: string | undefined) {
      if (!value)
        return this.createError({
          message: "At least one redirect URI is required",
        });
      const uris = value
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      for (const uri of uris) {
        if (!isValidOauthRedirectUri(uri)) {
          return this.createError({
            message: `Redirect URI is not valid: ${uri}`,
          });
        }
      }
      return true;
    }),
});

// Validation schema for verification request form
const VERIFICATION_REQUEST_SCHEMA = Yup.object({
  description: Yup.string()
    .min(10, "Description must be at least 10 characters")
    .max(2000, "Description must be at most 2000 characters")
    .required("Description is required"),
  appUrl: Yup.string()
    .url("Must be a valid URL (including https://)")
    .max(2000, "URL must be at most 2000 characters")
    .required(
      "A link where the Convex team can test your application is required",
    ),
});

// Handler for verification request submission
async function handleVerificationRequest(
  values: { description: string; appUrl: string },
  app: OauthAppResponse,
  teamId: number,
  setVerificationError: (error: string) => void,
  setVerificationLoading: (loading: boolean) => void,
  setVerificationModalOpen: (open: boolean) => void,
): Promise<void> {
  setVerificationError("");
  setVerificationLoading(true);

  try {
    const body = JSON.stringify({
      subject: `OAuth App Verification Request: ${app.appName}`,
      message: `OAuth App Verification Request

Team ID: ${teamId}
Client ID: ${app.clientId}
App Name: ${app.appName}
Application URL (for testing): ${values.appUrl}

Description:
${values.description}

Please review this OAuth application for verification.`,
      teamId,
    });

    const resp = await fetch("/api/contact-form", {
      method: "POST",
      body,
      headers: {
        "Content-Type": "application/json",
      },
    });

    if (!resp.ok) {
      try {
        if (resp.status < 500 || resp.status >= 400) {
          const { error } = await resp.json();
          captureMessage(error, "error");
        }
      } catch (e) {
        captureException(e);
      }

      toast(
        "error",
        "Failed to send verification request. Please try again or email us at support@convex.dev",
      );
      return;
    }

    setVerificationModalOpen(false);
    toast("success", "Verification request sent!");
  } catch (err: any) {
    setVerificationError(err?.message || "Failed to send verification request");
  } finally {
    setVerificationLoading(false);
  }
}

// Move OauthAppForm to the top of the file so it is in scope for all usages
function OauthAppForm({
  initialValues,
  validationSchema,
  onSubmit,
  submitLabel,
  loading,
  error,
  isVerified,
}: {
  initialValues: { appName: string; redirectUris: string };
  validationSchema: any;
  onSubmit: (
    values: { appName: string; redirectUris: string },
    helpers: {
      setSubmitting: (isSubmitting: boolean) => void;
      resetForm?: () => void;
    },
  ) => Promise<void>;
  submitLabel: string;
  loading: boolean;
  error?: string;
  isVerified: boolean;
}) {
  return (
    <Formik
      initialValues={initialValues}
      validationSchema={validationSchema}
      onSubmit={onSubmit}
      enableReinitialize
    >
      {({
        values,
        errors,
        touched,
        handleChange,
        handleSubmit,
        isSubmitting,
      }) => (
        <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
          <TextInput
            id="app-name"
            name="appName"
            label="Application Name"
            disabled={isVerified}
            placeholder="My OAuth App"
            value={values.appName}
            onChange={handleChange}
            error={
              touched.appName && typeof errors.appName === "string"
                ? errors.appName
                : undefined
            }
            required
          />
          <label
            htmlFor="redirect-uris"
            className="flex flex-col gap-1 text-sm text-content-primary"
          >
            Redirect URIs (seperated by commas)
            <textarea
              id="redirect-uris"
              name="redirectUris"
              className="h-24 resize-y rounded-sm border bg-background-secondary px-4 py-2 text-content-primary placeholder:text-content-tertiary focus:border-border-selected focus:outline-hidden"
              value={values.redirectUris}
              onChange={handleChange}
              placeholder="https://example.com/callback, http://localhost:1337/callback"
              required
            />
            {touched.redirectUris &&
              typeof errors.redirectUris === "string" && (
                <p className="text-xs text-content-errorSecondary" role="alert">
                  {errors.redirectUris}
                </p>
              )}
          </label>
          <div className="mt-2 flex items-center justify-between">
            {error && (
              <span className="mr-2 text-xs text-content-errorSecondary">
                {error}
              </span>
            )}
            <Button
              type="submit"
              className="ml-auto"
              loading={isSubmitting || loading}
            >
              {submitLabel}
            </Button>
          </div>
        </form>
      )}
    </Formik>
  );
}

// Verification request form component
function VerificationRequestForm({
  app,
  onSubmit,
  loading,
  error,
}: {
  app: OauthAppResponse;
  onSubmit: (values: { description: string; appUrl: string }) => Promise<void>;
  loading: boolean;
  error?: string;
}) {
  const profile = useProfile();
  const userEmail = profile?.email;

  if (!userEmail) {
    return <Loading />;
  }

  return (
    <Formik
      initialValues={{ description: "", appUrl: "" }}
      validationSchema={VERIFICATION_REQUEST_SCHEMA}
      onSubmit={async (values, { setSubmitting }) => {
        await onSubmit(values);
        setSubmitting(false);
      }}
    >
      {({
        values,
        errors,
        touched,
        handleChange,
        handleSubmit,
        isSubmitting,
      }) => (
        <form className="flex flex-col gap-2" onSubmit={handleSubmit}>
          <div className="space-y-2 text-sm">
            <div>
              <span>Application Name:</span>
              <span className="ml-1 font-semibold text-content-primary">
                {app.appName}
              </span>
              <span className="ml-2 text-content-tertiary">
                (cannot be changed after verification)
              </span>
            </div>
          </div>

          <label
            htmlFor="description"
            className="flex flex-col gap-1 text-sm text-content-primary"
          >
            Application Description
            <textarea
              id="description"
              name="description"
              className="h-48 resize-y rounded-sm border bg-background-secondary px-4 py-2 text-content-primary placeholder:text-content-tertiary focus:border-border-selected focus:outline-hidden"
              value={values.description}
              onChange={handleChange}
              placeholder="Tell us a bit about your application."
              required
            />
            {touched.description && typeof errors.description === "string" && (
              <p className="text-xs text-content-errorSecondary" role="alert">
                {errors.description}
              </p>
            )}
          </label>

          <TextInput
            id="app-url"
            name="appUrl"
            label="Application URL"
            description="A link where the Convex team can access and test your application. A working application is required for verification."
            placeholder="https://example.com"
            value={values.appUrl}
            onChange={handleChange}
            error={
              touched.appUrl && typeof errors.appUrl === "string"
                ? errors.appUrl
                : undefined
            }
            required
          />

          <div className="mt-4 rounded-sm border bg-blue-50 p-3 dark:bg-blue-900/20">
            <p className="text-sm text-content-secondary">
              <strong>Note:</strong> The Convex team will review your request
              and respond to you at{" "}
              <span className="font-mono text-content-primary">
                {userEmail || "your email"}
              </span>
              .
            </p>
          </div>

          <div className="mt-2 flex items-center justify-between">
            {error && (
              <span className="mr-2 text-xs text-content-errorSecondary">
                {error}
              </span>
            )}
            <Button
              type="submit"
              className="ml-auto"
              loading={isSubmitting || loading}
            >
              Send Request
            </Button>
          </div>
        </form>
      )}
    </Formik>
  );
}

// Shows how many distinct teams have authorized the app. For unverified apps,
// it shows progress toward the connected-teams limit and warns as it fills up.
function AuthorizedTeamCount({ app }: { app: OauthAppResponse }) {
  const count = app.authorizedTeamCount;
  const limit = app.unverifiedTeamLimit;
  const atLimit = !app.verified && count >= limit;
  const nearLimit = !app.verified && !atLimit && count >= limit * 0.8;

  return (
    <Tooltip
      tip={
        app.verified
          ? `${count} ${count === 1 ? "team has" : "teams have"} authorized this application.`
          : `${count} of ${limit} teams have authorized this application. Unverified applications can be authorized by up to ${limit} teams. Verify your application to remove this limit.`
      }
      side="right"
    >
      <div
        className={cn(
          "flex items-center gap-1 rounded-sm border p-1 text-xs text-content-secondary",
          atLimit && "bg-background-errorSecondary text-content-error",
          nearLimit &&
            "border-yellow-700 bg-yellow-100/50 text-yellow-700 dark:border-yellow-200 dark:bg-yellow-900/50 dark:text-yellow-200",
        )}
      >
        <PersonIcon />
        {app.verified
          ? `${formatNumberCompact(count)} ${count === 1 ? "team" : "teams"}`
          : `${formatNumberCompact(count)} / ${formatNumberCompact(limit)} teams`}
      </div>
    </Tooltip>
  );
}

export function OauthApps({ teamId }: { teamId: number }) {
  const canView = useHasCustomRolePermission(
    teamId,
    "oauthApplication:view",
    OAUTH_APPLICATION_RESOURCE,
    true,
  );
  const isAdmin = useIsCurrentMemberTeamAdmin();
  // OAuth-app mutations are admin-only by default; custom roles need an
  // explicit grant per action.
  const canCreateCustom = useHasCustomRolePermission(
    teamId,
    "oauthApplication:create",
    OAUTH_APPLICATION_RESOURCE,
    false,
  );
  const canUpdateCustom = useHasCustomRolePermission(
    teamId,
    "oauthApplication:update",
    OAUTH_APPLICATION_RESOURCE,
    false,
  );
  const canDeleteCustom = useHasCustomRolePermission(
    teamId,
    "oauthApplication:delete",
    OAUTH_APPLICATION_RESOURCE,
    false,
  );
  const canRegenerateSecretCustom = useHasCustomRolePermission(
    teamId,
    "oauthApplication:generateClientSecret",
    OAUTH_APPLICATION_RESOURCE,
    false,
  );
  const canCreate = isAdmin || canCreateCustom === true;
  const canUpdate = isAdmin || canUpdateCustom === true;
  const canDelete = isAdmin || canDeleteCustom === true;
  const canRegenerateSecret = isAdmin || canRegenerateSecretCustom === true;

  const { data: oauthApps, isLoading } = useTeamOauthApps(teamId);
  const registerOauthApp = useRegisterOauthApp(teamId);
  const [createModalOpen, setCreateModalOpen] = useState(false);
  const [registerError, setRegisterError] = useState("");

  if (canView === false) {
    return (
      <Sheet className="max-w-fit">
        <NoPermissionMessage
          message="You do not have permission to view OAuth applications."
          missingPermission="oauthApplication:view"
        />
      </Sheet>
    );
  }

  return (
    <Sheet>
      {createModalOpen && (
        <Modal
          onClose={() => setCreateModalOpen(false)}
          title="Register a new OAuth App"
        >
          <OauthAppForm
            initialValues={{ appName: "", redirectUris: "" }}
            validationSchema={OAUTH_APP_SCHEMA}
            isVerified={false}
            onSubmit={async (
              values: { appName: string; redirectUris: string },
              {
                setSubmitting,
                resetForm,
              }: {
                setSubmitting: (isSubmitting: boolean) => void;
                resetForm?: () => void;
              },
            ) => {
              setRegisterError("");
              setSubmitting(true);
              try {
                const redirectUris = values.redirectUris
                  .split(",")
                  .map((s) => s.trim())
                  .filter(Boolean);
                await registerOauthApp({
                  appName: values.appName,
                  redirectUris,
                });
                if (resetForm) resetForm();
                setCreateModalOpen(false);
              } catch (err: any) {
                setRegisterError(err?.message || "Failed to register app");
              } finally {
                setSubmitting(false);
              }
            }}
            submitLabel="Save"
            loading={false}
            error={registerError}
          />
        </Modal>
      )}
      <LoadingTransition loadingProps={{ className: "h-80 w-full" }}>
        {isLoading ? null : oauthApps && oauthApps.length ? (
          <div className="flex flex-col gap-4">
            <div className="mb-4 flex flex-wrap items-center justify-between gap-4">
              <p className="flex flex-col gap-1 text-sm">
                These are the OAuth applications registered by your team.
                <Link
                  href="https://docs.convex.dev/platform-apis/oauth-applications"
                  target="_blank"
                  rel="noopener noreferrer"
                  externalIcon
                >
                  Learn more about OAuth applications
                </Link>
              </p>
              <Button
                size="xs"
                icon={<PlusIcon />}
                disabled={!canCreate}
                tip={
                  !canCreate
                    ? permissionDeniedTip(
                        "You do not have permission to create OAuth apps.",
                        "oauthApplication:create",
                      )
                    : undefined
                }
                onClick={() => setCreateModalOpen(true)}
              >
                Create Application
              </Button>
            </div>
            <div className="flex w-full flex-col gap-4">
              {oauthApps.map((app: OauthAppResponse) => (
                <OauthAppListItem
                  key={app.clientId}
                  app={app}
                  canUpdate={canUpdate}
                  canDelete={canDelete}
                  canRegenerateSecret={canRegenerateSecret}
                  oauthAppSchema={OAUTH_APP_SCHEMA}
                  teamId={teamId}
                />
              ))}
            </div>
          </div>
        ) : (
          <div className="flex w-full flex-col items-center gap-4">
            <div className="text-center">
              <h3 className="mb-2 font-semibold text-content-primary">
                No OAuth Applications
              </h3>
              <p className="mb-4 max-w-md text-sm text-content-secondary">
                OAuth applications allow you to create and connect to Convex
                deployments owned by other teams.
              </p>
              <p className="mb-4 max-w-md text-sm text-content-secondary">
                This page is for developers who want to create Convex
                integrations.
              </p>
              <div className="mb-2 flex w-full flex-col items-center space-y-2 text-sm text-content-tertiary">
                <p>
                  <Link
                    href="https://docs.convex.dev/platform-apis/oauth-applications"
                    target="_blank"
                    rel="noopener noreferrer"
                    externalIcon
                  >
                    Learn more about OAuth applications
                  </Link>
                </p>
                <p>
                  <Link
                    href="https://docs.convex.dev/platform-apis"
                    target="_blank"
                    rel="noopener noreferrer"
                    externalIcon
                  >
                    Learn more about Convex Platform APIs
                  </Link>
                </p>
              </div>
            </div>
            <Button
              size="sm"
              icon={<PlusIcon />}
              onClick={() => setCreateModalOpen(true)}
              disabled={!canCreate}
              tip={
                !canCreate
                  ? permissionDeniedTip(
                      "You do not have permission to create OAuth apps.",
                      "oauthApplication:create",
                    )
                  : undefined
              }
            >
              Create Your First Application
            </Button>
          </div>
        )}
      </LoadingTransition>
    </Sheet>
  );
}

function OauthAppListItem({
  app,
  canUpdate,
  canDelete,
  canRegenerateSecret,
  oauthAppSchema,
  teamId,
}: {
  app: OauthAppResponse;
  canUpdate: boolean;
  canDelete: boolean;
  canRegenerateSecret: boolean;
  oauthAppSchema: any;
  teamId: number;
}) {
  // Local state for edit modal
  const [editModalOpen, setEditModalOpen] = useState(false);
  const [editName, setEditName] = useState(app.appName);
  const [editUris, setEditUris] = useState(app.redirectUris.join(", "));
  const [editError, setEditError] = useState("");
  const [editLoading, setEditLoading] = useState(false);
  const updateOauthApp = useUpdateOauthApp(teamId, app.clientId);
  const deleteOauthApp = useDeleteOauthApp(teamId, app.clientId);
  const regenerateOauthClientSecret = useRegenerateOauthClientSecret(
    teamId,
    app.clientId,
  );
  // Local state for delete confirmation
  const [showDelete, setShowDelete] = useState(false);
  const [deleteError, setDeleteError] = useState("");

  const [secretVisible, setSecretVisible] = useState(false);

  // Local state for verification request modal
  const [verificationModalOpen, setVerificationModalOpen] = useState(false);
  const [verificationError, setVerificationError] = useState("");
  const [verificationLoading, setVerificationLoading] = useState(false);

  // Local state for regenerate secret confirmation
  const [showRegenerateSecret, setShowRegenerateSecret] = useState(false);
  const [regenerateSecretError, setRegenerateSecretError] = useState("");

  return (
    <div className="scrollbar flex w-full flex-col gap-2 overflow-x-auto rounded-sm border bg-background-secondary p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-3">
          <h4>{app.appName}</h4>
          <Tooltip
            tip={
              !app.verified
                ? `This app is not verified yet. Unverified apps can be authorized by up to ${app.unverifiedTeamLimit} teams. Verify your app to remove this limit and let an unlimited number of teams connect.`
                : undefined
            }
            side="right"
          >
            <div
              className={cn(
                "flex items-center gap-1 rounded-sm border p-1 text-xs",
                !app.verified &&
                  "border-yellow-700 bg-yellow-100/50 text-yellow-700 dark:border-yellow-200 dark:bg-yellow-900/50 dark:text-yellow-200",
              )}
            >
              {app.verified ? "Verified" : "Unverified"}
              {!app.verified && <QuestionMarkCircledIcon />}
            </div>
          </Tooltip>
          <AuthorizedTeamCount app={app} />
        </div>
        <div className="flex items-center gap-2">
          <TimestampDistance date={new Date(app.createTime)} prefix="Created" />
          <Menu
            placement="bottom-start"
            buttonProps={{
              variant: "neutral",
              icon: <DotsVerticalIcon />,
              "aria-label": "App options",
              size: "xs",
            }}
          >
            {!app.verified ? (
              <MenuItem
                action={() => setVerificationModalOpen(true)}
                disabled={!canUpdate}
                tip={
                  !canUpdate
                    ? permissionDeniedTip(
                        "You do not have permission to request verification.",
                        "oauthApplication:update",
                      )
                    : undefined
                }
                tipSide="right"
              >
                Request Verification
              </MenuItem>
            ) : null}
            <MenuItem
              action={() => setEditModalOpen(true)}
              disabled={!canUpdate}
              tip={
                !canUpdate
                  ? permissionDeniedTip(
                      "You do not have permission to edit OAuth apps.",
                      "oauthApplication:update",
                    )
                  : undefined
              }
              tipSide="right"
            >
              Edit Application
            </MenuItem>
            <MenuItem
              action={() => setShowRegenerateSecret(true)}
              disabled={!canRegenerateSecret}
              tip={
                !canRegenerateSecret
                  ? permissionDeniedTip(
                      "You do not have permission to regenerate the client secret.",
                      "oauthApplication:generateClientSecret",
                    )
                  : undefined
              }
              tipSide="right"
            >
              Regenerate Client Secret
            </MenuItem>
            <MenuItem
              action={() => setShowDelete(true)}
              variant="danger"
              disabled={!canDelete}
              tip={
                !canDelete
                  ? permissionDeniedTip(
                      "You do not have permission to delete OAuth apps.",
                      "oauthApplication:delete",
                    )
                  : undefined
              }
              tipSide="right"
            >
              Delete Application
            </MenuItem>
          </Menu>
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        <div className="text-xs break-all">
          <div className="leading-6 font-semibold">Client ID</div>
          <CopyTextButton text={app.clientId} className="font-mono text-xs" />
        </div>
        {app.clientSecret ? (
          <div className="text-xs break-all">
            <div className="flex items-center gap-1 leading-6 font-semibold">
              Client Secret
              <Button
                type="button"
                aria-label={
                  secretVisible ? "Hide client secret" : "Show client secret"
                }
                inline
                size="xs"
                variant="neutral"
                onClick={() => setSecretVisible((v) => !v)}
              >
                {secretVisible ? <EyeNoneIcon /> : <EyeOpenIcon />}
              </Button>
            </div>
            <CopyTextButton
              text={app.clientSecret}
              className="font-mono text-xs"
              textHidden={!secretVisible}
            />
          </div>
        ) : (
          <div className="text-xs break-all">
            <div className="flex items-center gap-1 leading-6 font-semibold">
              Client Secret
              <HelpTooltip>
                Only team admins can see the client secret.
              </HelpTooltip>
            </div>
            <span className="text-xs/6.5">
              •••••••••••••••••••••••••••••••••
            </span>
          </div>
        )}
      </div>
      <div className="text-xs">
        <span className="leading-6 font-semibold">Redirect URIs</span>
        <ul className="list-inside list-disc">
          {app.redirectUris.map((uri: string, i: number) => (
            <li key={i} className="break-all">
              {uri}
            </li>
          ))}
        </ul>
      </div>
      {editModalOpen && (
        <Modal onClose={() => setEditModalOpen(false)} title="Edit OAuth App">
          <OauthAppForm
            initialValues={{ appName: editName, redirectUris: editUris }}
            isVerified={app.verified}
            validationSchema={oauthAppSchema}
            onSubmit={async (
              values: { appName: string; redirectUris: string },
              {
                setSubmitting,
              }: {
                setSubmitting: (isSubmitting: boolean) => void;
                resetForm?: () => void;
              },
            ) => {
              setEditError("");
              setEditLoading(true);
              setSubmitting(true);
              try {
                const redirectUris = values.redirectUris
                  .split(",")
                  .map((s: string) => s.trim())
                  .filter(Boolean);
                await updateOauthApp({
                  appName:
                    values.appName === app.appName ? undefined : values.appName,
                  redirectUris,
                });
                setEditModalOpen(false);
                setEditName(values.appName);
                setEditUris(values.redirectUris);
              } catch (err: any) {
                setEditError(err?.message || "Failed to update app");
              } finally {
                setEditLoading(false);
                setSubmitting(false);
              }
            }}
            submitLabel="Save"
            loading={editLoading}
            error={editError}
          />
        </Modal>
      )}
      {showDelete && (
        <ConfirmationDialog
          dialogTitle="Delete OAuth App"
          dialogBody="Are you sure you want to delete this OAuth app? This cannot be undone."
          validationText={app.appName}
          confirmText="Delete"
          error={deleteError}
          onClose={() => setShowDelete(false)}
          onConfirm={async () => {
            try {
              await deleteOauthApp();
              setShowDelete(false);
            } catch (err: any) {
              setDeleteError(err?.message || "Failed to delete app");
            }
          }}
        />
      )}
      {showRegenerateSecret && (
        <ConfirmationDialog
          dialogTitle="Regenerate Client Secret"
          dialogBody="Regenerating the client secret will immediately invalidate the old client secret."
          validationText={app.appName}
          confirmText="Regenerate"
          error={regenerateSecretError}
          onClose={() => setShowRegenerateSecret(false)}
          onConfirm={async () => {
            try {
              await regenerateOauthClientSecret();
              setShowRegenerateSecret(false);
            } catch (err: any) {
              setRegenerateSecretError(
                err?.message || "Failed to regenerate client secret",
              );
            }
          }}
        />
      )}
      {verificationModalOpen && (
        <Modal
          onClose={() => setVerificationModalOpen(false)}
          title="Request OAuth App Verification"
        >
          <VerificationRequestForm
            app={app}
            onSubmit={(values) =>
              handleVerificationRequest(
                values,
                app,
                teamId,
                setVerificationError,
                setVerificationLoading,
                setVerificationModalOpen,
              )
            }
            loading={verificationLoading}
            error={verificationError}
          />
        </Modal>
      )}
    </div>
  );
}
