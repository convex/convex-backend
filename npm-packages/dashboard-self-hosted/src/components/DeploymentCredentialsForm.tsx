import { EnterIcon, EyeNoneIcon, EyeOpenIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { TextInput } from "@ui/TextInput";
import { useState } from "react";

export function DeploymentCredentialsForm({
  onSubmit,
  initialAdminKey,
  initialDeploymentUrl,
}: {
  onSubmit: ({
    submittedAdminKey,
    submittedDeploymentUrl,
    submittedDeploymentName,
  }: {
    submittedAdminKey: string;
    submittedDeploymentUrl: string;
    submittedDeploymentName: string;
  }) => Promise<void>;
  initialAdminKey: string | null;
  initialDeploymentUrl: string | null;
}) {
  const [draftAdminKey, setDraftAdminKey] = useState<string>(
    initialAdminKey ?? "",
  );
  const [draftDeploymentUrl, setDraftDeploymentUrl] = useState<string>(
    initialDeploymentUrl ?? "",
  );
  const [showKey, setShowKey] = useState(false);
  return (
    <form
      className="flex w-120 flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        void onSubmit({
          submittedAdminKey: draftAdminKey,
          submittedDeploymentUrl: draftDeploymentUrl,
          submittedDeploymentName: "",
        });
      }}
    >
      <TextInput
        id="deploymentUrl"
        label="Deployment URL"
        value={draftDeploymentUrl}
        placeholder="Enter the deployment URL"
        onChange={(e) => {
          setDraftDeploymentUrl(e.target.value);
        }}
      />
      <TextInput
        id="adminKey"
        label="Admin Key"
        type={showKey ? "text" : "password"}
        Icon={showKey ? EyeNoneIcon : EyeOpenIcon}
        outerClassname="w-[30rem]"
        placeholder="Enter the admin key for this deployment"
        value={draftAdminKey}
        action={() => {
          setShowKey(!showKey);
        }}
        description="The admin key is required every time you open the dashboard."
        onChange={(e) => {
          setDraftAdminKey(e.target.value);
        }}
      />
      <Button
        type="submit"
        icon={<EnterIcon />}
        disabled={!draftAdminKey || !draftDeploymentUrl}
        size="xs"
        className="ml-auto w-fit"
      >
        Log In
      </Button>
    </form>
  );
}
