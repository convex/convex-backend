import { Link } from "@ui/Link";

import { useDiscordAccounts, useUnlinkDiscordAccount } from "api/discord";
import { Button } from "@ui/Button";
import { Sheet } from "@ui/Sheet";
import { Cross2Icon } from "@radix-ui/react-icons";
import { DiscordAccount, DiscordAccountDetails } from "generatedApi";
import { PROFILE_SECTIONS } from "lib/sectionAnchors";

function DiscordAccountDetail({
  id,
  details,
}: {
  id: string;
  details: DiscordAccountDetails | null | undefined;
}) {
  if (details === null || details === undefined) {
    return <span className="italic">Account {id}</span>;
  }

  // https://discord.com/developers/docs/reference#image-formatting-cdn-endpoints
  const withoutDiscriminator = details.discriminator === "0";
  const avatarUrl = details.avatar
    ? `https://cdn.discordapp.com/avatars/${id}/${details.avatar}.png`
    : `https://cdn.discordapp.com/embed/avatars/${
        withoutDiscriminator
          ? (BigInt(id) >> BigInt(22)) % BigInt(6)
          : +details.discriminator % 5
      }.png`;

  const primaryText = withoutDiscriminator
    ? details.global_name
      ? details.global_name
      : details.username
    : details.username;
  const secondaryText = withoutDiscriminator
    ? details.global_name
      ? ` ${details.username}`
      : ""
    : `#${details.discriminator}`;

  return (
    <>
      <span className="inline-flex size-9 items-center justify-center overflow-hidden rounded-full bg-background-tertiary select-none">
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img src={avatarUrl} alt="" />
      </span>

      <div>
        <span className="font-medium text-content-primary">
          <span>{primaryText}</span>
        </span>
        <span className="text-content-secondary">
          <span>{secondaryText}</span>
        </span>
      </div>
    </>
  );
}

function DiscordAccountListRow({ account }: { account: DiscordAccount }) {
  const unlinkAccount = useUnlinkDiscordAccount();

  return (
    <div key={account.id} className="flex items-center gap-4 py-4">
      <DiscordAccountDetail id={account.id} details={account.details} />

      <div className="ml-auto">
        <Button
          variant="danger"
          icon={<Cross2Icon />}
          onClick={() => {
            void unlinkAccount({ discordId: account.id });
          }}
        >
          Unlink
        </Button>
      </div>
    </div>
  );
}

export function DiscordAccountsList({
  accounts,
}: {
  accounts: DiscordAccount[];
}) {
  return (
    <div className="flex flex-col divide-y divide-border-transparent">
      {accounts.map((account) => (
        <DiscordAccountListRow account={account} key={account.id} />
      ))}
    </div>
  );
}

export function DiscordAccounts() {
  const accounts = useDiscordAccounts();

  return (
    <Sheet
      id={PROFILE_SECTIONS.discordAccounts.id}
      className="flex flex-col gap-4"
    >
      <h3>Discord Accounts</h3>

      <p className="max-w-lg text-sm">
        Join the{" "}
        <Link href="https://convex.dev/community" target="_blank">
          Convex Discord server
        </Link>{" "}
        to get support, share ideas, and chat with other Convex users and team
        members.
      </p>

      {accounts && <DiscordAccountsList accounts={accounts} />}
    </Sheet>
  );
}
