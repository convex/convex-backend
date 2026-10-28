import { Combobox, Option } from "@ui/Combobox";
import { DateRangePicker } from "@common/elements/DateRangePicker";
import { endOfToday, startOfDay } from "date-fns";
import sortBy from "lodash/sortBy";
import type { DateRange } from "react-day-picker";
import { AuditLogAction } from "api/auditLog";
import { MemberResponse } from "generatedApi";

const AUDIT_LOG_ACTIONS = [
  "team:join",
  "team:create",
  "team:update",
  "team:delete",
  "team:token:create",
  "team:token:update",
  "team:token:delete",
  "team:token:view",
  "team:domain:create",
  "team:domain:delete",
  "team:disableExceedingSpendingLimits",
  "team:applyReferralCode",
  "project:create",
  "project:transfer",
  "project:receive",
  "project:update",
  "project:delete",
  "project:updateMemberRole",
  "project:token:create",
  "project:token:update",
  "project:token:delete",
  "project:token:view",
  "defaultEnvironmentVariable:create",
  "defaultEnvironmentVariable:update",
  "defaultEnvironmentVariable:delete",
  "deployment:create",
  "deployment:delete",
  "deployment:update",
  "deployment:transfer",
  "deployment:receive",
  "deployment:token:create",
  "deployment:token:update",
  "deployment:token:delete",
  "deployment:token:view",
  "deployment:customDomain:create",
  "deployment:customDomain:delete",
  "deployment:backups:create",
  "deployment:backups:import",
  "deployment:backups:configurePeriodic",
  "deployment:backups:disablePeriodic",
  "deployment:backups:delete",
  "member:invite",
  "member:cancelInvitation",
  "member:remove",
  "member:updateRole",
  "member:token:createWithSsoAccess",
  "billing:paymentMethod:update",
  "billing:contact:update",
  "billing:address:update",
  "billing:subscription:create",
  "billing:subscription:resume",
  "billing:subscription:cancel",
  "billing:subscription:changePlan",
  "billing:spendingLimit:update",
  "oauthApplication:create",
  "oauthApplication:update",
  "oauthApplication:delete",
  "oauthApplication:verify",
  "oauthApplication:generateClientSecret",
  "integration:workos:team:create",
  "integration:workos:team:disconnect",
  "integration:workos:team:inviteMember",
  "integration:workos:environment:create",
  "integration:workos:environment:delete",
  "integration:workos:environment:retrieveCredentials",
  "integration:workos:projectEnvironment:create",
  "integration:workos:projectEnvironment:delete",
  "integration:workos:projectEnvironment:retrieveCredentials",
  "sso:enable",
  "sso:disable",
  "sso:update",
  "member:loginWithSso",
  "directorySync:enable",
  "directorySync:disable",
  "directorySync:updateGroupMapping",
  "directorySync:deleteGroupMapping",
  "customRole:create",
  "customRole:update",
  "customRole:delete",
] as const satisfies readonly AuditLogAction[];

// Assert that AUDIT_LOG_ACTIONS is exhaustive
type AssertSubset<Sub extends Super, Super> = Sub;
type _AllActionsListed = AssertSubset<
  AuditLogAction,
  (typeof AUDIT_LOG_ACTIONS)[number]
>;

const actionOptions: Option<AuditLogAction | "all_actions">[] = [
  { label: "All actions", value: "all_actions" },
  ...AUDIT_LOG_ACTIONS.map((action) => ({ label: action, value: action })),
];

export function AuditLogToolbar({
  selectedMember,
  setSelectedMember,
  selectedAction,
  setSelectedAction,
  selectedStartDay,
  selectedEndDay,
  members,
  setDate,
  auditLogRetentionDays,
}: {
  selectedMember: string;
  setSelectedMember: (member: string) => void;
  selectedAction: AuditLogAction | "all_actions";
  setSelectedAction: (action: AuditLogAction | "all_actions") => void;
  selectedStartDay: Date;
  selectedEndDay: Date;
  members: MemberResponse[];
  setDate: (date: DateRange) => void;
  auditLogRetentionDays: number;
}) {
  const minStartDate = startOfDay(
    auditLogRetentionDays === -1
      ? new Date(2024, 5, 5)
      : Date.now() - auditLogRetentionDays * 24 * 60 * 60 * 1000,
  );
  const beforeMinDateTooltip =
    auditLogRetentionDays === -1
      ? null
      : `Audit logs are preserved for ${auditLogRetentionDays} days.`;
  const maxEndDate = endOfToday();

  const startDate =
    selectedStartDay > minStartDate ? selectedStartDay : minStartDate;

  return (
    <div className="flex flex-wrap gap-2">
      <DateRangePicker
        minDate={minStartDate}
        maxDate={maxEndDate}
        date={{ from: startDate, to: selectedEndDay }}
        setDate={setDate}
        beforeMinDateTooltip={beforeMinDateTooltip}
      />
      <Combobox
        options={[
          { label: "All members", value: "all_members" },
          ...sortBy(
            members.map((m) => ({
              label: m.name ? `${m.name} (${m.email})` : m.email,
              value: m.id.toString(),
            })),
            [(option) => option.label.toLowerCase()],
          ),
        ]}
        optionsWidth="fit"
        allowCustomValue
        selectedOption={selectedMember}
        setSelectedOption={(o) =>
          setSelectedMember(o === null ? "all_members" : o)
        }
        label="Members"
      />
      <Combobox
        options={actionOptions}
        selectedOption={selectedAction}
        setSelectedOption={(o) =>
          setSelectedAction(o === null ? "all_actions" : o)
        }
        label="Actions"
      />
    </div>
  );
}
