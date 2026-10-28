import { Meta, StoryObj } from "@storybook/nextjs";
import { SSOLoginRequiredSheet } from "./SSOLoginRequired";

const meta = {
  component: SSOLoginRequiredSheet,
} satisfies Meta<typeof SSOLoginRequiredSheet>;

export default meta;
type Story = StoryObj<typeof meta>;

export const WithSSOLogin: Story = {
  args: {
    ssoLoginHref:
      "/api/auth/login?organizationId=org_01HXYZ&returnTo=%2Ft%2Facme",
  },
};

export const LogOutOnly: Story = {};
