import { Button } from "@ui/Button";
import { Chip } from "@ui/Chip";
import { Tooltip } from "@ui/Tooltip";

// Shared by the two project admin editors: per project (members as rows) and
// per member (projects as rows).

/**
 * Minimum height for a bordered list with a header and `rowCount` two-line
 * rows, capped at the list's 50vh max height. Reserving the unfiltered list's
 * height keeps the list from changing size as search narrows the results.
 * Rows are 3.8125rem (py-3 around a text-sm and a text-xs line, plus a
 * divider). The 2.375rem base is the header (py-2 around a text-sm line, plus
 * its border) and the list's top and bottom borders, less the divider the
 * first row doesn't have.
 * `footerRem` reserves room for a footer inside the border, like pagination.
 */
export function listMinHeight(rowCount: number, footerRem = 0) {
  return `min(50vh, ${2.375 + rowCount * 3.8125 + footerRem}rem)`;
}

export function PendingRoleChangeBadge({
  isOriginalAdmin,
  isNewAdmin,
}: {
  isOriginalAdmin: boolean;
  isNewAdmin: boolean;
}) {
  if (isOriginalAdmin && !isNewAdmin) {
    return (
      <span className="shrink-0 rounded-sm bg-background-error px-2 py-0.5 text-xs font-medium text-content-error">
        Role will be removed
      </span>
    );
  }
  if (!isOriginalAdmin && isNewAdmin) {
    return (
      <span className="shrink-0 rounded-sm bg-background-success px-2 py-0.5 text-xs font-medium text-content-success">
        Role will be added
      </span>
    );
  }
  return null;
}

export function RoleChangesFooter({
  added,
  removed,
  noun,
  isSaving,
  onCancel,
}: {
  /** Display names of the rows gaining the admin role. */
  added: string[];
  /** Display names of the rows losing the admin role. */
  removed: string[];
  /** Singular noun for a row, e.g. "admin" or "project". */
  noun: string;
  isSaving: boolean;
  onCancel: () => void;
}) {
  const hasChanges = added.length > 0 || removed.length > 0;
  const pluralize = (count: number) =>
    `${count} ${noun}${count === 1 ? "" : "s"}`;
  return (
    <div className="flex flex-wrap items-center justify-between gap-4">
      {hasChanges && (
        <div
          role="status"
          className="flex flex-wrap items-center gap-2 text-sm font-medium text-content-primary"
        >
          Unsaved changes
          {added.length > 0 && (
            <ChangeChip names={added}>Add {pluralize(added.length)}</ChangeChip>
          )}
          {removed.length > 0 && (
            <ChangeChip names={removed}>
              Remove {pluralize(removed.length)}
            </ChangeChip>
          )}
        </div>
      )}
      <div className="ml-auto flex gap-2">
        <Button variant="neutral" disabled={isSaving} onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit" disabled={!hasChanges} loading={isSaving}>
          Save
        </Button>
      </div>
    </div>
  );
}

// Keeps the tooltip short enough to fit on screen when many rows change.
const MAX_TOOLTIP_NAMES = 10;

function ChangeChip({
  names,
  children,
}: {
  names: string[];
  children: React.ReactNode;
}) {
  const sortedNames = [...names].sort((a, b) =>
    a.localeCompare(b, undefined, { sensitivity: "base" }),
  );
  const shown = sortedNames.slice(0, MAX_TOOLTIP_NAMES);
  const hiddenCount = sortedNames.length - shown.length;
  return (
    <Tooltip
      side="top"
      contentClassName="text-left"
      tip={
        <ul className="list-disc py-0.5 pr-1 pl-4">
          {shown.map((name, i) => (
            // Names can repeat (e.g. two members with the same name).
            <li key={`${i}-${name}`}>{name}</li>
          ))}
          {hiddenCount > 0 && (
            <li className="text-content-secondary">and {hiddenCount} more</li>
          )}
        </ul>
      }
    >
      <Chip>{children}</Chip>
    </Tooltip>
  );
}
