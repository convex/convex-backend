import { useContext } from "react";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { DirectorySidebar } from "@common/features/functions/components/DirectorySidebar";
import { VirtualizedDirectorySidebar } from "@common/features/functions/components/VirtualizedDirectorySidebar";

export function FunctionsSidebar(props: { onChangeFunction: () => void }) {
  const { virtualizedFunctionsSidebarEnabled } = useContext(
    DeploymentInfoContext,
  );
  return virtualizedFunctionsSidebarEnabled ? (
    <VirtualizedDirectorySidebar {...props} />
  ) : (
    <DirectorySidebar {...props} />
  );
}
