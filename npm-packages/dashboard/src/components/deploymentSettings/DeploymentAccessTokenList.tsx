import { PlatformDeployKeyResponse } from "@convex-dev/platform/managementApi";

import { LoadingTransition } from "@ui/Loading";
import { Button } from "@ui/Button";
import { PlusIcon } from "@radix-ui/react-icons";
import { JSX, useState } from "react";
import { DeployKeyListItem } from "components/DeployKeyListItem";
import {
  CreateDeployKeyForm,
  CreateDeployKeyFormProps,
  DeployKeyGenerationDisabledReason,
  DEPLOY_KEY_GENERATION_DISABLED_REASONS,
  getGenerateButtonText,
} from "./GenerateDeployKeyButton";

export function DeploymentAccessTokenList({
  deploymentType,
  onDelete,
  canDelete = true,
  deployKeys,
  disabledReason,
  buttonProps,
  header,
  description,
  headingLevel = "h4",
}: {
  deploymentType: string;
  onDelete: (args: { id: string }) => Promise<unknown>;
  canDelete?: boolean;
  deployKeys: PlatformDeployKeyResponse[] | undefined;
  disabledReason: DeployKeyGenerationDisabledReason | null;
  buttonProps: CreateDeployKeyFormProps;
  header: string;
  description: React.ReactNode;
  headingLevel?: "h3" | "h4";
}) {
  const HeadingTag = (headingLevel ?? "h4") as keyof JSX.IntrinsicElements;
  const [showForm, setShowForm] = useState(false);
  return (
    <>
      <div className="mb-2 flex w-full items-center justify-between">
        <HeadingTag>{header}</HeadingTag>
        <Button
          disabled={buttonProps.disabledReason !== null}
          tip={
            buttonProps.disabledReason === null
              ? undefined
              : DEPLOY_KEY_GENERATION_DISABLED_REASONS[
                  buttonProps.disabledReason
                ]
          }
          onClick={() => setShowForm(true)}
          icon={<PlusIcon />}
        >
          {getGenerateButtonText(buttonProps.deploymentType)}
        </Button>
      </div>
      {description}
      {/* Local deployments don't have remote deploy keys, so the list
          isn't applicable there. For any other disabled reason
          (e.g. the member can view but not create), still render the
          list — including the empty-state message — so they can see
          existing keys. */}
      {disabledReason !== "LocalDeployment" && (
        <LoadingTransition
          loadingProps={{ fullHeight: false, className: "h-14 w-full" }}
        >
          {deployKeys && (
            <div className="flex w-full flex-col divide-y">
              {deployKeys.length > 0 ? (
                deployKeys
                  ?.sort((a, b) => b.creationTime - a.creationTime)
                  .map((deployKey) => (
                    <DeployKeyListItem
                      deployKey={deployKey}
                      deploymentType={deploymentType}
                      onDelete={onDelete}
                      canDelete={canDelete}
                      key={deployKey.name}
                    />
                  ))
              ) : (
                <div className="my-4 flex w-full items-center justify-center rounded-md border border-dashed px-4 py-6 text-center text-sm text-content-secondary">
                  {deploymentType === "preview"
                    ? "No preview deploy keys yet. Create one to deploy previews from CI or a hosting provider."
                    : "No deploy keys yet. Create one to deploy from CI or a hosting provider."}
                </div>
              )}
            </div>
          )}
        </LoadingTransition>
      )}
      {showForm && (
        <CreateDeployKeyForm
          {...buttonProps}
          onClose={() => setShowForm(false)}
        />
      )}
    </>
  );
}
