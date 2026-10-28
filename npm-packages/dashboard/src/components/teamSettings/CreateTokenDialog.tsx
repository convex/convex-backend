import { Modal } from "@ui/Modal";
import { Button } from "@ui/Button";
import { TextInput } from "@ui/TextInput";
import { Callout } from "@ui/Callout";
import { CopyButton } from "@common/elements/CopyButton";
import { Formik } from "formik";
import * as Yup from "yup";
import React, { useState } from "react";
import {
  TokenExpirationSelector,
  TokenExpirationValue,
  resolveExpirationTime,
} from "components/TokenExpirationSelector";
import {
  CreateTeamAccessTokenArgs,
  CreateTeamAccessTokenResponse,
} from "@convex-dev/platform/managementApi";

const CREATE_TOKEN_SCHEMA = Yup.object({
  tokenName: Yup.string()
    .min(1, "Token name is required")
    .max(50, "Token name must be at most 50 characters")
    .required("Token name is required"),
});

export function CreateTokenDialog({
  onClose,
  onSubmit,
}: {
  onClose: () => void;
  onSubmit: (
    args: CreateTeamAccessTokenArgs,
  ) => Promise<CreateTeamAccessTokenResponse>;
}) {
  const [expiration, setExpiration] = useState<TokenExpirationValue>(null);
  const [createdToken, setCreatedToken] = useState<string | null>(null);

  if (createdToken !== null) {
    return (
      <Modal onClose={onClose} title="Team Access Token Created">
        <div className="flex flex-col gap-4">
          <Callout variant="instructions">
            Copy your new access token now. You won't be able to see it again.
          </Callout>
          <div className="flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded-sm bg-background-tertiary px-2 py-1 text-sm">
              {createdToken}
            </code>
            <CopyButton text={createdToken} />
          </div>
          <div className="flex justify-end">
            <Button onClick={onClose}>Done</Button>
          </div>
        </div>
      </Modal>
    );
  }

  return (
    <Modal onClose={onClose} title="Create Team Access Token">
      <Formik
        initialValues={{ tokenName: "" }}
        validationSchema={CREATE_TOKEN_SCHEMA}
        onSubmit={async (values, { setSubmitting }) => {
          const expiresAt = resolveExpirationTime(expiration);
          try {
            const result = await onSubmit({
              name: values.tokenName,
              ...(expiresAt !== null && { expiresAt }),
            });
            setCreatedToken(result.accessToken);
          } finally {
            setSubmitting(false);
          }
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
          <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
            <p className="text-sm text-content-primary">
              This token will allow programmatic access to your team's
              resources. Keep it secure and do not share it publicly.
            </p>

            <TextInput
              id="tokenName"
              label="Token Name"
              type="text"
              value={values.tokenName}
              onChange={handleChange}
              placeholder="Enter a memorable name, like 'asdfjkl;'"
              autoFocus
              error={
                touched.tokenName && typeof errors.tokenName === "string"
                  ? errors.tokenName
                  : undefined
              }
              required
            />

            <TokenExpirationSelector
              value={expiration}
              onChange={setExpiration}
            />

            <div className="flex justify-end gap-2">
              <Button variant="neutral" onClick={onClose} type="button">
                Cancel
              </Button>
              <Button type="submit" loading={isSubmitting}>
                Save
              </Button>
            </div>
          </form>
        )}
      </Formik>
    </Modal>
  );
}
