import { DotsVerticalIcon } from "@radix-ui/react-icons";
import { Menu, MenuItem } from "@ui/Menu";
import { ReferralState, TeamResponse } from "generatedApi";
import { CopyTextButton } from "@common/elements/CopyTextButton";
import { ReferralProgress } from "./ReferralProgress";

interface ReferralsBannerProps {
  team: TeamResponse;
  referralState: ReferralState;
  onHide: () => void;
}

export function ReferralsBanner({
  team,
  referralState,
  onHide,
}: ReferralsBannerProps) {
  const referralCode = team?.referralCode;

  return (
    <div className="mb-4 flex items-center gap-2 overflow-x-auto rounded-lg border bg-background-secondary px-4 py-2">
      <div className="flex grow items-center gap-2 py-2 md:justify-between">
        <div className="flex max-w-prose grow flex-col flex-wrap gap-2 xl:flex-row xl:items-center">
          <span className="text-sm font-medium text-balance">
            Boost your free plan resource limits by up to 5 times by sharing
            your referral code{" "}
          </span>
          <div className="w-72">
            <CopyTextButton
              text={`https://convex.dev/referral/${referralCode}`}
            />
          </div>
        </div>
        <ReferralProgress referralState={referralState} />
      </div>
      <Menu
        placement="bottom-start"
        buttonProps={{
          "aria-label": "Open project settings",
          variant: "neutral",
          inline: true,
          className: "w-fit h-fit",
          icon: <DotsVerticalIcon className="text-content-secondary" />,
        }}
      >
        <MenuItem action={onHide}>Hide banner</MenuItem>
        <MenuItem href={`/t/${team?.slug}/settings/referrals`}>
          View referrals
        </MenuItem>
      </Menu>
    </div>
  );
}
