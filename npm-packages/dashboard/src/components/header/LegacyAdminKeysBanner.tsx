import classNames from "classnames";
import { Cross2Icon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Link } from "@ui/Link";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { useCurrentTeam } from "api/teams";
import {
  formatDeploymentCount,
  LEGACY_ADMIN_KEYS_SECTION_ID,
  RECENT_LEGACY_ADMIN_KEY_USE_MS,
  useShowLegacyAdminKeyUsage,
  useTeamLegacyAdminKeyUsage,
} from "api/legacyAdminKeys";

export function LegacyAdminKeysBanner({
  deploymentCount,
  onDismiss,
}: {
  deploymentCount: number;
  onDismiss: () => void;
}) {
  const team = useCurrentTeam();
  return (
    <div
      className={classNames(
        "relative flex items-center justify-center border-b px-10 py-1 text-center h-24 sm:h-12 overflow-x-hidden",
        "bg-background-error text-content-error text-xs",
      )}
    >
      <span>
        {formatDeploymentCount(deploymentCount)} in this team{" "}
        {deploymentCount === 1 ? "has" : "have"} used a legacy admin key
        recently. Replace them with deploy keys, then{" "}
        <Link
          href={`/t/${team?.slug}/settings#${LEGACY_ADMIN_KEYS_SECTION_ID}`}
          passHref
        >
          disable legacy admin keys
        </Link>
        .
      </span>
      <Button
        className="absolute right-2 h-fit"
        variant="neutral"
        size="xs"
        inline
        title="Dismiss"
        onClick={onDismiss}
      >
        <Cross2Icon />
      </Button>
    </div>
  );
}

export function useLegacyAdminKeysBanner(): {
  deploymentCount: number;
  dismiss: () => void;
} {
  const team = useCurrentTeam();
  const show = useShowLegacyAdminKeyUsage();
  const [isDismissed, setIsDismissed] = useGlobalLocalStorage(
    `legacyAdminKeysBannerDismissed:${team?.id}`,
    false,
  );
  const usage = useTeamLegacyAdminKeyUsage(
    show && !isDismissed ? team?.id : undefined,
  );
  const dismiss = () => setIsDismissed(true);
  if (
    !show ||
    isDismissed ||
    usage === undefined ||
    usage.disabledTime !== null
  ) {
    return { deploymentCount: 0, dismiss };
  }
  return {
    deploymentCount: usage.deployments.filter(
      (d) => Date.now() - d.lastUsedTime < RECENT_LEGACY_ADMIN_KEY_USE_MS,
    ).length,
    dismiss,
  };
}
