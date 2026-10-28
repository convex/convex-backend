import { Command } from "cmdk";
import { GearIcon, PlusIcon } from "@radix-ui/react-icons";
import type { DirectorySyncOffer, TeamResponse } from "generatedApi";
import { useTeams } from "api/teams";
import { useDirectorySyncOffers } from "api/directorySync";
import { Avatar } from "elements/Avatar";
import { useCreateTeamModalOpen } from "hooks/useCreateTeamModal";
import { useIgnoredDirectorySyncTeams } from "hooks/useIgnoredDirectorySyncTeams";
import { useJoinDirectorySyncedTeamPrompt } from "hooks/useJoinDirectorySyncedTeamModal";
import { useRouter } from "next/router";
import { useCopyAction } from "./copy";
import { teamSwitchDestination } from "./navigation";
import {
  ActionItem,
  CurrentBadge,
  HighlightedText,
  LoadingSignal,
  NavigationItem,
  PinnedActions,
} from "./items";
import { usePaletteAnalytics } from "./analytics";

// The drilled-into "Switch Team" page.
export function TeamsCommands({
  onNavigate,
  onClose,
  contextual,
}: {
  onNavigate: (href: string) => void;
  onClose: () => void;
  // Only the anchored team-switcher menu shows the Team Settings shortcut; the
  // main palette's Switch Team page omits it.
  contextual: boolean;
}) {
  const { teams, selectedTeamSlug } = useTeams();
  const offers = useDirectorySyncOffers();
  const { trackSelected } = usePaletteAnalytics();
  const [, setCreateTeamOpen] = useCreateTeamModalOpen();
  const [, setJoinPrompt] = useJoinDirectorySyncedTeamPrompt();
  const { ignoredTeamIds } = useIgnoredDirectorySyncTeams();
  const { pathname } = useRouter();

  const currentTeam = teams?.find((t) => t.slug === selectedTeamSlug);
  const availableTeams = offers?.filter(
    (offer) => !ignoredTeamIds.includes(offer.teamId),
  );

  return (
    <>
      {!teams ? (
        <LoadingSignal />
      ) : (
        <>
          {contextual && currentTeam && (
            <Command.Group heading="Team">
              <NavigationItem
                target={{
                  label: "Team Settings",
                  href: `/t/${currentTeam.slug}/settings`,
                  Icon: GearIcon,
                }}
                onNavigate={onNavigate}
              />
            </Command.Group>
          )}
          {availableTeams && availableTeams.length > 0 && (
            <Command.Group heading="Available Teams">
              {availableTeams.map((offer) => (
                <JoinTeamItem
                  key={offer.teamId}
                  offer={offer}
                  onSelect={() => {
                    trackSelected("join-directory-synced-team");
                    onClose();
                    setJoinPrompt({ offer, canIgnore: true });
                  }}
                />
              ))}
            </Command.Group>
          )}
          <Command.Group heading="Switch Team">
            {teams.map((team) => (
              <TeamItem
                key={team.id}
                team={team}
                isCurrent={team.slug === selectedTeamSlug}
                onSelect={() => {
                  trackSelected("switch-team");
                  onNavigate(teamSwitchDestination(team.slug, pathname));
                }}
              />
            ))}
          </Command.Group>
        </>
      )}
      <PinnedActions>
        <ActionItem
          value="action:create-team"
          onSelect={() => {
            trackSelected("create-team");
            onClose();
            setCreateTeamOpen(true);
          }}
          Icon={PlusIcon}
          label="Create Team…"
        />
      </PinnedActions>
    </>
  );
}

function TeamItem({
  team,
  isCurrent,
  onSelect,
}: {
  team: TeamResponse;
  isCurrent: boolean;
  onSelect: () => void;
}) {
  const value = `team:${team.slug}`;
  useCopyAction(value, { label: "slug", getText: () => team.slug });
  return (
    <Command.Item
      value={value}
      className="animate-fadeInFromLoading"
      keywords={[team.name, team.slug]}
      onSelect={onSelect}
    >
      <Avatar name={team.name} hashKey={team.id.toString()} />
      <span className="flex min-w-0 flex-col">
        <span className="truncate">
          <HighlightedText text={team.name} />
        </span>
        <span className="truncate text-xs text-content-tertiary">
          <HighlightedText text={team.slug} />
        </span>
      </span>
      {isCurrent && (
        <span className="ml-auto text-xs text-content-tertiary">
          <CurrentBadge />
        </span>
      )}
    </Command.Item>
  );
}

// A team the caller isn't in yet but whose directory lists one of their
// verified emails. Dashed, to set it apart from the teams they're already a
// member of.
function JoinTeamItem({
  offer,
  onSelect,
}: {
  offer: DirectorySyncOffer;
  onSelect: () => void;
}) {
  return (
    <Command.Item
      value={`join-team:${offer.teamId}`}
      className="animate-fadeInFromLoading rounded-md outline-1 -outline-offset-1 outline-border-selected/60 outline-dashed"
      keywords={[offer.teamName, offer.email, "join team"]}
      onSelect={onSelect}
    >
      <span className="shrink-0 opacity-50">
        <Avatar name={offer.teamName} hashKey={offer.teamId.toString()} />
      </span>
      <span className="flex min-w-0 flex-col">
        <span className="truncate text-content-secondary">
          <HighlightedText text={`Join ${offer.teamName}`} />
        </span>
        <span className="truncate text-xs text-content-tertiary">
          Select to accept or ignore this invitation.
        </span>
      </span>
      <PlusIcon className="ml-auto size-4 shrink-0 text-content-tertiary" />
    </Command.Item>
  );
}
