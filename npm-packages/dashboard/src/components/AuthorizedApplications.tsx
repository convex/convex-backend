import { AppAccessTokenResponse } from "generatedApi";
import { Sheet } from "@ui/Sheet";
import { LoadingTransition } from "@ui/Loading";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { Button } from "@ui/Button";
import {
  Cross2Icon,
  DotsVerticalIcon,
  ExclamationTriangleIcon,
} from "@radix-ui/react-icons";
import React, { useState } from "react";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Menu, MenuItem } from "@ui/Menu";
import { Modal } from "@ui/Modal";
import { Formik } from "formik";
import * as Yup from "yup";
import { toast } from "@common/lib/utils";
import { captureException, captureMessage } from "@sentry/nextjs";
import { useCurrentTeam } from "api/teams";

export function AuthorizedApplications({
  accessTokens,
  explainer,
  onRevoke,
  revokeDisabledReason,
}: {
  accessTokens: AppAccessTokenResponse[] | undefined;
  explainer: React.ReactNode;
  onRevoke: (token: AppAccessTokenResponse) => Promise<void>;
  revokeDisabledReason?: React.ReactNode;
}) {
  return (
    <Sheet>
      {explainer}
      <LoadingTransition
        loadingProps={{ fullHeight: false, className: "h-14 w-full" }}
      >
        {accessTokens !== undefined && (
          <div className="mt-2 flex w-full flex-col gap-2 divide-y">
            {accessTokens.length ? (
              accessTokens
                .sort((a, b) => b.creationTime - a.creationTime)
                .map((token) => (
                  <AuthorizedApplicationListItem
                    key={token.name}
                    token={token}
                    onRevoke={onRevoke}
                    revokeDisabledReason={revokeDisabledReason}
                  />
                ))
            ) : (
              <div className="my-6 flex w-full justify-center text-content-secondary">
                You have not authorized any applications yet.
              </div>
            )}
          </div>
        )}
      </LoadingTransition>
    </Sheet>
  );
}

function AuthorizedApplicationListItem({
  token,
  onRevoke,
  revokeDisabledReason,
}: {
  token: AppAccessTokenResponse;
  onRevoke: (token: AppAccessTokenResponse) => Promise<void>;
  revokeDisabledReason?: React.ReactNode;
}) {
  const team = useCurrentTeam();
  const [showConfirmation, setShowConfirmation] = useState(false);

  // Local state for abuse report modal
  const [abuseModalOpen, setAbuseModalOpen] = useState(false);
  const [abuseError, setAbuseError] = useState("");
  const [abuseLoading, setAbuseLoading] = useState(false);

  return (
    <div className="flex w-full flex-col pb-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>{token.appName}</div>
        <div className="flex flex-wrap items-center gap-4">
          <div className="flex flex-col items-end">
            {token.lastUsedTime !== null && token.lastUsedTime !== undefined ? (
              <TimestampDistance
                prefix="Last used "
                date={new Date(token.lastUsedTime)}
              />
            ) : (
              <div className="text-xs text-content-secondary">Never used</div>
            )}
            <TimestampDistance
              prefix="Created "
              date={new Date(token.creationTime)}
            />
          </div>
          <Menu
            placement="bottom-start"
            buttonProps={{
              variant: "neutral",
              icon: <DotsVerticalIcon />,
              "aria-label": "Application options",
              size: "xs",
            }}
          >
            <MenuItem action={() => setAbuseModalOpen(true)}>
              <ExclamationTriangleIcon />
              Report Abuse
            </MenuItem>
            <MenuItem
              action={() => setShowConfirmation(true)}
              variant="danger"
              disabled={revokeDisabledReason !== undefined}
              tip={revokeDisabledReason}
            >
              <Cross2Icon />
              Revoke
            </MenuItem>
          </Menu>
        </div>
      </div>
      {showConfirmation && (
        <ConfirmationDialog
          dialogTitle={`Revoke access for ${token.appName}`}
          dialogBody="Are you sure you want to revoke access for this application?"
          confirmText="Revoke"
          onClose={() => setShowConfirmation(false)}
          onConfirm={async () => {
            await onRevoke(token);
          }}
        />
      )}
      {abuseModalOpen && (
        <Modal
          onClose={() => setAbuseModalOpen(false)}
          title="Report Application Abuse"
        >
          <AbuseReportForm
            token={token}
            onSubmit={(description) =>
              handleAbuseReport(
                description,
                token,
                team?.id!,
                setAbuseError,
                setAbuseLoading,
                setAbuseModalOpen,
              )
            }
            loading={abuseLoading}
            error={abuseError}
          />
        </Modal>
      )}
    </div>
  );
}

// Validation schema for abuse report form
const ABUSE_REPORT_SCHEMA = Yup.object({
  description: Yup.string()
    .min(10, "Description must be at least 10 characters")
    .max(2000, "Description must be at most 2000 characters")
    .required("Description is required"),
});

// Handler for abuse report submission
async function handleAbuseReport(
  description: string,
  token: AppAccessTokenResponse,
  teamId: number,
  setAbuseError: (error: string) => void,
  setAbuseLoading: (loading: boolean) => void,
  setAbuseModalOpen: (open: boolean) => void,
): Promise<void> {
  setAbuseError("");
  setAbuseLoading(true);

  try {
    const body = JSON.stringify({
      subject: `Abuse Report: ${token.appName} (${token.appClientId})`,
      message: `Abuse Report

App Name: ${token.appName}
App ID: ${token.name}
Last Used: ${token.lastUsedTime ? new Date(token.lastUsedTime).toISOString() : "Never"}
Created: ${new Date(token.creationTime).toISOString()}

Description:
${description}

Review this abuse report for the authorized application.`,
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
        "Failed to send abuse report. Please try again or email us at support@convex.dev",
      );
      return;
    }

    setAbuseModalOpen(false);
    toast("success", "Abuse report sent!");
  } catch (err: any) {
    setAbuseError(err?.message || "Failed to send abuse report");
  } finally {
    setAbuseLoading(false);
  }
}

// Abuse report form component
function AbuseReportForm({
  token,
  onSubmit,
  loading,
  error,
}: {
  token: AppAccessTokenResponse;
  onSubmit: (description: string) => Promise<void>;
  loading: boolean;
  error?: string;
}) {
  return (
    <Formik
      initialValues={{ description: "" }}
      validationSchema={ABUSE_REPORT_SCHEMA}
      onSubmit={async (values, { setSubmitting }) => {
        await onSubmit(values.description);
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
            <span>Application Name:</span>
            <span className="ml-1 font-semibold text-content-primary">
              {token.appName}
            </span>
          </div>
          <p className="text-xs text-content-secondary">
            This application has been authorized to operator on behalf of your
            team. If you believe it is misbehaving, use this form to report
            abuse or suspicious activity.
          </p>

          <label
            htmlFor="description"
            className="flex flex-col gap-1 text-sm text-content-primary"
          >
            Abuse Description
            <textarea
              id="description"
              name="description"
              className="h-48 resize-y rounded-sm border bg-background-secondary px-4 py-2 text-content-primary placeholder:text-content-tertiary focus:border-border-selected focus:outline-hidden"
              value={values.description}
              onChange={handleChange}
              placeholder="Describe the abuse or suspicious activity you've observed."
              required
            />
            {touched.description && typeof errors.description === "string" && (
              <p className="text-xs text-content-errorSecondary" role="alert">
                {errors.description}
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
              Send Report
            </Button>
          </div>
        </form>
      )}
    </Formik>
  );
}
