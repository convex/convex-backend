import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { LoadingTransition } from "@ui/Loading";
import { PlusIcon, Cross2Icon } from "@radix-ui/react-icons";
import { useState } from "react";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { CopyButton } from "@common/elements/CopyButton";
import { Tooltip } from "@ui/Tooltip";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Modal } from "@ui/Modal";
import { TextInput } from "@ui/TextInput";
import { Formik } from "formik";
import * as Yup from "yup";
import {
  usePaginatedPersonalAccessTokens,
  useCreatePersonalAccessToken,
  useDeletePersonalAccessToken,
} from "api/personalAccessTokens";
import { useTeams } from "api/teams";
import { PaginationControls } from "elements/PaginationControls";
import {
  useCursorPagination,
  useSnapBackOnEmptyPage,
} from "hooks/useCursorPagination";
import { PROFILE_SECTIONS } from "lib/sectionAnchors";
import {
  TokenExpirationSelector,
  TokenExpirationValue,
  resolveExpirationTime,
} from "components/TokenExpirationSelector";

type PersonalAccessToken = {
  name: string;
  creationTime: number;
  lastUsedTime?: number | null;
  expiresAt?: number | null;
  ssoTeamId?: number | null;
};

export function PersonalAccessTokens() {
  const createToken = useCreatePersonalAccessToken();
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const {
    currentCursor,
    currentPage,
    canGoPrevious,
    onNextPage,
    onPreviousPage,
  } = useCursorPagination();

  const { data, isLoading } = usePaginatedPersonalAccessTokens(currentCursor);

  const tokens = data?.items;
  const hasMore = data?.pagination.hasMore ?? false;
  const nextCursor = data?.pagination.nextCursor;

  useSnapBackOnEmptyPage(
    { canGoPrevious, onPreviousPage },
    { isLoading, currentPageItems: tokens },
  );

  return (
    <Sheet
      id={PROFILE_SECTIONS.personalAccessTokens.id}
      className="flex flex-col gap-4"
    >
      <div className="flex items-center justify-between">
        <h3>Personal Access Tokens</h3>
        <Button onClick={() => setShowCreateDialog(true)} icon={<PlusIcon />}>
          Create Token
        </Button>
      </div>
      <p className="max-w-prose text-sm text-content-primary">
        Personal access tokens allow you to authenticate with the Convex CLI and
        APIs. They have the same access as your account.
      </p>

      <LoadingTransition
        loadingProps={{ fullHeight: false, className: "h-14 w-full" }}
      >
        {tokens !== undefined && (
          <div className="flex w-full flex-col divide-y">
            {tokens.length > 0
              ? tokens.map((token) => (
                  <PersonalAccessTokenItem key={token.name} token={token} />
                ))
              : !isLoading && (
                  <div className="my-6 flex w-full justify-center text-content-secondary">
                    You have not created any personal access tokens yet.
                  </div>
                )}
          </div>
        )}
      </LoadingTransition>
      {tokens && tokens.length > 0 && (hasMore || canGoPrevious) && (
        <PaginationControls
          isCursorBasedPagination
          currentPage={currentPage}
          hasMore={hasMore}
          pageSize={10}
          onPageSizeChange={() => {}}
          onPreviousPage={onPreviousPage}
          onNextPage={() => onNextPage(nextCursor)}
          canGoPrevious={canGoPrevious}
          showPageSize={false}
        />
      )}
      {showCreateDialog && (
        <CreatePersonalTokenDialog
          onClose={() => setShowCreateDialog(false)}
          onCreate={async ({ tokenName, expiresAt }) => {
            const result = await createToken({
              name: tokenName,
              ...(expiresAt !== undefined && { expiresAt }),
            });
            return result?.accessToken ?? null;
          }}
        />
      )}
    </Sheet>
  );
}

function PersonalAccessTokenItem({ token }: { token: PersonalAccessToken }) {
  const deleteToken = useDeletePersonalAccessToken();
  const [showDeleteConfirmation, setShowDeleteConfirmation] = useState(false);
  const { teams } = useTeams();
  const ssoTeamName =
    token.ssoTeamId !== null && token.ssoTeamId !== undefined
      ? teams?.find((t) => t.id === token.ssoTeamId)?.name
      : undefined;

  return (
    <div className="flex w-full flex-col py-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          {token.name}
          {ssoTeamName && (
            <Tooltip
              tip={`This token was created with an SSO-based login, and has access to ${ssoTeamName}.`}
            >
              <span className="rounded-full bg-background-tertiary px-2 py-0.5 text-xs text-content-primary">
                SSO Access
              </span>
            </Tooltip>
          )}
        </div>
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
            {token.expiresAt !== null && token.expiresAt !== undefined && (
              <TimestampDistance
                prefix="Expires "
                date={new Date(token.expiresAt)}
                className="text-left text-content-errorSecondary"
              />
            )}
          </div>
          <Button
            variant="danger"
            icon={<Cross2Icon />}
            onClick={() => setShowDeleteConfirmation(true)}
          >
            Delete
          </Button>
        </div>
      </div>
      {showDeleteConfirmation && (
        <ConfirmationDialog
          onClose={() => setShowDeleteConfirmation(false)}
          onConfirm={async () => {
            await deleteToken({ id: token.name });
          }}
          confirmText="Delete"
          dialogTitle="Delete Personal Access Token"
          dialogBody={
            <>
              Are you sure you want to delete:{" "}
              <span className="font-semibold">{token.name}</span>?
            </>
          }
        />
      )}
    </div>
  );
}

const CREATE_TOKEN_SCHEMA = Yup.object({
  tokenName: Yup.string()
    .min(1, "Token name is required")
    .max(50, "Token name must be at most 50 characters")
    .required("Token name is required"),
});

function CreatePersonalTokenDialog({
  onClose,
  onCreate,
}: {
  onClose: () => void;
  onCreate: (args: {
    tokenName: string;
    expiresAt?: number;
  }) => Promise<string | null>;
}) {
  const [createdToken, setCreatedToken] = useState<string | null>(null);
  const [expiration, setExpiration] = useState<TokenExpirationValue>(null);

  if (createdToken) {
    return (
      <Modal onClose={onClose} title="Personal Access Token Created">
        <div className="flex flex-col gap-4">
          <p className="text-sm text-content-primary">
            Copy your new token now. You won't be able to see it again.
          </p>
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
    <Modal onClose={onClose} title="Create Personal Access Token">
      <Formik
        initialValues={{ tokenName: "" }}
        validationSchema={CREATE_TOKEN_SCHEMA}
        onSubmit={async (values, { setSubmitting }) => {
          const expiresAt = resolveExpirationTime(expiration);
          const token = await onCreate({
            tokenName: values.tokenName,
            ...(expiresAt !== null && { expiresAt }),
          });
          if (token) {
            setCreatedToken(token);
          }
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
          <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
            <p className="text-sm text-content-primary">
              This token will have the same access as your account. Keep it
              secure and do not share it publicly.
            </p>
            <TextInput
              id="tokenName"
              label="Token Name"
              type="text"
              value={values.tokenName}
              onChange={handleChange}
              placeholder="Enter a name for your PAT"
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
                Create
              </Button>
            </div>
          </form>
        )}
      </Formik>
    </Modal>
  );
}
