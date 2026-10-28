import {
  Cross2Icon,
  ExclamationTriangleIcon,
  MagnifyingGlassIcon,
} from "@radix-ui/react-icons";
import { GenericDocument } from "convex/server";
import { ValidatorJSON } from "convex/values";
import { Button } from "@ui/Button";
import { cn } from "@ui/cn";
import { FilterItem, FilterItemUpdate } from "./useFilterActions";
import { FloatingPanel } from "./FloatingPanel";
import {
  IndexedClauseEditor,
  ScanClauseEditor,
  SearchFilterClauseEditor,
  SearchTextEditor,
} from "./ClauseEditor";
import {
  describeIndexedClause,
  describeScanClause,
  formatFilterValue,
} from "./filterModel";

export function filterItemLabel(filterItem: FilterItem): string {
  switch (filterItem.kind) {
    case "indexed":
      return describeIndexedClause(filterItem.field, filterItem.clause);
    case "scan":
      return describeScanClause(filterItem.clause);
    case "search":
      return `${filterItem.field} ~ ${filterItem.search === "" ? "…" : JSON.stringify(filterItem.search)}`;
    case "searchFilter":
      return `${filterItem.clause.field} = ${formatFilterValue(filterItem.clause.field, filterItem.clause.value)}`;
    default:
      return "";
  }
}

export function FilterChip({
  filterItem,
  open,
  error,
  defaultDocument,
  getValidator,
  shouldSurfaceValidatorErrors,
  onOpenChange,
  onUpdate,
  canRemove = true,
  onRemove,
  onDone,
  onError,
}: {
  filterItem: FilterItem;
  open: boolean;
  error?: string;
  defaultDocument: GenericDocument;
  getValidator(field?: string): ValidatorJSON | undefined;
  shouldSurfaceValidatorErrors?: boolean;
  onOpenChange(open: boolean): void;
  onUpdate(update: FilterItemUpdate): void;
  // Indexed filters must be removed from the end, so the remove button is
  // disabled while later filters still depend on this one.
  canRemove?: boolean;
  onRemove(): void;
  onDone(): void;
  // `shown` is false for an error about a value the user hasn't edited yet: it
  // still blocks the filter from being applied, but nothing marks the chip.
  onError(errors: string[], shown: boolean): void;
}) {
  const label = filterItemLabel(filterItem);
  const icon =
    filterItem.kind === "search" ? (
      <MagnifyingGlassIcon className="size-3 shrink-0" />
    ) : null;

  const editor = (() => {
    switch (filterItem.kind) {
      case "indexed":
        return (
          <IndexedClauseEditor
            field={filterItem.field}
            clause={filterItem.clause}
            isLast={filterItem.isLast}
            onChange={(clause) => onUpdate({ kind: "indexed", clause })}
            onError={onError}
            onApply={onDone}
            validator={getValidator(filterItem.field)}
            shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
            path={filterItem.key}
          />
        );
      case "scan":
        return (
          <ScanClauseEditor
            clause={filterItem.clause}
            defaultDocument={defaultDocument}
            onChange={(clause) => onUpdate({ kind: "scan", clause })}
            onError={onError}
            onApply={onDone}
            validator={getValidator(filterItem.clause.field)}
            shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
            path={filterItem.key}
          />
        );
      case "search":
        return (
          <SearchTextEditor
            field={filterItem.field}
            value={filterItem.search}
            onChange={(search) => onUpdate({ kind: "search", search })}
            onApply={onDone}
          />
        );
      case "searchFilter":
        return (
          <SearchFilterClauseEditor
            field={filterItem.clause.field}
            value={filterItem.clause.value}
            onChange={(value) =>
              onUpdate({ kind: "searchFilter", value: value as any })
            }
            onError={onError}
            onApply={onDone}
            validator={getValidator(filterItem.clause.field)}
            shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
            path={filterItem.key}
          />
        );
      default:
        return null;
    }
  })();

  return (
    <FloatingPanel
      open={open}
      onOpenChange={onOpenChange}
      label={`Edit filter ${label}`}
      className="w-88 max-w-[calc(100vw-2rem)]"
      button={
        <div
          className={cn(
            "group flex h-6 max-w-full min-w-0 items-stretch overflow-hidden rounded-md border text-xs",
            "bg-background-secondary",
            error && "border-content-error bg-background-error",
            open && "border-border-selected",
          )}
          data-testid={`filter-chip-${filterItem.key}`}
        >
          <Button
            variant="unstyled"
            onClick={() => onOpenChange(!open)}
            className="flex min-w-0 items-center gap-1.5 pr-1.5 pl-2 hover:bg-background-tertiary"
            aria-label={`Edit filter: ${label}`}
            aria-expanded={open}
          >
            {icon}
            <span className="truncate font-mono">{label}</span>
            {error && (
              <ExclamationTriangleIcon className="size-3 shrink-0 text-content-errorSecondary" />
            )}
          </Button>
          <Button
            variant="unstyled"
            className={cn(
              "mx-0.5 flex items-center self-center rounded-full p-0.5 text-content-secondary",
              canRemove
                ? "hover:bg-background-tertiary hover:text-content-primary"
                : "cursor-not-allowed opacity-40",
            )}
            onClick={onRemove}
            disabled={!canRemove}
            tip={
              canRemove
                ? undefined
                : "An index is filtered in order, so remove the filters after this one first."
            }
            tipSide="bottom"
            aria-label={
              canRemove
                ? `Remove filter: ${label}`
                : `Remove filter: ${label} (remove the filters after it first)`
            }
            icon={<Cross2Icon className="size-3" />}
          />
        </div>
      }
    >
      {editor}
      {error && (
        <p
          className="text-xs wrap-break-word text-content-errorSecondary"
          role="alert"
        >
          {error}
        </p>
      )}
    </FloatingPanel>
  );
}
