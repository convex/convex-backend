import { DeploymentSettingsLayout } from "@common/layouts/DeploymentSettingsLayout";
import { PauseDeployment } from "@common/features/settings/components/PauseDeployment";
import { useRef } from "react";
import { useScrollToHash } from "@common/lib/useScrollToHash";

export default function Settings() {
  const pauseDeploymentRef = useRef<HTMLDivElement | null>(null);

  useScrollToHash("#pause-deployment", pauseDeploymentRef);

  return (
    <DeploymentSettingsLayout page="general">
      <div className="flex flex-col gap-4">
        <div ref={pauseDeploymentRef}>
          <PauseDeployment />
        </div>
      </div>
    </DeploymentSettingsLayout>
  );
}
