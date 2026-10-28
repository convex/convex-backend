import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked, screen, userEvent } from "storybook/test";
import { useEffect, type ContextType } from "react";
import type { FunctionReturnType } from "convex/server";
import type { Value } from "convex/values";
import udfs from "@common/udfs";
import {
  DeploymentInfoContext,
  MaybeConnectedDeploymentContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import type {
  AnalyzedModuleFunction,
  Module,
  UdfType,
} from "system-udfs/convex/_system/frontend/common";
import type { FileMetadata } from "system-udfs/convex/_system/frontend/fileStorageV2";
import { flagDefaults, useLaunchDarkly } from "hooks/useLaunchDarkly";
import { useCurrentTeam, useTeams } from "api/teams";
import {
  useCurrentProject,
  useInfiniteProjects,
  useProjectById,
} from "api/projects";
import {
  useCurrentDeployment,
  useDeployments,
  useInfiniteDeployments,
} from "api/deployments";
import { useProfile } from "api/profile";
import { ignoredDirectorySyncTeamsKey } from "hooks/useIgnoredDirectorySyncTeams";
import {
  useDirectorySyncOffers,
  useJoinDirectorySyncedTeam,
} from "api/directorySync";
import type {
  DirectorySyncOffer,
  PlatformDeploymentResponse,
} from "generatedApi";
import {
  CommandPalette,
  useCommandPaletteAnchor,
  useCommandPaletteInitialPages,
  useCommandPaletteOpen,
} from "./CommandPalette";

const mockTeam = {
  id: 2,
  creator: 1,
  slug: "acme",
  name: "Acme Corp",
  suspended: false,
  referralCode: "ACME01",
  referredBy: null,
};

const mockProject = {
  id: 7,
  teamId: mockTeam.id,
  name: "My amazing app",
  slug: "my-amazing-app",
  createTime: Date.now(),
  prodDeploymentName: "musical-otter-456",
  devDeploymentName: "happy-capybara-123",
} as NonNullable<ReturnType<typeof useCurrentProject>>;

const otherProjects = [
  { ...mockProject },
  {
    id: 8,
    teamId: mockTeam.id,
    name: "Marketing site",
    slug: "marketing-site",
    createTime: Date.now(),
  },
  {
    id: 9,
    teamId: mockTeam.id,
    name: "Internal tools",
    slug: "internal-tools",
    createTime: Date.now(),
  },
] as NonNullable<ReturnType<typeof useCurrentProject>>[];

const devDeployment: PlatformDeploymentResponse = {
  id: 11,
  name: "happy-capybara-123",
  deploymentType: "dev",
  kind: "cloud",
  isDefault: true,
  projectId: mockProject.id,
  creator: 1,
  createTime: 0,
  class: "s256",
  deploymentUrl: "https://happy-capybara-123.convex.cloud",
  reference: "dev/nicolas",
  region: "aws-us-east-1",
};

const DEPLOYMENT_URI = "/t/acme/my-amazing-app/happy-capybara-123";

const mockProfile = {
  id: 1,
  name: "Nicolas Ettlin",
  email: "nicolas@acme.dev",
};

// A team the member isn't in, offered because its directory lists one of their
// verified emails.
const directorySyncOffer: DirectorySyncOffer = {
  teamId: 14,
  teamName: "Example Org",
  email: "nicolas@example.org",
};

// The palette's open state lives in a global (so the header trigger can open
// it from anywhere); flip it on when the story mounts so the dialog renders.
function OpenCommandPalette() {
  const [, setOpen] = useCommandPaletteOpen();
  useEffect(() => {
    setOpen(true);
    return () => setOpen(false);
  }, [setOpen]);
  return <CommandPalette />;
}

const meta = {
  component: CommandPalette,
  parameters: {
    layout: "fullscreen",
    // The palette is a focus-trapping Radix dialog rendered over an empty
    // canvas, which trips the automated a11y checks meant for full pages.
    a11y: { test: "todo" },
  },
  render: () => <OpenCommandPalette />,
  beforeEach: () => {
    mocked(useLaunchDarkly).mockReturnValue(flagDefaults);
    mocked(useTeams).mockReturnValue({
      selectedTeamSlug: mockTeam.slug,
      teams: [mockTeam],
    });
    mocked(useCurrentTeam).mockReturnValue(mockTeam);
    mocked(useProfile).mockReturnValue(mockProfile);
    mocked(useDirectorySyncOffers).mockReturnValue([]);
    // The ProjectSelector stories persist ignored offers to localStorage, which
    // the test runner shares across stories; reset it so the offer still shows.
    window.localStorage.removeItem(
      ignoredDirectorySyncTeamsKey(mockProfile.id),
    );
    mocked(useJoinDirectorySyncedTeam).mockReturnValue(async () => ({
      teamId: directorySyncOffer.teamId,
      teamSlug: "example-org",
    }));
    // These hooks are server-backed: their remote rows bypass the palette's
    // client-side filter, so the results must already reflect the query.
    // Filter the mock data by the search argument to match that behavior —
    // otherwise every deployment/project matches every query.
    mocked(useInfiniteProjects).mockImplementation(
      (_teamId, searchQuery = "") => {
        const q = searchQuery.trim().toLowerCase();
        const projects = otherProjects.filter(
          (p) => !q || `${p.name} ${p.slug}`.toLowerCase().includes(q),
        );
        return {
          projects,
          isLoading: false,
          isLoadingMore: false,
          hasMore: false,
          loadMore: () => {},
          debouncedQuery: searchQuery,
          pageSize: 20,
        };
      },
    );
    mocked(useInfiniteDeployments).mockImplementation(
      (_teamId, searchQuery = "") => {
        const q = searchQuery.trim().toLowerCase();
        const deployments = [devDeployment].filter(
          (d) =>
            !q ||
            `${"reference" in d ? d.reference : ""} ${d.name}`
              .toLowerCase()
              .includes(q),
        );
        return {
          deployments,
          isLoading: false,
          isLoadingMore: false,
          hasMore: false,
          loadMore: () => {},
          debouncedQuery: searchQuery,
          pageSize: 25,
        };
      },
    );
    mocked(useDeployments).mockReturnValue({
      deployments: [devDeployment],
      isLoading: false,
    });
    mocked(useProjectById).mockReturnValue({
      project: mockProject,
      isLoading: false,
      error: undefined,
    });
  },
} satisfies Meta<typeof CommandPalette>;

export default meta;
type Story = StoryObj<typeof meta>;

// The root page while viewing a deployment: current page, the deployment's
// pages, the project, the team, and the account/help groups.
export const InsideDeployment: Story = {
  parameters: {
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/data",
        route: "/t/[team]/[project]/[deploymentName]/data",
        asPath: "/t/acme/my-amazing-app/happy-capybara-123/data",
        query: {
          team: "acme",
          project: "my-amazing-app",
          deploymentName: "happy-capybara-123",
        },
      },
    },
  },
  beforeEach: () => {
    mocked(useCurrentProject).mockReturnValue(mockProject);
    mocked(useCurrentDeployment).mockReturnValue(devDeployment);
  },
};

