import { useContext } from "react";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { NentSwitcherList } from "@common/elements/NentSwitcherList";
import { NentSwitcherTree } from "@common/elements/NentSwitcherTree";

export { NentNameOption } from "@common/elements/NentSwitcherList";

export function NentSwitcher(props: {
  onChange?: (nent: string | null) => void;
  className?: string;
}) {
  const { componentSwitcherTreeEnabled } = useContext(DeploymentInfoContext);
  return componentSwitcherTreeEnabled ? (
    <NentSwitcherTree {...props} />
  ) : (
    <NentSwitcherList {...props} />
  );
}
