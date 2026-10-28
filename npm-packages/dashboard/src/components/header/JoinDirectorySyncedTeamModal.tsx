import { useState } from "react";
import { useRouter } from "next/router";
import { Button } from "@ui/Button";
import { Link } from "@ui/Link";
import { Modal } from "@ui/Modal";
import { Avatar } from "elements/Avatar";
import { useJoinDirectorySyncedTeam } from "api/directorySync";
import { useIgnoredDirectorySyncTeams } from "hooks/useIgnoredDirectorySyncTeams";
import {
  useJoinDirectorySyncedTeamPrompt,
  type JoinDirectorySyncedTeamPrompt,
} from "hooks/useJoinDirectorySyncedTeamModal";
import { PROFILE_AVAILABLE_TEAMS_SECTION } from "lib/sectionAnchors";

// The prompt shown before joining a team that lists one of your verified emails
// in its directory. Rendered once near the app root and driven entirely by
// `useJoinDirectorySyncedTeamPrompt`, like the Create Team modal, so the team
// switcher and the Profile page can both open it.
export function JoinDirectorySyncedTeamModal() {
  const [prompt, setPrompt] = useJoinDirectorySyncedTeamPrompt();

  if (!prompt) {
    return null;
  }

  return (
    // Keyed by team so reopening the modal starts back on the prompt rather
    // than on whatever step the previous offer ended on.
    <JoinDirectorySyncedTeamDialog
      key={prompt.offer.teamId}
      prompt={prompt}
      onClose={() => setPrompt(null)}
    />
  );
}

function JoinDirectorySyncedTeamDialog({
  prompt: { offer, canIgnore },
  onClose,
}: {
  prompt: JoinDirectorySyncedTeamPrompt;
  onClose: () => void;
}) {
  const joinTeam = useJoinDirectorySyncedTeam();
  const { ignoreTeam, isReady: canRecordIgnore } =
    useIgnoredDirectorySyncTeams();
  const router = useRouter();
  const [isJoining, setIsJoining] = useState(false);
  const [isIgnored, setIsIgnored] = useState(false);

  if (isIgnored) {
    return (
      <Modal title="Invitation ignored" onClose={onClose}>
        <div className="flex flex-col gap-5">
          <p className="max-w-prose">
            The team switcher won't offer{" "}
            <span className="font-semibold">{offer.teamName}</span> anymore. You
            can still join it from{" "}
            <Link
              href={`/profile#${PROFILE_AVAILABLE_TEAMS_SECTION.id}`}
              // Without this the modal stays up over the page it just
              // navigated to.
              onClick={onClose}
            >
              {PROFILE_AVAILABLE_TEAMS_SECTION.label}
            </Link>{" "}
            on your profile, which lists every team you're eligible to join.
          </p>
          <div className="flex justify-end">
            <Button onClick={onClose}>Done</Button>
          </div>
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      title={
        <span className="flex items-center gap-2">
          <Avatar name={offer.teamName} hashKey={offer.teamId.toString()} />
          Join {offer.teamName}
        </span>
      }
      onClose={onClose}
    >
      <div className="flex flex-col gap-5">
        <p className="max-w-prose">
          <span className="font-semibold">{offer.email}</span> is listed in the
          directory of <span className="font-semibold">{offer.teamName}</span>.
        </p>
        <div className="flex justify-end gap-2">
          {canIgnore ? (
            <Button
              variant="neutral"
              disabled={isJoining || !canRecordIgnore}
              tip={
                canRecordIgnore ? undefined : "Loading your account details…"
              }
              onClick={() => {
                ignoreTeam(offer.teamId);
                setIsIgnored(true);
              }}
            >
              Ignore
            </Button>
          ) : (
            <Button variant="neutral" disabled={isJoining} onClick={onClose}>
              Cancel
            </Button>
          )}
          <Button
            loading={isJoining}
            onClick={async () => {
              setIsJoining(true);
              let joined;
              try {
                joined = await joinTeam({ proposedTeamId: offer.teamId });
              } catch {
                // The join can fail on a seat limit or a directory user already
                // linked to another account. `useBBMutation` has toasted the
                // message; stay open so the prompt can be retried or dismissed.
                setIsJoining(false);
                return;
              }
              // Past this point they are on the team, so close before
              // navigating: a failed route change must not leave a retryable
              // prompt behind and invite a second, now-pointless join.
              onClose();
              try {
                await router.push(`/t/${joined!.teamSlug}`);
              } catch {
                // A route change can abort for reasons that have nothing to do
                // with the join (a competing navigation, say). The teams list
                // has already been revalidated, so the team is reachable from
                // wherever they ended up.
              }
            }}
          >
            Join team
          </Button>
        </div>
      </div>
    </Modal>
  );
}