// The root page from a team-level page, with no current project or deployment:
// only the project search, team, and account/help groups.
export const TeamLevel: Story = {
  parameters: {
    nextjs: {
      router: {
        pathname: "/t/[team]",
        route: "/t/[team]",
        asPath: "/t/acme",
        query: { team: "acme" },
      },
    },
  },
  beforeEach: () => {
    mocked(useCurrentProject).mockReturnValue(undefined);
    mocked(useCurrentDeployment).mockReturnValue(undefined);
  },
};

export const SearchLoading: Story = {
  parameters: InsideDeployment.parameters,
  beforeEach: () => {
    mocked(useCurrentProject).mockReturnValue(mockProject);
    mocked(useCurrentDeployment).mockReturnValue(devDeployment);
    const pending = {
      isLoading: true,
      isLoadingMore: false,
      hasMore: false,
      loadMore: () => {},
      debouncedQuery: "",
    };
    mocked(useInfiniteProjects).mockReturnValue({
      ...pending,
      projects: [],
      pageSize: 20,
    });
    mocked(useInfiniteDeployments).mockReturnValue({
      ...pending,
      deployments: [],
      pageSize: 25,
    });
  },
  play: async () => {
    await userEvent.type(await screen.findByRole("combobox"), "checkout");
  },
};

// --- Switch Team page --------------------------------------------------------

// The palette drilled onto its "Switch Team" page.
function SwitchTeamPalette() {
  const [, setOpen] = useCommandPaletteOpen();
  const [, setInitialPages] = useCommandPaletteInitialPages();
  useEffect(() => {
    setInitialPages([{ type: "teams" }]);
    setOpen(true);
    return () => setOpen(false);
  }, [setOpen, setInitialPages]);
  return <CommandPalette />;
}

// Teams the member can join without an invitation get their own "Available
// Teams" section above the ones they belong to, as dashed "Join <team>"
// entries. Selecting one opens the join prompt (see the ProjectSelector
// stories, which mount that modal the way `_app` does).
export const SwitchTeamWithJoinOffer: Story = {
  parameters: TeamLevel.parameters,
  render: () => <SwitchTeamPalette />,
  beforeEach: () => {
    mocked(useCurrentProject).mockReturnValue(undefined);
    mocked(useCurrentDeployment).mockReturnValue(undefined);
    mocked(useDirectorySyncOffers).mockReturnValue([directorySyncOffer]);
  },
};

