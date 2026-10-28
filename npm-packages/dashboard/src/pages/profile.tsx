import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { TextInput } from "@ui/TextInput";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { PageContent } from "@common/elements/PageContent";

import { RadioGroup } from "@headlessui/react";
import classNames from "classnames";
import {
  CheckCircledIcon,
  DiscordLogoIcon,
  EnvelopeClosedIcon,
  Half2Icon,
  LockClosedIcon,
  PersonIcon,
  TrashIcon,
} from "@radix-ui/react-icons";
import { KeyIcon, UserGroupIcon } from "@heroicons/react/24/outline";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { logout } from "lib/logout";
import Head from "next/head";
import {
  useDeleteAccount,
  useProfile,
  useProfileEmails,
  useUpdateProfileName,
} from "api/profile";
import { useState } from "react";
import { Emails } from "components/profile/Emails";
import { AvailableTeams } from "components/profile/AvailableTeams";
import { DiscordAccounts } from "components/profile/DiscordAccounts";
import { MemberResponse } from "generatedApi";
import { Loading } from "@ui/Loading";
import { useTheme } from "next-themes";
import { Security } from "components/profile/Security";
import { PersonalAccessTokens } from "components/profile/PersonalAccessTokens";
import { useDirectorySyncOffers } from "api/directorySync";
import {
  PROFILE_AVAILABLE_TEAMS_SECTION,
  PROFILE_SECTIONS,
} from "lib/sectionAnchors";
import { SettingsLayout, type SettingsSection } from "elements/SettingsLayout";

export { getServerSideProps } from "lib/ssr";

// The Available Teams section renders only when there is a team to join, so
// `sections` is assembled per render rather than being a module constant.
function profileSections(hasAvailableTeams: boolean): SettingsSection[] {
  return [
    { ...PROFILE_SECTIONS.profileInformation, Icon: PersonIcon },
    { ...PROFILE_SECTIONS.emails, Icon: EnvelopeClosedIcon },
    ...(hasAvailableTeams
      ? [{ ...PROFILE_AVAILABLE_TEAMS_SECTION, Icon: UserGroupIcon }]
      : []),
    { ...PROFILE_SECTIONS.security, Icon: LockClosedIcon },
    { ...PROFILE_SECTIONS.personalAccessTokens, Icon: KeyIcon },
    { ...PROFILE_SECTIONS.dashboardTheme, Icon: Half2Icon },
    { ...PROFILE_SECTIONS.discordAccounts, Icon: DiscordLogoIcon },
    { ...PROFILE_SECTIONS.deleteAccount, Icon: TrashIcon },
  ];
}

export function Profile() {
  const profile = useProfile();
  const emails = useProfileEmails();
  const availableTeams = useDirectorySyncOffers();

  const [showConfirmation, setShowConfirmation] = useState(false);
  const deleteAccount = useDeleteAccount();
  const [deleteAccountError, setDeleteAccountError] = useState<
    string | undefined
  >();
  const deleteAccountBody = (
    <p className="max-w-prose text-sm text-pretty">
      To delete your account, your account must match the following criteria:
      <ul className="mt-2 list-inside list-disc">
        <li>You must not be the only admin in teams with other members.</li>
        <li>
          You must delete all projects in teams where you are the only member.
        </li>
      </ul>
    </p>
  );

  return (
    <>
      <Head>
        <title>Profile | Convex Dashboard</title>
      </Head>
      <PageContent>
        <SettingsLayout
          title="Profile"
          sections={profileSections(!!availableTeams?.length)}
          contentReady={!!(emails && profile)}
        >
          {emails && profile ? (
            <>
              <Sheet
                id={PROFILE_SECTIONS.profileInformation.id}
                className="flex w-full flex-col gap-4"
              >
                <h3>Profile information</h3>
                <ProfileForm profile={profile} />
              </Sheet>

              <Emails emails={emails} />

              {availableTeams && availableTeams.length > 0 && (
                <AvailableTeams offers={availableTeams} />
              )}

              <Security />

              <PersonalAccessTokens />

              <ToggleDarkMode />

              <DiscordAccounts />

              <Sheet
                id={PROFILE_SECTIONS.deleteAccount.id}
                className="flex flex-col gap-4"
              >
                <h3>Delete Account</h3>
                {deleteAccountBody}
                <Button
                  variant="danger"
                  className="w-fit"
                  onClick={() => setShowConfirmation(true)}
                >
                  Delete account
                </Button>
                {showConfirmation && (
                  <ConfirmationDialog
                    onClose={() => setShowConfirmation(false)}
                    onConfirm={async () => {
                      try {
                        document.cookie = "";
                        window.localStorage.clear();
                        await deleteAccount();
                        // The account deletion also deletes the WorkOS user, so
                        // skip the hosted WorkOS logout (which would render a
                        // blank page for the now-nonexistent user) and just
                        // clear our session cookie before sending them to login.
                        logout(
                          "/api/auth/logout?sessionDeleted=true&returnTo=/login",
                        );
                      } catch (e: any) {
                        setDeleteAccountError(e.message);
                        throw e;
                      }
                    }}
                    confirmText="Delete account"
                    dialogTitle="Delete Account"
                    error={deleteAccountError}
                    dialogBody={deleteAccountBody}
                    validationText="Delete my account"
                  />
                )}
              </Sheet>
            </>
          ) : (
            <Loading className="h-200" fullHeight={false} />
          )}
        </SettingsLayout>
      </PageContent>
    </>
  );
}

