import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Combobox } from "@ui/Combobox";
import { Sheet } from "@ui/Sheet";
import { TextInput } from "@ui/TextInput";
import { toast } from "@common/lib/utils";

import { useFormik } from "formik";
import { useCreateInvite } from "api/invitations";
import { useTeamOrbSubscription } from "api/billing";
import { useTeamEntitlements } from "api/teams";
import { useHasCustomRolePermission, useListCustomRoles } from "api/roles";
import { CUSTOM_ROLE_RESOURCE } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { TeamResponse } from "generatedApi";
import { TeamMember } from "@convex-dev/platform/managementApi";
import * as Yup from "yup";
import { Link } from "@ui/Link";
import { CustomRolesSelector } from "./CustomRolesSelector";
import { TEAM_SETTINGS_SECTIONS } from "lib/sectionAnchors";

type RoleChoice = "admin" | "developer" | "custom";

export type InviteMemberFormProps = {
  team: TeamResponse;
  members: TeamMember[];
  // `undefined` while the permission check is loading; the form renders
  // disabled until it resolves.
  canInvite: boolean | undefined;
};

export function InviteMemberForm({
  team,
  members,
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

  const InviteSchema = Yup.object().shape({
    inviteEmail: Yup.string()
      .email("Must be a valid email.")
      .notOneOf(
        members.map((member) => member.email) || [],
        "Email is already a member of this team.",
      )
      .max(254, "Email must be at most 254 characters long."),
    role: Yup.string().oneOf(["developer", "admin", "custom"]),
    customRoleIds: Yup.array().of(Yup.number()),
  });
  const createInvite = useCreateInvite(team.id);
  const formState = useFormik<{
    inviteEmail: string;
    role: RoleChoice;
    customRoleIds: number[];
  }>({
    initialValues: {
      // Define the form value as "inviteEmail" to avoid browser extensions thinking this is a login form.
      inviteEmail: "",
      role: "developer",
      customRoleIds: [],
    },
    validationSchema: InviteSchema,
    onSubmit: async (values) => {
      if (!inviteAllowed) {
        return;
      }
      const validation = await formState.validateForm();
      if (Object.keys(validation).length) {
        toast("success", "Invalid email", "email");
        return;
      }
      if (values.role === "custom") {
        if (values.customRoleIds.length === 0) {
          return;
        }
        await createInvite({
          email: values.inviteEmail,
          role: "custom",
          customRoles: values.customRoleIds,
        });
      } else {
        await createInvite({
          email: values.inviteEmail,
          role: values.role,
        });
      }
      // Keep the role / customRoleIds selection so an admin inviting a
      // batch of folks into the same role doesn't have to reselect each
      // time. Just clear the email field.
      await formState.setFieldValue("inviteEmail", "");
    },
  });

  const customDisabledReason = !customRolesEnabled
    ? "Custom roles are not enabled for this team."
    : canViewCustomRoles === false
      ? "You do not have permission to view custom roles."
      : undefined;
  const roleOptions = [
    { label: "Admin", value: "admin" as const, disabled: false },
    { label: "Developer", value: "developer" as const, disabled: false },
    {
      label: "Custom",
      value: "custom" as const,
      disabled: customDisabledReason !== undefined,
    },
  ];

  const customSelectionEmpty =
    formState.values.role === "custom" &&
    formState.values.customRoleIds.length === 0;
  const noCustomRolesAvailable =
    formState.values.role === "custom" && availableCustomRoles.length === 0;

  return (
    <Sheet
      id={TEAM_SETTINGS_SECTIONS.inviteMember.id}
      className="min-w-fit text-sm"
    >
      <h3 className="mb-4">Invite Member</h3>
      <form onSubmit={formState.handleSubmit} aria-label="Invite team member">
        <div className="mb-4 flex w-full grow flex-wrap items-start gap-4 sm:flex-nowrap">
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
            <TextInput
              label="Email"
              placeholder="Email address"
              type="email"
              onChange={formState.handleChange}
              value={formState.values.inviteEmail}
              error={
                formState.touched ? formState.errors.inviteEmail : undefined
              }
              onBlur={formState.handleBlur}
              id="inviteEmail"
              aria-label="Email"
              disabled={!inviteAllowed}
            />
          </Tooltip>
          {inviteAllowed && (
            // Combobox renders Label + content as siblings of this wrapper
            // (HeadlessCombobox is a Fragment), so the inner gap controls
            // the label-to-input spacing. Use `gap-1` to match TextInput.
            <div className="flex flex-col gap-1">
              <Combobox
                buttonClasses="w-fit"
                disableSearch
                label="Role"
                labelHidden={false}
                buttonProps={{
                  tip: (
                    <span>
                      Select a{" "}
                      <Link href="https://docs.convex.dev/dashboard/teams#roles-and-permissions">
                        team role
                      </Link>{" "}
                      for the new member.
                    </span>
                  ),
                  tipSide: "top",
                }}
                options={roleOptions}
                selectedOption={formState.values.role}
                setSelectedOption={async (role) => {
                  if (!role) {
                    return;
                  }
                  await formState.setFieldValue("role", role);
                  if (role !== "custom") {
                    await formState.setFieldValue("customRoleIds", []);
                  }
                }}
                Option={({ label, disabled }) =>
                  disabled && customDisabledReason ? (
                    <Tooltip tip={customDisabledReason} side="left">
                      <span>{label}</span>
                    </Tooltip>
                  ) : (
                    <span>{label}</span>
                  )
                }
              />
            </div>
          )}
        </div>
        {inviteAllowed && formState.values.role === "custom" && (
          <div className="mb-4 flex flex-col gap-1">
            <CustomRolesSelector
              availableRoles={availableCustomRoles}
              selectedIds={formState.values.customRoleIds}
              onChange={(ids) => formState.setFieldValue("customRoleIds", ids)}
            />
            {customSelectionEmpty && !noCustomRolesAvailable && (
              <span className="text-xs text-content-secondary">
                Select at least one custom role.
              </span>
            )}
          </div>
        )}
        <div className="flex flex-wrap items-end justify-between gap-4">
          {subscription?.plan.seatPrice ? (
            <p className="max-w-prose text-xs text-pretty text-content-secondary">
              Once a member accepts a team invitation,{" "}
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
          <Button
            disabled={
              !inviteAllowed ||
              !formState.dirty ||
              formState.isSubmitting ||
              !formState.isValid ||
              customSelectionEmpty ||
              noCustomRolesAvailable
            }
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
            className="ml-auto"
          >
            Send Invite
          </Button>
        </div>
      </form>
    </Sheet>
  );
}
