import { createGlobalState } from "react-use";
import type { DirectorySyncOffer } from "generatedApi";

export type JoinDirectorySyncedTeamPrompt = {
  offer: DirectorySyncOffer;
  // The team switcher can hide an offer it isn't interested in; the Profile
  // page, which is where those hidden offers stay reachable, only joins.
  canIgnore: boolean;
};

// What the join-team modal is currently asking about, or `null` when it's
// closed.
export const useJoinDirectorySyncedTeamPrompt =
  createGlobalState<JoinDirectorySyncedTeamPrompt | null>(null);
