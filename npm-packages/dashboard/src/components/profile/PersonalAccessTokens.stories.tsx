import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked, fn } from "storybook/test";
import {
  usePaginatedPersonalAccessTokens,
  useCreatePersonalAccessToken,
  useDeletePersonalAccessToken,
} from "api/personalAccessTokens";
import { useTeams } from "api/teams";
import { PersonalAccessTokens } from "./PersonalAccessTokens";

const now = Date.now();
const oneDay = 86400000;

const meta = {
  component: PersonalAccessTokens,
  beforeEach: () => {
    mocked(useCreatePersonalAccessToken).mockReturnValue(fn());
    mocked(useDeletePersonalAccessToken).mockReturnValue(fn());
    mocked(useTeams).mockReturnValue({
      selectedTeamSlug: "my-team",
      teams: [
        { id: 1, name: "My Team", slug: "my-team" } as ReturnType<
          typeof useTeams
        >["teams"] extends (infer T)[] | undefined
          ? T
          : never,
      ],
    });
  },
} satisfies Meta<typeof PersonalAccessTokens>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = {
  beforeEach: () => {
    mocked(usePaginatedPersonalAccessTokens).mockReturnValue({
      data: { items: [], pagination: { hasMore: false } },
      isLoading: false,
    });
  },
};

export const WithTokens: Story = {
  beforeEach: () => {
    mocked(usePaginatedPersonalAccessTokens).mockReturnValue({
      data: {
        items: [
          {
            id: 1,
            name: "ci-deploy",
            creationTime: now - 30 * oneDay,
            lastUsedTime: now - 2 * oneDay,
          },
          {
            id: 2,
            name: "local-dev",
            creationTime: now - 7 * oneDay,
            lastUsedTime: null,
          },
        ],
        pagination: { hasMore: false },
      },
      isLoading: false,
    });
  },
};

export const WithSSOToken: Story = {
  beforeEach: () => {
    mocked(usePaginatedPersonalAccessTokens).mockReturnValue({
      data: {
        items: [
          {
            id: 1,
            name: "ci-deploy",
            creationTime: now - 30 * oneDay,
            lastUsedTime: now - 2 * oneDay,
            ssoTeamId: 1,
          },
          {
            id: 2,
            name: "personal-token",
            creationTime: now - 14 * oneDay,
            lastUsedTime: now - oneDay,
          },
        ],
        pagination: { hasMore: false },
      },
      isLoading: false,
    });
  },
};

export const WithPagination: Story = {
  beforeEach: () => {
    mocked(usePaginatedPersonalAccessTokens).mockReturnValue({
      data: {
        items: [
          {
            id: 1,
            name: "ci-deploy",
            creationTime: now - 30 * oneDay,
            lastUsedTime: now - 2 * oneDay,
          },
        ],
        pagination: { hasMore: true, nextCursor: "abc123" },
      },
      isLoading: false,
    });
  },
};

export const Loading: Story = {
  beforeEach: () => {
    mocked(usePaginatedPersonalAccessTokens).mockReturnValue({
      data: undefined,
      isLoading: true,
    } as unknown as ReturnType<typeof usePaginatedPersonalAccessTokens>);
  },
};
