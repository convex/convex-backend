import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked } from "storybook/test";
import { useEffect } from "react";
import { useJoinDirectorySyncedTeam } from "api/directorySync";
import { useProfile } from "api/profile";
import { useJoinDirectorySyncedTeamPrompt } from "hooks/useJoinDirectorySyncedTeamModal";
import { JoinDirectorySyncedTeamModal } from "./JoinDirectorySyncedTeamModal";

const offer = {
  teamId: 14,
  teamName: "Example Org",
  email: "nicolas@example.org",
};

// The modal takes what it asks about from a global, so a story fills it in the
// way the team switcher and the Profile page do.
function OpenJoinModal({ canIgnore }: { canIgnore: boolean }) {
  const [, setPrompt] = useJoinDirectorySyncedTeamPrompt();
  useEffect(() => {
    setPrompt({ offer, canIgnore });
    return () => setPrompt(null);
  }, [setPrompt, canIgnore]);
  return <JoinDirectorySyncedTeamModal />;
}

const meta = {
  component: JoinDirectorySyncedTeamModal,
  parameters: {
    layout: "fullscreen",
    // A focus-trapping dialog over an otherwise empty canvas, which trips the
    // automated a11y checks meant for full pages.
    a11y: { test: "todo" },
  },
  beforeEach: () => {
    mocked(useProfile).mockReturnValue({
      id: 1,
      name: "Nicolas Ettlin",
      email: "nicolas@acme.dev",
    });
    mocked(useJoinDirectorySyncedTeam).mockReturnValue(async () => ({
      teamId: offer.teamId,
      teamSlug: "example-org",
    }));
  },
} satisfies Meta<typeof JoinDirectorySyncedTeamModal>;

export default meta;
type Story = StoryObj<typeof meta>;

// Opened from the Profile page's Available Teams section, which is where
// ignored offers stay reachable — so it offers no way to ignore them again.
export const FromProfilePage: Story = {
  render: () => <OpenJoinModal canIgnore={false} />,
};
