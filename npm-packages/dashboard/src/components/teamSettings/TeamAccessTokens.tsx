import { Sheet } from "@ui/Sheet";
import { LoadingTransition } from "@ui/Loading";
import { AccessTokenListItem } from "components/AccessTokenListItem";
import { Button } from "@ui/Button";
import { PlusIcon } from "@radix-ui/react-icons";
import { HelpTooltip } from "@ui/HelpTooltip";
import React, { useState } from "react";
import { CreateTokenDialog } from "components/teamSettings/CreateTokenDialog";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { useCurrentTeam } from "api/teams";
import { Link } from "@ui/Link";
import { PaginationControls } from "elements/PaginationControls";
import {
  CreateTeamAccessTokenArgs,
  CreateTeamAccessTokenResponse,
  TeamAccessTokenResponse,
} from "@convex-dev/platform/managementApi";

export function TeamAccessTokens({
  accessTokens,
  onCreateToken,
  canCreate,
  canDelete,
  isLoading,
  hasMore,
  currentPage,
  canGoPrevious,
  onPreviousPage,
  onNextPage,
}: {
  accessTokens: TeamAccessTokenResponse[] | undefined;
  onCreateToken: (
    args: CreateTeamAccessTokenArgs,
  ) => Promise<CreateTeamAccessTokenResponse>;
  canCreate: boolean | undefined;
  canDelete: boolean | undefined;
  isLoading: boolean;
  hasMore: boolean;
  currentPage: number;
  canGoPrevious: boolean;
  onPreviousPage: () => void;
  onNextPage: () => void;
}) {
  const team = useCurrentTeam();
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  return (
    <Sheet className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <p className="text-sm text-content-primary">
            These access tokens allow your team to access your Convex projects
            using{" "}
            <Link href="https://docs.convex.dev/platform-apis" target="_blank">
              Convex Platform APIs
            </Link>
            .
          </p>
          <Button
            onClick={() => setShowCreateDialog(true)}
            icon={<PlusIcon />}
            disabled={!canCreate}
            tip={
              canCreate === false
                ? permissionDeniedTip(
                    "You do not have permission to create team access tokens.",
                    "team:token:create",
                  )
                : undefined
            }
          >
            Create Token
          </Button>
        </div>
        <div>
          <div className="flex items-center gap-1">
            <span className="font-semibold">Team ID</span>
            <HelpTooltip tipSide="right">
              This is the ID of your team. It is used to identify your team in
              the Platform API.
            </HelpTooltip>
          </div>
          <span className="font-mono">{team?.id}</span>
        </div>
        <p className="my-2 text-sm text-content-primary">
          A team access token can only perform actions you have access to on the
          team. See{" "}
          <Link
            href="https://docs.convex.dev/team-management/role-actions"
            target="_blank"
          >
            Role Actions
          </Link>{" "}
          for the full list of actions and which roles can perform them.
        </p>
        <p className="mt-1 mb-2 text-sm text-content-primary">
          You cannot see tokens that other members of your team have created.
        </p>
      </div>
      <LoadingTransition
        loadingProps={{ fullHeight: false, className: "h-14 w-full" }}
      >
        {team && accessTokens !== undefined && (
          <div className="flex w-full flex-col divide-y">
            {accessTokens.length > 0
              ? accessTokens.map((token) => (
                  <AccessTokenListItem
                    key={token.name}
                    token={token}
                    teamId={team.id}
                    canDelete={canDelete}
                  />
                ))
              : !isLoading && (
                  <div className="my-6 flex w-full justify-center text-content-secondary">
                    You have not created any team access tokens yet.
                  </div>
                )}
          </div>
        )}
      </LoadingTransition>
      {accessTokens &&
        accessTokens.length > 0 &&
        (hasMore || canGoPrevious) && (
          <PaginationControls
            isCursorBasedPagination
            currentPage={currentPage}
            hasMore={hasMore}
            pageSize={10}
            onPageSizeChange={() => {}}
            onPreviousPage={onPreviousPage}
            onNextPage={onNextPage}
            canGoPrevious={canGoPrevious}
            showPageSize={false}
          />
        )}
      {showCreateDialog && (
        <CreateTokenDialog
          onClose={() => setShowCreateDialog(false)}
          onSubmit={onCreateToken}
        />
      )}
    </Sheet>
  );
}