function ProfileForm({ profile }: { profile: MemberResponse }) {
  const [name, setName] = useState(profile.name);
  const [isLoading, setIsLoading] = useState(false);
  const updateProfileName = useUpdateProfileName();

  return (
    <div className="flex flex-col gap-4">
      <form
        className="flex flex-col gap-1"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!name) return;
          setIsLoading(true);
          try {
            await updateProfileName({ name });
          } finally {
            setIsLoading(false);
          }
        }}
      >
        <div className="flex items-end gap-2">
          <TextInput
            id="name"
            label="Name"
            value={name || ""}
            onChange={(e) => setName(e.target.value)}
            error={
              name
                ? name.length > 128
                  ? "Name must be at most 128 characters long."
                  : undefined
                : undefined
            }
          />
          <Button
            type="submit"
            disabled={
              name === profile.name || (name ? name.length > 128 : false)
            }
            loading={isLoading}
          >
            Save
          </Button>
        </div>
      </form>
    </div>
  );
}

export default withAuthenticatedPage(Profile);

const themes = [
  {
    title: "Light",
    value: "light",
  },
  { title: "Dark", value: "dark" },
  { title: "System", value: "system" },
];

function ToggleDarkMode() {
  const { theme: currentTheme, setTheme } = useTheme();

  return (
    <Sheet
      id={PROFILE_SECTIONS.dashboardTheme.id}
      className="flex flex-col gap-4"
    >
      <RadioGroup value={currentTheme} onChange={setTheme}>
        <RadioGroup.Label>
          <h3>Dashboard theme</h3>
        </RadioGroup.Label>

        <div className="mt-4 grid grid-cols-1 gap-y-6 sm:grid-cols-3 sm:gap-x-4">
          {themes.map((theme) => (
            <RadioGroup.Option
              key={theme.title}
              value={theme.value}
              className={({ checked }) =>
                classNames(
                  "relative block cursor-pointer rounded-2xl border px-6 py-4 focus:outline-none sm:flex sm:justify-between",
                  checked
                    ? "[--theme-selector-border:var(--border-transparent)]"
                    : "[--theme-selector-border:transparent]",
                  "focus-visible:[--theme-selector-border:var(--border-selected)]",
                  "bg-background-primary/30 hover:bg-background-primary/70 transition-colors shadow-sm",
                  checked
                    ? "bg-background-tertiary"
                    : "bg-background-secondary",
                )
              }
            >
              {({ checked }) => (
                <>
                  <span className="flex flex-1">
                    <span className="flex flex-col">
                      <RadioGroup.Label
                        as="span"
                        className="block text-sm font-medium text-content-primary"
                      >
                        {theme.title}
                      </RadioGroup.Label>
                    </span>
                  </span>
                  <CheckCircledIcon
                    className={classNames(!checked ? "invisible" : "", "mt-1")}
                    aria-hidden="true"
                  />
                  <span
                    className={classNames(
                      "border border-(--theme-selector-border)",
                      "pointer-events-none absolute -inset-px rounded-2xl",
                    )}
                    aria-hidden="true"
                  />
                </>
              )}
            </RadioGroup.Option>
          ))}
        </div>
      </RadioGroup>
    </Sheet>
  );
}