// --- Deployment menu (the header's deployment switcher) ----------------------

// The deployment switcher in the header opens the palette anchored beneath its
// trigger, drilled straight onto the project's "Switch Deployment" page. This
// renders that anchored menu the way it appears in the app: a compact popover
// attached under a stand-in trigger, showing the Project Settings shortcut
// (contextual), the create-deployment actions, and the project's deployments.
function DeploymentSwitcherMenu() {
  const [, setOpen] = useCommandPaletteOpen();
  const [, setAnchor] = useCommandPaletteAnchor();
  const [, setInitialPages] = useCommandPaletteInitialPages();
  useEffect(() => {
    setInitialPages([{ type: "deployments", project: mockProject }]);
    setAnchor({ left: 16, top: 56, source: "deployment-switcher" });
    setOpen(true);
    return () => {
      setOpen(false);
      setAnchor(null);
    };
  }, [setOpen, setAnchor, setInitialPages]);
  return (
    <div className="h-screen bg-background-primary">
      <div className="flex h-14 items-center border-b bg-background-secondary px-4">
        <div className="flex h-9 items-center gap-2 rounded-full border bg-background-primary px-4 text-sm font-medium text-content-primary">
          <span className="font-mono font-normal">dev/nicolas</span>
        </div>
      </div>
      <CommandPalette />
    </div>
  );
}

// The Switch Deployment menu anchored under the header's deployment switcher.
export const DeploymentMenu: Story = {
  parameters: {
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/data",
        route: "/t/[team]/[project]/[deploymentName]/data",
        asPath: "/t/acme/my-amazing-app/happy-capybara-123/data",
        query: {
          team: "acme",
          project: "my-amazing-app",
          deploymentName: "happy-capybara-123",
        },
      },
    },
  },
  render: () => <DeploymentSwitcherMenu />,
  beforeEach: () => {
    mocked(useCurrentProject).mockReturnValue(mockProject);
    mocked(useCurrentDeployment).mockReturnValue(devDeployment);
  },
};

// A project with nothing provisioned yet: the menu is just the two dashed
// create-deployment placeholders.
export const DeploymentMenuNothingProvisioned: Story = {
  ...DeploymentMenu,
  beforeEach: () => {
    mocked(useCurrentProject).mockReturnValue(mockProject);
    mocked(useCurrentDeployment).mockReturnValue(undefined);
    mocked(useDeployments).mockReturnValue({
      deployments: [],
      isLoading: false,
    });
  },
};

// --- Data-plane search (tables, functions, and lookup-by-ID) -----------------

// Example document IDs, precomputed so the story doesn't reimplement Convex's
// ID encoding. Each decodes (via id-encoding, which getReferencedTableName
// uses) to the table number it's mapped to below.
const MESSAGE_ID = "m57068j1c1zsxfewzcd3jp3qjttx99vg"; // table 10017
const STORAGE_ID = "k570e9j5cj1t5gf0zwf3tq3vkawxhqr0"; // table 10009
const SCHEDULED_ID = "nx70paj9d23tdhf40ch42r3zktyxrddc"; // table 10031

// getTableMapping exposes the two previewable system tables under their
// internal names; everything else is a user table.
const TABLE_MAPPING: Record<number, string> = {
  10017: "messages",
  10024: "users",
  10009: "_file_storage",
  10031: "_scheduled_jobs",
};

const messageDoc: Record<string, Value> = {
  _id: MESSAGE_ID,
  _creationTime: 1_700_000_000_000,
  author: "Alice",
  body: "Hello from the command palette!",
};

const scheduledDoc: Record<string, Value> = {
  _id: SCHEDULED_ID,
  _creationTime: 1_700_000_000_000,
  name: "messages.js:sendDigest",
  args: [{ template: "weekly-digest-v3", dryRun: false }],
  scheduledTime: 1_700_000_600_000,
  state: { kind: "pending" },
};

const storageFile: FileMetadata = {
  _id: STORAGE_ID as FileMetadata["_id"],
  _creationTime: 1_700_000_000_000,
  contentType: "image/png",
  sha256: "3a7bd3e2360a3d29eea436fcfb7e44c735d117c42d1c1835420b6b9942dd4f1b",
  size: 20_480,
  url: "https://example.convex.cloud/api/storage/example.png",
};

const docsById: Record<string, Value> = {
  [MESSAGE_ID]: messageDoc,
  [SCHEDULED_ID]: scheduledDoc,
};

function makeAnalyzedFunction(
  name: string,
  udfType: UdfType,
): AnalyzedModuleFunction {
  return { name, udfType, visibility: { kind: "public" } };
}

