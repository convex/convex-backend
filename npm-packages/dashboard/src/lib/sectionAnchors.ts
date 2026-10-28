type SectionAnchor = { id: string; label: string };

export const DEPLOYMENT_SETTINGS_SECTIONS = {
  deployKeys: { id: "deploy-keys", label: "Deploy Keys" },
  deploymentReference: {
    id: "deployment-reference",
    label: "Deployment Reference",
  },
  sendLogsToClient: {
    id: "send-logs-to-client",
    label: "Send Logs to Client",
  },
  dashboardEditConfirmation: {
    id: "dashboard-edit-confirmation",
    label: "Dashboard Edit Confirmation",
  },
  deploymentExpiry: { id: "deployment-expiry", label: "Deployment Expiry" },
  pauseDeployment: { id: "pause-deployment", label: "Pause Deployment" },
  transferDeployment: {
    id: "transfer-deployment",
    label: "Transfer Deployment",
  },
  deleteDeployment: { id: "delete-deployment", label: "Delete Deployment" },
} satisfies Record<string, SectionAnchor>;

export const PROJECT_SETTINGS_SECTIONS = {
  editProject: { id: "project-form", label: "Edit Project" },
  projectAdmins: { id: "project-roles", label: "Project Admins" },
  projectUsage: { id: "project-usage", label: "Project Usage" },
  customDomains: { id: "custom-domains", label: "Custom Domains" },
  previewDeployKeys: {
    id: "preview-deploy-keys",
    label: "Preview Deploy Keys",
  },
  authorizedApplications: {
    id: "applications",
    label: "Authorized Applications",
  },
  environmentVariables: {
    id: "env-vars",
    label: "Default Environment Variables",
  },
  transferProject: { id: "transfer-project", label: "Transfer Project" },
  deleteProject: { id: "delete-project", label: "Delete Project" },
} satisfies Record<string, SectionAnchor>;

export const TEAM_SETTINGS_SECTIONS = {
  teamName: { id: "team-name", label: "Team Name" },
  teamSlug: { id: "team-slug", label: "Team Slug" },
  teamId: { id: "team-id", label: "Team ID" },
  defaultRegion: { id: "default-region", label: "Default Region" },
  deleteTeam: { id: "delete-team", label: "Delete Team" },
  inviteMember: { id: "invite-member", label: "Invite Member" },
} satisfies Record<string, SectionAnchor>;

export const PROFILE_SECTIONS = {
  profileInformation: {
    id: "profile-information",
    label: "Profile Information",
  },
  emails: { id: "emails", label: "Emails" },
  security: { id: "security", label: "Security" },
  personalAccessTokens: {
    id: "personal-access-tokens",
    label: "Personal Access Tokens",
  },
  dashboardTheme: { id: "dashboard-theme", label: "Dashboard Theme" },
  discordAccounts: { id: "discord-accounts", label: "Discord Accounts" },
  deleteAccount: { id: "delete-account", label: "Delete Account" },
} satisfies Record<string, SectionAnchor>;

// Rendered on the Profile page only when the member has a team available to
// join, so it stays out of PROFILE_SECTIONS, which the command palette turns
// into navigation targets unconditionally.
export const PROFILE_AVAILABLE_TEAMS_SECTION: SectionAnchor = {
  id: "available-teams",
  label: "Available Teams",
};
