import { DotsVerticalIcon } from "@radix-ui/react-icons";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Link } from "@ui/Link";
import { Menu, MenuItem } from "@ui/Menu";
import {
  useDeleteProfileEmail,
  useResendProfileEmailVerification,
  useUpdatePrimaryProfileEmail,
} from "api/profile";
import { useTeams } from "api/teams";
import { useState } from "react";
import { MemberEmailResponse } from "generatedApi";

// The backend rejects deleting an email that ties the account to a
// Vercel-managed team with the code `EmailBoundToVercelTeam(<team slug>)`.
// Parse out the slug so we can link the user to that team.
function vercelBoundTeamSlug(code: unknown): string | undefined {
  if (typeof code !== "string") {
    return undefined;
  }
  const match = code.match(/^EmailBoundToVercelTeam\((.+)\)$/);
  return match?.[1];
}

// Resolves the bound team's slug to its display name. Kept in its own component
// so `useTeams` (and its router dependency) only runs when the delete is
// actually rejected for this reason, not on every email row.
function VercelBoundTeamError({ slug }: { slug: string }) {
  const { teams } = useTeams();
  // Fall back to the slug if the bound team isn't in the member's team list.
  const name = teams?.find((team) => team.slug === slug)?.name ?? slug;
  return (
    <span>
      This email connects your account to the Vercel-managed team{" "}
      <Link href={`/t/${slug}/settings/members`}>{name}</Link>. Leave the Vercel
      team before removing this email.
    </span>
  );
}

export function EmailListItem({ email }: { email: MemberEmailResponse }) {
  const deleteEmail = useDeleteProfileEmail();
  const updatePrimaryEmail = useUpdatePrimaryProfileEmail();
  const resentEmailVerification = useResendProfileEmailVerification();

  const [showDeleteConfirmation, setShowDeleteConfirmation] = useState(false);
  const [error, setError] = useState<string>();
  const [boundTeamSlug, setBoundTeamSlug] = useState<string>();

  return (
    <div className="flex flex-wrap items-center justify-between gap-4 border-b py-2 last:border-b-0">
      <div className="flex grow items-center gap-2">
        <div className="grow">{email.email}</div>
        {email.isPrimary && (
          <div className="rounded-sm border p-1 text-xs">Primary</div>
        )}
        <div className="rounded-sm border p-1 text-xs">
          {email.isVerified ? "Verified" : "Unverified"}
        </div>
      </div>
      <Menu
        placement="bottom-end"
        buttonProps={{
          variant: "neutral",
          icon: <DotsVerticalIcon />,
          "aria-label": "Email options",
          size: "xs",
        }}
      >
        <MenuItem
          action={() => updatePrimaryEmail({ email: email.email })}
          disabled={!email.isVerified || email.isPrimary}
          tip={
            !email.isVerified
              ? "This email is not verified."
              : email.isPrimary
                ? "This is already your primary email."
                : undefined
          }
          tipSide="right"
        >
          Set as primary
        </MenuItem>
        {!email.isVerified ? (
          <MenuItem
            action={() => resentEmailVerification({ email: email.email })}
          >
            Resend verification email
          </MenuItem>
        ) : null}
        <MenuItem
          action={() => setShowDeleteConfirmation(true)}
          disabled={email.isPrimary}
          variant="danger"
          tip={
            email.isPrimary
              ? "You cannot delete your primary email."
              : undefined
          }
          tipSide="right"
        >
          Delete
        </MenuItem>
      </Menu>
      {showDeleteConfirmation && (
        <ConfirmationDialog
          onClose={() => {
            setShowDeleteConfirmation(false);
            setError(undefined);
            setBoundTeamSlug(undefined);
          }}
          onConfirm={async () => {
            try {
              await deleteEmail({ email: email.email });
              setShowDeleteConfirmation(false);
            } catch (e: any) {
              const slug = vercelBoundTeamSlug(e?.code);
              if (slug !== undefined) {
                setBoundTeamSlug(slug);
                setError(undefined);
              } else {
                setBoundTeamSlug(undefined);
                setError(e.message);
              }
              throw e;
            }
          }}
          confirmText="Delete"
          variant="danger"
          dialogTitle="Delete Email"
          dialogBody={
            <p>Deleting this email will remove it from your account.</p>
          }
          error={
            boundTeamSlug !== undefined ? (
              <VercelBoundTeamError slug={boundTeamSlug} />
            ) : (
              error
            )
          }
          validationText={email.email}
        />
      )}
    </div>
  );
}
