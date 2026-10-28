import { Button } from "@ui/Button";
import { Sheet } from "@ui/Sheet";
import { Avatar } from "elements/Avatar";
import type { DirectorySyncOffer } from "generatedApi";
import { useJoinDirectorySyncedTeamPrompt } from "hooks/useJoinDirectorySyncedTeamModal";
import { PROFILE_AVAILABLE_TEAMS_SECTION } from "lib/sectionAnchors";

// Teams whose directory lists one of the member's verified emails, so they can
// join without an invitation. Unlike the team switcher this also lists the ones
// they ignored there, so ignoring an offer never loses it.
export function AvailableTeams({ offers }: { offers: DirectorySyncOffer[] }) {
  return (
    <Sheet
      id={PROFILE_AVAILABLE_TEAMS_SECTION.id}
      className="flex flex-col gap-4"
    >
      <h3>{PROFILE_AVAILABLE_TEAMS_SECTION.label}</h3>

      <p className="max-w-lg text-sm">
        These teams have directory sync enabled and list one of your verified
        emails.
      </p>

      <div className="flex flex-col divide-y divide-border-transparent">
        {offers.map((offer) => (
          <AvailableTeamRow key={offer.teamId} offer={offer} />
        ))}
      </div>
    </Sheet>
  );
}

function AvailableTeamRow({ offer }: { offer: DirectorySyncOffer }) {
  const [, setJoinPrompt] = useJoinDirectorySyncedTeamPrompt();
  return (
    <div className="flex items-center gap-4 py-4">
      <Avatar name={offer.teamName} hashKey={offer.teamId.toString()} />
      <div className="flex min-w-0 flex-col">
        <span className="truncate font-medium text-content-primary">
          {offer.teamName}
        </span>
        <span className="truncate text-sm text-content-secondary">
          {offer.email}
        </span>
      </div>
      <Button
        className="ml-auto"
        onClick={() => setJoinPrompt({ offer, canIgnore: false })}
      >
        Join
      </Button>
    </div>
  );
}
