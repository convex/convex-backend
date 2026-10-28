import startCase from "lodash/startCase";
import Link from "next/link";
import { cn } from "@ui/cn";
import type { TeamMemberCustomRole } from "@convex-dev/platform/managementApi";

type RoleDisplayProps = {
  role: string;
  customRoles?: TeamMemberCustomRole[] | null;
  teamSlug: string;
  /** Right aligned where the role is read against the one beside it. */
  align?: "left" | "right";
};

/**
 * Renders a team member's role: built-in roles (Admin/Developer) as plain
 * text and `custom` as a row of tag chips with each attached role's name.
 * Custom-role chips link to the role's view-definition page. Used by both
 * the team-members list and the pending-invites list so they stay visually
 * consistent.
 */
export function RoleDisplay({
  role,
  customRoles,
  teamSlug,
  align = "left",
}: RoleDisplayProps) {
  const alignRight = align === "right";
  if (role !== "custom") {
    return (
      <div
        className={cn(
          "text-sm text-content-primary",
          alignRight && "text-right",
        )}
      >
        {startCase(role)}
      </div>
    );
  }
  const list = customRoles ?? [];
  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-1",
        alignRight && "justify-end",
      )}
    >
      {list.length === 0 ? (
        <div className="text-sm text-content-primary">Custom</div>
      ) : (
        list.map(({ id, name }) => (
          <Link
            key={id}
            href={{
              pathname: "/t/[team]/settings/custom-roles",
              query: { team: teamSlug, view: id.toString() },
            }}
            className="rounded-sm border px-1.5 py-1 text-xs text-content-primary hover:bg-background-tertiary"
          >
            {name}
          </Link>
        ))
      )}
    </div>
  );
}
