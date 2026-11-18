import {
  CheckIcon,
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Checkbox } from "@ui/Checkbox";
import { cn } from "@ui/cn";
import { Link } from "@ui/Link";
import { Loading } from "@ui/Loading";
import { Popover } from "@ui/Popover";
import sortBy from "lodash/sortBy";
import { useState } from "react";
import type { CustomRoleResponse } from "@convex-dev/platform/managementApi";

export type InviteRole =
  | { role: "admin" | "developer" }
  | { role: "custom"; customRoleIds: number[] };

const BUILT_IN_ROLES = [
  { role: "admin", label: "Admin" },
  { role: "developer", label: "Developer" },
] as const;

/**
 * Picks Admin, Developer, or Custom, with custom roles in a second view.
 * Unchecking the last custom role falls back to Developer.
 */
export function InviteRolePicker({
  label,
  value,
  onChange,
  disabled = false,
  customRoles,
  customRolesLoading = false,
  customRolesDisabledReason,
  teamSlug,
}: {
  label: string;
  value: InviteRole;
  onChange: (value: InviteRole) => void;
  disabled?: boolean;
  customRoles: CustomRoleResponse[];
  customRolesLoading?: boolean;
  customRolesDisabledReason?: string;
  teamSlug: string;
}) {
  const sortedCustomRoles = sortBy(customRoles, (r) => r.name.toLowerCase());
  const selectedIds = value.role === "custom" ? value.customRoleIds : [];
  const selectedNames = sortedCustomRoles
    .filter((r) => selectedIds.includes(r.id))
    .map((r) => r.name);
  const summary =
    value.role !== "custom"
      ? BUILT_IN_ROLES.find((r) => r.role === value.role)!.label
      : selectedNames.length > 1
        ? `${selectedNames[0]} +${selectedNames.length - 1}`
        : (selectedNames[0] ?? "Custom");

  return (
    <Popover
      placement="bottom-end"
      offset={[0, 4]}
      padding={false}
      portal
      className="w-64 py-1"
      asChild
      button={({ open }) => (
        <Button
          variant="unstyled"
          aria-label={`${label}: ${summary}`}
          disabled={disabled}
          title={
            selectedNames.length > 1 ? selectedNames.join(", ") : undefined
          }
          className={cn(
            "flex h-[34px] w-32 shrink-0 cursor-pointer items-center gap-1 rounded-md border bg-background-secondary px-2 text-left text-sm hover:bg-background-tertiary sm:w-40",
            "focus-visible:border-border-selected focus-visible:outline-hidden",
            "disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-background-secondary",
            open && "border-border-selected",
          )}
        >
          <span className="grow truncate text-content-primary">{summary}</span>
          <ChevronDownIcon className="shrink-0 text-content-secondary" />
        </Button>
      )}
    >
      {({ close }) => (
        <RolePickerPanel
          value={value}
          onChange={onChange}
          close={close}
          customRoles={sortedCustomRoles}
          customRolesLoading={customRolesLoading}
          customRolesDisabledReason={customRolesDisabledReason}
          teamSlug={teamSlug}
        />
      )}
    </Popover>
  );
}

const ITEM_CLASSES =
  "flex w-full items-center gap-2 px-3 py-1.5 text-left text-content-primary hover:bg-background-tertiary focus-visible:bg-background-tertiary";

// Mounted on each open, so a custom value reopens on its custom roles unless
// they can't be picked.
function RolePickerPanel({
  value,
  onChange,
  close,
  customRoles,
  customRolesLoading,
  customRolesDisabledReason,
  teamSlug,
}: {
  value: InviteRole;
  onChange: (value: InviteRole) => void;
  close: () => void;
  customRoles: CustomRoleResponse[];
  customRolesLoading: boolean;
  customRolesDisabledReason?: string;
  teamSlug: string;
}) {
  const [view, setView] = useState<"roles" | "custom">(
    value.role === "custom" && customRolesDisabledReason === undefined
      ? "custom"
      : "roles",
  );
  const selectedIds = value.role === "custom" ? value.customRoleIds : [];

  const toggleCustomRole = (id: number) => {
    const customRoleIds = selectedIds.includes(id)
      ? selectedIds.filter((selected) => selected !== id)
      : [...selectedIds, id];
    onChange(
      customRoleIds.length === 0
        ? { role: "developer" }
        : { role: "custom", customRoleIds },
    );
  };

  if (view === "roles") {
    return (
      <div className="flex flex-col text-sm">
        {BUILT_IN_ROLES.map(({ role, label }) => (
          <Button
            key={role}
            variant="unstyled"
            className={ITEM_CLASSES}
            onClick={() => {
              onChange({ role });
              close();
            }}
          >
            <CheckIcon
              className={cn(
                "size-4 shrink-0",
                value.role !== role && "invisible",
              )}
            />
            {label}
          </Button>
        ))}
        <Button
          variant="unstyled"
          className={cn(
            ITEM_CLASSES,
            "disabled:cursor-not-allowed disabled:text-content-tertiary disabled:hover:bg-transparent",
          )}
          disabled={customRolesDisabledReason !== undefined}
          tip={customRolesDisabledReason}
          tipSide="left"
          onClick={() => setView("custom")}
        >
          <CheckIcon
            className={cn(
              "size-4 shrink-0",
              value.role !== "custom" && "invisible",
            )}
          />
          <span className="grow">Custom</span>
          <ChevronRightIcon className="shrink-0 text-content-secondary" />
        </Button>
        <div className="mt-1 border-t px-3 pt-2 pb-1 text-xs">
          <Link
            href="https://docs.convex.dev/dashboard/teams#roles-and-permissions"
            target="_blank"
          >
            Learn about team roles
          </Link>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col text-sm">
      <Button
        variant="unstyled"
        className={cn(ITEM_CLASSES, "text-xs text-content-secondary")}
        onClick={() => setView("roles")}
      >
        <ChevronLeftIcon className="size-4 shrink-0" />
        Custom roles
      </Button>
      <div className="border-t pt-1">
        {customRolesLoading ? (
          <div className="px-3 py-1.5">
            <Loading fullHeight={false} className="h-6 w-full" />
          </div>
        ) : customRoles.length === 0 ? (
          <p className="px-3 py-1.5 text-xs text-content-secondary">
            This team has no custom roles yet.{" "}
            <Link href={`/t/${teamSlug}/settings/custom-roles`}>
              Create one
            </Link>
          </p>
        ) : (
          <div className="max-h-48 overflow-y-auto">
            {customRoles.map((customRole) => (
              <label
                key={customRole.id}
                className={cn(ITEM_CLASSES, "cursor-pointer")}
              >
                <Checkbox
                  checked={selectedIds.includes(customRole.id)}
                  onChange={() => toggleCustomRole(customRole.id)}
                  aria-label={customRole.name}
                />
                <span className="truncate">{customRole.name}</span>
              </label>
            ))}
          </div>
        )}
      </div>
      <div className="mt-1 flex justify-end border-t px-3 pt-2 pb-1">
        <Button size="xs" onClick={close}>
          Done
        </Button>
      </div>
    </div>
  );
}
