import { fn, mocked } from "storybook/test";
import type { Meta, StoryObj } from "@storybook/nextjs";
import {
  useCreateVanityDomain,
  useDeleteVanityDomain,
} from "api/vanityDomains";
import { useDeployments } from "api/deployments";
import { useHasProjectAdminPermissions } from "api/roles";
import { CustomDomainsForm } from "components/deploymentSettings/CustomDomains";

const deploymentName = "festive-capybara-729";

const meta = {
  component: CustomDomainsForm,
  args: {
    team: {
      id: 1,
      name: "Rap Genie",
      slug: "rapgenie",
      suspended: false,
      referralCode: "RAPGENIE01",
    },
    deployment: {
      id: 11,
      name: deploymentName,
      deploymentType: "prod" as const,
      kind: "cloud" as const,
      isDefault: true,
      projectId: 1,
      creator: 1,
      createTime: Date.now(),
      class: "s256",
      deploymentUrl: `https://${deploymentName}.convex.cloud`,
      reference: "production",
      region: "aws-us-east-1",
    },
    hasEntitlement: true,
    vanityDomains: [
      {
        domain: "api.rapgenie.net",
        requestDestination: "convexCloud" as const,
        deploymentName,
        creationTime: 0,
        verificationTime: 1,
      },
    ],
  },
  parameters: { a11y: { test: "todo" } },
  decorators: [
    (Story) => (
      <div className="max-w-4xl">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof CustomDomainsForm>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  beforeEach() {
    mocked(useCreateVanityDomain).mockReturnValue(fn());
    mocked(useDeleteVanityDomain).mockReturnValue(fn());
    mocked(useDeployments).mockReturnValue({
      deployments: [],
      isLoading: false,
    });
    mocked(useHasProjectAdminPermissions).mockReturnValue(true);
  },
};
