import { useCallback } from "react";
import { useLocalStorage } from "react-use";
import { useProfile } from "api/profile";

// Exported so tests and stories can reset a member's dismissals without
// restating the key format. The key may not be falsy, and there is nothing
// sensible to read or write until we know who is signed in, so park those
// renders on their own bucket.
export function ignoredDirectorySyncTeamsKey(memberId: number | undefined) {
  return `/ignoredDirectorySyncTeams/${memberId ?? "unknown"}`;
}

// Teams the member dismissed from the team switcher's "Available Teams"
// section. Kept client-side rather than on the server: ignoring only hides the
// offer from the switcher, and the Profile page still lists every team they're
// eligible to join.
//
// Scoped to the member, so signing a second account into the same browser
// doesn't inherit the first one's dismissals — and so signing back in restores
// your own.
export function useIgnoredDirectorySyncTeams(): {
  ignoredTeamIds: number[];
  ignoreTeam: (teamId: number) => void;
  // False until `/profile` resolves. Dismissals are keyed by member, so until
  // then there is nowhere to record one, and callers must not offer the
  // action — silently dropping it would tell the member their offer was
  // dismissed when it wasn't.
  isReady: boolean;
} {
  const memberId = useProfile()?.id;
  // No initial value, so the "unknown" bucket is never written to.
  const [ignored, setIgnored] = useLocalStorage<number[]>(
    ignoredDirectorySyncTeamsKey(memberId),
  );

  const ignoreTeam = useCallback(
    (teamId: number) => {
      if (memberId === undefined) {
        return;
      }
      setIgnored([...(ignored ?? []).filter((id) => id !== teamId), teamId]);
    },
    [ignored, memberId, setIgnored],
  );

  return {
    ignoredTeamIds: memberId === undefined ? [] : (ignored ?? []),
    ignoreTeam,
    isReady: memberId !== undefined,
  };
}
