import { DotsVerticalIcon, PlusIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Menu, MenuItem, MenuLink } from "@ui/Menu";
import { ReactNode } from "react";
import { AuthIntegration } from "@common/lib/integrationHelpers";

export function WorkOSIntegrationOverflowMenu({
  integration,
  onConfigure,
  disabled = false,
  disabledTip,
}: {
  integration: AuthIntegration;
  onConfigure: () => void;
  disabled?: boolean;
  disabledTip?: ReactNode;
}) {
  const environmentId = integration.existing?.workosEnvironmentId;

  return integration.existing ? (
    <Menu
      placement="bottom-end"
      buttonProps={{
        "aria-label": "Open integration settings",
        className: "ml-auto",
        icon: <DotsVerticalIcon />,
        size: "xs",
        variant: "neutral",
      }}
    >
      <MenuItem
        action={onConfigure}
        disabled={disabled}
        tip={disabled ? disabledTip : undefined}
        tipSide="left"
      >
        Configure Integration
      </MenuItem>
      <MenuLink
        href={`https://dashboard.workos.com/${environmentId}/authentication`}
        target="_blank"
      >
        Go to WorkOS
      </MenuLink>
    </Menu>
  ) : (
    <Button
      size="xs"
      icon={<PlusIcon />}
      variant="neutral"
      tip={disabled ? disabledTip : "Configure Integration"}
      tipSide="right"
      onClick={onConfigure}
      disabled={disabled}
    />
  );
}
