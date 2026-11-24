import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Sheet } from "@ui/Sheet";
import { cn } from "@ui/cn";
import { TextInput } from "@ui/TextInput";
import { toast } from "@common/lib/utils";
import { Cross2Icon, PlusIcon } from "@radix-ui/react-icons";
import { FormEvent, useState } from "react";
import { flushSync } from "react-dom";
import { useCreateInvite } from "api/invitations";
import { useTeamOrbSubscription } from "api/billing";
import { useTeamEntitlements } from "api/teams";
import { useHasCustomRolePermission, useListCustomRoles } from "api/roles";
import { CUSTOM_ROLE_RESOURCE } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { TeamResponse } from "generatedApi";
import { Link } from "@ui/Link";
import {
  InvitationResponse,
  TeamMember,
} from "@convex-dev/platform/managementApi";
import * as Yup from "yup";
import { InviteRolePicker } from "./InviteRolePicker";
import { TEAM_SETTINGS_SECTIONS } from "lib/sectionAnchors";

type RoleChoice = "admin" | "developer" | "custom";

const emailSchema = Yup.string().email().max(254);

const MAX_INVITES_PER_BATCH = 10;

type InviteRow = {
  id: number;
  email: string;
  role: RoleChoice;
  customRoleIds: number[];
};

export type InviteMemberFormProps = {
  team: TeamResponse;
  members: TeamMember[];
  invites: InvitationResponse[];
  // `undefined` while the permission check is loading; the form renders
  // disabled until it resolves.
  canInvite: boolean | undefined;
};

