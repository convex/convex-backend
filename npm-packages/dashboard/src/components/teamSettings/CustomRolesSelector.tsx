import { ChipInput } from "@ui/ChipInput";
import { useMemo, useState } from "react";
import sortBy from "lodash/sortBy";
import type { CustomRoleResponse } from "@convex-dev/platform/managementApi";

type CustomRolesSelectorProps = {
  /** All custom roles defined for the team. */
  availableRoles: CustomRoleResponse[];
  /** IDs of custom roles currently selected. */
  selectedIds: number[];
  /** Called with the new selection when the user adds or removes a role. */
  onChange: (ids: number[]) => void;
};

/**
 * Multi-select for custom team roles: a `ChipInput` whose list offers the
 * team's unselected roles that match the typed search. Used when inviting a
 * member with `role=custom`, when changing an existing member's role to
 * `custom`, and when mapping a directory group to custom roles.
 */
export function CustomRolesSelector({
  availableRoles,
  selectedIds,
  onChange,
}: CustomRolesSelectorProps) {
  const [query, setQuery] = useState("");

  const sortedRoles = useMemo(
    () => sortBy(availableRoles, (r) => r.name.toLowerCase()),
    [availableRoles],
  );
  const nameById = useMemo(
    () => new Map(sortedRoles.map((r) => [r.id, r.name] as const)),
    [sortedRoles],
  );
  const selectedSorted = useMemo(
    () => sortBy(selectedIds, (id) => (nameById.get(id) ?? "").toLowerCase()),
    [selectedIds, nameById],
  );
  const unselectedRoles = useMemo(() => {
    const selectedSet = new Set(selectedIds);
    return sortedRoles.filter((r) => !selectedSet.has(r.id));
  }, [sortedRoles, selectedIds]);

  const normalizedQuery = query.trim().toLowerCase();
  const matchingRoles =
    normalizedQuery === ""
      ? unselectedRoles
      : unselectedRoles.filter((r) =>
          r.name.toLowerCase().includes(normalizedQuery),
        );
  const noRolesExist = sortedRoles.length === 0;
  const nameOf = (id: number) => nameById.get(id) ?? `Role #${id}`;

  const note = noRolesExist
    ? "No custom roles exist for this team yet."
    : unselectedRoles.length === 0
      ? "All custom roles are added."
      : matchingRoles.length === 0
        ? `No custom roles match "${query.trim()}".`
        : undefined;

  return (
    <ChipInput
      label="Custom roles"
      values={selectedSorted}
      // The query stays after a pick, so several roles matching one search
      // can be added in a row.
      onChange={onChange}
      chipLabel={nameOf}
      removeLabel={(id) => `Remove custom role ${nameOf(id)}`}
      removeTip="Remove custom role"
      chipClassName="max-w-56"
      query={query}
      onQueryChange={setQuery}
      options={matchingRoles.map((r) => ({ value: r.id, label: r.name }))}
      onBlur={() => setQuery("")}
      placeholder="Search custom roles"
      disabled={noRolesExist}
      description={note}
    />
  );
}
