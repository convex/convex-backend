import { Meta, StoryObj } from "@storybook/nextjs";
import { useEffect } from "react";
import { fn, mocked, screen, userEvent } from "storybook/test";
import { TeamResponse } from "generatedApi";
import { useCreateProject } from "api/projects";
import { useCurrentTeam } from "api/teams";
import { flagDefaults, useLaunchDarkly } from "hooks/useLaunchDarkly";
import {
  CreateProjectModal,
  useCreateProjectModalRequest,
} from "./useCreateProjectModal";

const team: TeamResponse = {
  id: 2,
  creator: 1,
  slug: "acme",
  name: "Acme Corp",
  suspended: false,
  referralCode: "ACME01",
  referredBy: null,
};

/** Opens the modal the way the Create Project button does. */
function OpenCreateProjectModal() {
  const [, requestCreateProject] = useCreateProjectModalRequest();
  useEffect(() => {
    requestCreateProject({ team });
    return () => requestCreateProject(null);
  }, [requestCreateProject]);
  return <CreateProjectModal />;
}

const meta = {
  component: OpenCreateProjectModal,
  beforeEach: () => {
    mocked(useCurrentTeam).mockReturnValue(team);
    mocked(useCreateProject).mockReturnValue(fn());
    mocked(useLaunchDarkly).mockReturnValue({
      ...flagDefaults,
      tagEditUi: true,
    });
  },
} satisfies Meta<typeof OpenCreateProjectModal>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithTags: Story = {
  play: async () => {
    await userEvent.type(await screen.findByLabelText("Project Name"), "Shop");
    await userEvent.type(
      screen.getByRole("combobox", { name: "Tags" }),
      "backend{Enter}billing{Enter}prod",
    );
  },
};