export function InviteMemberForm({
  team,
  members,
  invites,
  canInvite,
}: InviteMemberFormProps) {
  const inviteAllowed = canInvite === true;
  const { subscription } = useTeamOrbSubscription(team.id);
  const entitlements = useTeamEntitlements(team.id);
  const customRolesEnabled = entitlements?.customRolesEnabled ?? false;
  const canViewCustomRoles = useHasCustomRolePermission(
    team.id,
    "customRole:view",
    CUSTOM_ROLE_RESOURCE,
    true,
  );
  const customRolesAvailable =
    customRolesEnabled && canViewCustomRoles === true;
  const { data: customRolesData } = useListCustomRoles(
    customRolesAvailable ? team.id : undefined,
  );
  const availableCustomRoles = customRolesData?.items ?? [];

  const createInvite = useCreateInvite(team.id, { toastOnSuccess: false });
  const [rows, setRows] = useState<InviteRow[]>([
    { id: 0, email: "", role: "developer", customRoleIds: [] },
  ]);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const memberEmails = new Set(
    members.map((member) => member.email.toLowerCase()),
  );
  const seenEmails = new Set<string>();
  const rowErrors = rows.map(({ email }) => {
    const normalized = email.trim().toLowerCase();
    if (normalized === "") {
      return undefined;
    }
    if (!emailSchema.isValidSync(normalized)) {
      return "Must be a valid email.";
    }
    if (memberEmails.has(normalized)) {
      return "Email is already a member of this team.";
    }
    if (seenEmails.has(normalized)) {
      return "This email is already in the list.";
    }
    seenEmails.add(normalized);
    return undefined;
  });
  // Blank rows are skipped, so a spare empty row doesn't block sending.
  const pendingRows = rows.filter(({ email }) => email.trim() !== "");

  // Members and unexpired invitations count against `maxTeamMembers`, and
  // resending a pending invitation takes no new seat.
  const pendingInviteEmails = new Set(
    invites
      .filter((invite) => !invite.expired)
      .map((invite) => invite.email.toLowerCase()),
  );
  const seatsLeft =
    entitlements === undefined
      ? Infinity
      : Math.max(
          0,
          entitlements.maxTeamMembers -
            members.length -
            pendingInviteEmails.size,
        );
  const seatRowCount = pendingRows.filter(
    ({ email }) => !pendingInviteEmails.has(email.trim().toLowerCase()),
  ).length;
  const overSeatLimit = seatRowCount > seatsLeft;
  const resendRowCount = pendingRows.length - seatRowCount;
  const atSeatLimit = rows.length - resendRowCount >= seatsLeft;
  const atBatchLimit = rows.length >= MAX_INVITES_PER_BATCH;
  const canAddRow = !atSeatLimit && !atBatchLimit;
  const seats = (count: number) => (count === 1 ? "seat" : "seats");

  const canSubmit =
    inviteAllowed &&
    !isSubmitting &&
    pendingRows.length > 0 &&
    !overSeatLimit &&
    rowErrors.every((error) => error === undefined);

  const updateRow = (id: number, update: Partial<InviteRow>) =>
    setRows((current) =>
      current.map((row) => (row.id === id ? { ...row, ...update } : row)),
    );

  // A new row starts with the role of the row above it, since admins usually
  // invite a batch of people into the same role.
  const addRow = () => {
    const last = rows[rows.length - 1];
    const id = Math.max(...rows.map((row) => row.id)) + 1;
    flushSync(() =>
      setRows((current) => [
        ...current,
        { id, email: "", role: last.role, customRoleIds: last.customRoleIds },
      ]),
    );
    document.getElementById(`inviteEmail-${id}`)?.focus();
  };

  const onSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!canSubmit) {
      return;
    }
    setIsSubmitting(true);
    const failedRows: InviteRow[] = [];
    for (const row of pendingRows) {
      const email = row.email.trim();
      try {
        await createInvite(
          row.role === "custom"
            ? { email, role: "custom", customRoles: row.customRoleIds }
            : { email, role: row.role },
        );
      } catch {
        // useCreateInvite already toasted the error.
        failedRows.push(row);
      }
    }
    const sentCount = pendingRows.length - failedRows.length;
    if (sentCount > 0) {
      toast(
        "success",
        sentCount === 1 ? "Invitation sent." : `${sentCount} invitations sent.`,
      );
    }
    // Failed rows stay so they can be retried. Otherwise one empty row keeps
    // the first row's role, so the next batch doesn't have to reselect it.
    setRows(failedRows.length > 0 ? failedRows : [{ ...rows[0], email: "" }]);
    setIsSubmitting(false);
  };

  // Until entitlements load, Custom stays enabled and its list shows loading.
  const customDisabledReason =
    entitlements !== undefined && !customRolesEnabled
      ? "Custom roles aren't available on this team's plan."
      : canViewCustomRoles === false
        ? "You do not have permission to view custom roles."
        : undefined;
  return (
    <Sheet
      id={TEAM_SETTINGS_SECTIONS.inviteMember.id}
      className="min-w-fit text-sm"
    >
      <h3 className="mb-4">Invite Member</h3>
      <form onSubmit={onSubmit} aria-label="Invite team member">
        <Tooltip
          tip={
            !inviteAllowed
              ? permissionDeniedTip(
                  "You do not have permission to invite team members.",
                  "member:invite",
                )
              : undefined
          }
          className="w-full"
        >
          <div className="mb-4 flex w-full flex-col gap-2">
            {rows.map((row, index) => (
              <div key={row.id} className="flex items-start gap-2">
                <div className="min-w-0 grow">
                  <TextInput
                    label="Email"
                    labelHidden={index > 0}
                    placeholder="Email address"
                    // A zero basis keeps the input's intrinsic width out of the
                    // Sheet's `min-w-fit`, so narrow screens don't scroll sideways.
                    className="w-0"
                    type="email"
                    onChange={(e) =>
                      updateRow(row.id, { email: e.target.value })
                    }
                    value={row.email}
                    error={rowErrors[index]}
                    id={`inviteEmail-${row.id}`}
                    aria-label={index === 0 ? "Email" : `Email ${index + 1}`}
                    disabled={!inviteAllowed || isSubmitting}
                  />
                </div>
                {inviteAllowed && (
                  <div className="flex flex-col gap-1">
                    {index === 0 && (
                      <span
                        className="text-left text-sm text-content-primary"
                        aria-hidden
                      >
                        Role
                      </span>
                    )}
                    <InviteRolePicker
                      label={index === 0 ? "Role" : `Role ${index + 1}`}
                      value={
                        row.role === "custom"
                          ? { role: "custom", customRoleIds: row.customRoleIds }
                          : { role: row.role }
                      }
                      onChange={(value) =>
                        updateRow(row.id, {
                          role: value.role,
                          customRoleIds:
                            value.role === "custom" ? value.customRoleIds : [],
                        })
                      }
                      disabled={isSubmitting}
                      customRoles={availableCustomRoles}
                      customRolesLoading={
                        customDisabledReason === undefined &&
                        customRolesData === undefined
                      }
                      customRolesDisabledReason={customDisabledReason}
                      teamSlug={team.slug}
                    />
                  </div>
                )}
                {inviteAllowed && (
                  // Reserves the remove button's column so the inputs don't
                  // shift when a second row appears.
                  <div
                    className={cn(
                      "flex h-[34px] w-7 shrink-0 items-center justify-center",
                      index === 0 && "mt-6",
                    )}
                  >
                    {rows.length > 1 && (
                      <Button
                        variant="neutral"
                        inline
                        size="xs"
                        icon={<Cross2Icon />}
                        aria-label={`Remove email ${index + 1}`}
                        tip="Remove"
                        disabled={isSubmitting}
                        onClick={() =>
                          setRows((current) =>
                            current.filter(({ id }) => id !== row.id),
                          )
                        }
                      />
                    )}
                  </div>
                )}
              </div>
            ))}
          </div>
        </Tooltip>
        {inviteAllowed && atBatchLimit && !atSeatLimit && (
          <p className="mb-4 text-xs text-content-secondary">
            You can invite up to {MAX_INVITES_PER_BATCH} people at a time.
          </p>
        )}
        {inviteAllowed && (overSeatLimit || atSeatLimit) && (
          <p
            className={cn(
              "mb-4 text-xs",
              overSeatLimit ? "text-content-error" : "text-content-secondary",
            )}
          >
            {overSeatLimit
              ? `This team has ${seatsLeft} ${seats(seatsLeft)} left. Remove ${seatRowCount - seatsLeft} ${seatRowCount - seatsLeft === 1 ? "email" : "emails"} to send.`
              : `${rows.length === 1 ? "This invite uses" : "These invites use"} the team's last ${seatsLeft === 1 ? "seat" : `${seatsLeft} seats`}.`}{" "}
            <Link href={`/t/${team.slug}/settings/billing`}>
              Get more seats
            </Link>
          </p>
        )}
        <div className="flex flex-wrap items-end justify-between gap-4">
          {subscription?.plan.seatPrice ? (
            <p className="max-w-prose text-xs text-pretty text-content-secondary">
              Once each member accepts a team invitation,{" "}
              <span className="font-semibold">
                your bill will increase by ${subscription.plan.seatPrice} per
                month.
              </span>{" "}
              There may also be an immediate charge for a prorated amount based
              on the remaining time in your current billing cycle.
            </p>
          ) : (
            <span />
          )}
          <div className="ml-auto flex gap-2">
            <Button
              variant="neutral"
              icon={<PlusIcon />}
              onClick={addRow}
              disabled={!inviteAllowed || isSubmitting || !canAddRow}
            >
              Invite another email
            </Button>
            <Button
              disabled={!canSubmit}
              loading={isSubmitting}
              tip={
                !inviteAllowed
                  ? permissionDeniedTip(
                      "You do not have permission to invite team members.",
                      "member:invite",
                    )
                  : undefined
              }
              type="submit"
              aria-label="submit"
            >
              {pendingRows.length > 1
                ? `Send ${pendingRows.length} Invites`
                : "Send Invite"}
            </Button>
          </div>
        </div>
      </form>
    </Sheet>
  );
}
