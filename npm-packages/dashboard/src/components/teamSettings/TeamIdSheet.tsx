import { CopyIcon } from "@radix-ui/react-icons";
import { Sheet } from "@ui/Sheet";
import { TextInput } from "@ui/TextInput";
import { Link } from "@ui/Link";
import { TeamResponse } from "generatedApi";
import { useCopy } from "@common/lib/useCopy";
import { TEAM_SETTINGS_SECTIONS } from "lib/sectionAnchors";

export type TeamIdSheetProps = {
  team: TeamResponse;
};

export function TeamIdSheet({ team }: TeamIdSheetProps) {
  const copyToClipboard = useCopy("Team ID");
  const teamId = team.id.toString();

  return (
    <Sheet id={TEAM_SETTINGS_SECTIONS.teamId.id} className="text-sm">
      <h3 className="mb-1">Team ID</h3>
      <p className="mb-4 max-w-prose text-content-secondary">
        Your team ID identifies your team in the{" "}
        <Link href="https://docs.convex.dev/platform-apis" target="_blank">
          Convex Platform APIs
        </Link>
        .
      </p>
      <TextInput
        id="teamId"
        label="Team ID"
        labelHidden
        outerClassname="max-w-[20rem]"
        value={teamId}
        readOnly
        Icon={CopyIcon}
        iconTooltip="Copy team ID"
        action={() => copyToClipboard(teamId)}
      />
    </Sheet>
  );
}