// The `modules.list` shape: [modulePath, Module][] for the current component.
const modules: [string, Module][] = [
  [
    "messages",
    {
      functions: [
        makeAnalyzedFunction("list", "Query"),
        makeAnalyzedFunction("send", "Mutation"),
      ],
      sourcePackageId: "storybook",
    },
  ],
  [
    "users",
    {
      functions: [makeAnalyzedFunction("getCurrentUser", "Query")],
      sourcePackageId: "storybook",
    },
  ],
  [
    "http",
    {
      functions: [
        makeAnalyzedFunction("GET /messages", "HttpAction"),
        makeAnalyzedFunction("POST /webhooks/stripe", "HttpAction"),
      ],
      sourcePackageId: "storybook",
    },
  ],
];

const components = [
  {
    id: "waitlist",
    name: "waitlist",
    path: "waitlist",
    args: {},
    state: "active",
  },
  { id: "email", name: "email", path: "email", args: {}, state: "active" },
] as FunctionReturnType<typeof udfs.components.list>;

const dataPlaneClient = mockConvexReactClient()
  .registerQueryFake(udfs.getTableMapping.default, () => TABLE_MAPPING)
  .registerQueryFake(udfs.modules.list, () => modules)
  .registerQueryFake(udfs.components.list, () => components)
  .registerQueryFake(
    udfs.convexSiteUrl.default,
    () => "https://happy-capybara-123.convex.site",
  )
  .registerQueryFake(udfs.fileStorageV2.getFile, ({ storageId }) =>
    storageId === STORAGE_ID ? storageFile : null,
  )
  .registerQueryFake(udfs.getById.default, ({ id }) => docsById[id] ?? null);

const deploymentInfo = {
  ...mockDeploymentInfo,
  deploymentsURI: DEPLOYMENT_URI,
};

const connectedDeployment = {
  deployment: {
    client: dataPlaneClient,
    httpClient: {} as never,
    deploymentUrl: devDeployment.deploymentUrl,
    adminKey: "STORYBOOK-FAKE-KEY",
    deploymentName: devDeployment.name,
  },
  deploymentName: devDeployment.name,
  loading: false,
  errorKind: "None",
} as ContextType<typeof MaybeConnectedDeploymentContext>;

// A backdrop listing example IDs to copy into the palette, since the palette
// looks documents up by their (unguessable) ID.
function ExampleIdsBackdrop() {
  const rows: [string, string][] = [
    ["Document (messages)", MESSAGE_ID],
    ["Storage file", STORAGE_ID],
    ["Scheduled function", SCHEDULED_ID],
  ];
  return (
    <div className="flex h-screen flex-col items-center justify-center gap-4 bg-background-primary p-8">
      <div className="max-w-xl text-center text-sm text-content-secondary">
        Search a table name like <code>messages</code>, or paste one of these
        example IDs to jump straight to a document, file, or scheduled function:
      </div>
      <div className="flex w-full max-w-xl flex-col gap-2">
        {rows.map(([label, id]) => (
          <div
            key={id}
            className="flex items-center justify-between gap-4 rounded-md border bg-background-secondary px-3 py-2"
          >
            <span className="shrink-0 text-xs text-content-tertiary">
              {label}
            </span>
            <code className="min-w-0 truncate font-mono text-xs text-content-primary select-all">
              {id}
            </code>
          </div>
        ))}
      </div>
    </div>
  );
}

function DataDeploymentPalette() {
  return (
    <DeploymentInfoContext.Provider value={deploymentInfo}>
      <MaybeConnectedDeploymentContext.Provider value={connectedDeployment}>
        <ExampleIdsBackdrop />
        <OpenCommandPalette />
      </MaybeConnectedDeploymentContext.Provider>
    </DeploymentInfoContext.Provider>
  );
}

const dataDeploymentRouter = {
  nextjs: {
    router: {
      pathname: "/t/[team]/[project]/[deploymentName]/data",
      route: "/t/[team]/[project]/[deploymentName]/data",
      asPath: "/t/acme/my-amazing-app/happy-capybara-123/data",
      query: {
        team: "acme",
        project: "my-amazing-app",
        deploymentName: "happy-capybara-123",
      },
    },
  },
};

function setupDataDeployment() {
  mocked(useCurrentProject).mockReturnValue(mockProject);
  mocked(useCurrentDeployment).mockReturnValue(devDeployment);
}

// Interactive: the palette wired to a mock deployment. Search a table or
// function name, or paste one of the example IDs from the backdrop to preview a
// document, storage file, or scheduled function.
export const DataPlaneSearch: Story = {
  parameters: dataDeploymentRouter,
  render: () => <DataDeploymentPalette />,
  beforeEach: setupDataDeployment,
};
