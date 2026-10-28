import { Meta, StoryObj } from "@storybook/nextjs";
import { fn, mocked, screen, userEvent } from "storybook/test";
import {
  useGenerateDirectorySyncConfigurationLink,
  useGetDirectorySync,
} from "api/directorySync";
import type { DirectoryResponse } from "generatedApi";
import { ConnectDirectoryDialog } from "./ConnectDirectoryDialog";

function mockDirectory(directory: DirectoryResponse | null) {
  mocked(useGetDirectorySync).mockReturnValue({
    data: { directory, enabled: false, mirrored: false },
    isLoading: false,
    error: undefined,
  });
}

const meta = {
  component: ConnectDirectoryDialog,
  args: { teamId: 1, onClose: fn() },
  beforeEach: () => {
    // The portal opens in a new tab, which a story should not do.
    const realOpen = window.open;
    // The dialog opens the tab on the click and navigates it once the link
    // arrives, so the stub has to look enough like a window for that.
    window.open = fn(() => ({
      location: { href: "" },
      close: fn(),
    })) as unknown as typeof window.open;
    mocked(useGenerateDirectorySyncConfigurationLink).mockReturnValue(
      fn(async () => ({ link: "https://portal.workos.com/example" })) as any,
    );
    mockDirectory(null);
    return () => {
      window.open = realOpen;
    };
  },
} satisfies Meta<typeof ConnectDirectoryDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

// The dialog only starts reporting the directory's state once the member has
// been handed off to the portal, so every status story goes through the button.
const openPortal: Story["play"] = async () => {
  await userEvent.click(
    await screen.findByRole("button", { name: "Configure Identity Provider" }),
  );
};

export const WaitingForConnection: Story = {
  play: openPortal,
};

// A directory the identity provider created but has not linked is the same
// wait as one it has not created at all, so this reads like the story above.
export const WaitingWithUnlinkedDirectory: Story = {
  beforeEach: () => {
    mockDirectory({
      id: "directory_1",
      name: "Acme Okta Directory",
      type: "okta scim v2.0",
      state: "unlinked",
      linked: false,
    });
  },
  play: openPortal,
};

export const InvalidCredentials: Story = {
  beforeEach: () => {
    mockDirectory({
      id: "directory_1",
      name: "Acme Okta Directory",
      type: "okta scim v2.0",
      state: "invalid_credentials",
      linked: false,
    });
  },
  play: openPortal,
};

export const Linked: Story = {
  beforeEach: () => {
    mockDirectory({
      id: "directory_1",
      name: "Acme Okta Directory",
      type: "okta scim v2.0",
      state: "linked",
      linked: true,
    });
  },
  play: openPortal,
};
