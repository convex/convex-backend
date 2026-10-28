import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { deploymentIdentityLabel } from "@common/lib/deploymentTypeColorClasses";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { useContext } from "react";

export function AuthorizeEditsConfirmationDialog({
  onClose,
  onConfirm,
}: {
  onClose(): void;
  onConfirm(): Promise<void>;
}) {
  const { useCurrentDeployment } = useContext(DeploymentInfoContext);
  const deployment = useCurrentDeployment();
  return (
    <ConfirmationDialog
      onClose={onClose}
      onConfirm={onConfirm}
      confirmText="Confirm"
      variant="primary"
      dialogTitle="Enable edit mode"
      dialogBody={
        <div className="flex flex-col gap-2">
          <p>
            You are about to start editing data in{" "}
            <span className="font-semibold">
              {deployment && deploymentIdentityLabel(deployment)}
            </span>
            .
          </p>
          <p>
            Once confirmed, you will not be asked to confirm again for the
            remainder of your browser session.
          </p>
        </div>
      }
    />
  );
}
