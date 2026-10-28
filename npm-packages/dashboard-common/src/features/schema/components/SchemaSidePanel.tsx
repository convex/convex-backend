import { forwardRef, useEffect, useMemo, useState } from "react";
import {
  Cross2Icon,
  ChevronDownIcon,
  ChevronUpIcon,
  TableIcon,
  TargetIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { cn } from "@ui/cn";
import {
  SchemaNode,
  SchemaUnion,
} from "@common/features/schema/lib/buildSchemaGraph";
import { FieldIcon } from "@common/elements/icons";
import { IndexList } from "@common/features/data/components/IndexList";

const ID_REFERENCE = /Id<"([A-Za-z0-9_]+)">/g;

// Split a type string into text and `Id<"table">` segments, rendering the table
// name as a link that focuses the referenced table.
function renderTypeTokens(
  type: string,
  onFocusTable: (table: string) => void,
): React.ReactNode[] {
  const parts: React.ReactNode[] = [];
  let cursor = 0;
  let key = 0;
  // `matchAll` (not `exec` on the shared `/g` regex) so a stray `lastIndex`
  // can't leak between renders and drop `Id<"…">` links.
  for (const match of type.matchAll(ID_REFERENCE)) {
    const [full, table] = match;
    const { index } = match;
    if (index > cursor) {
      parts.push(type.slice(cursor, index));
    }
    parts.push('Id<"');
    key += 1;
    // System tables (names starting with `_`) aren't part of the schema graph,
    // so there's nothing to focus — render them as plain text.
    if (table.startsWith("_")) {
      parts.push(table);
    } else {
      parts.push(
        <Button
          key={key}
          variant="unstyled"
          // eslint-disable-next-line no-restricted-syntax -- link-styled in-app action, not a navigation Link
          className="inline text-content-link underline decoration-content-link/40 hover:decoration-content-link"
          onClick={() => onFocusTable(table)}
        >
          {table}
        </Button>,
      );
    }
    parts.push('">');
    cursor = index + full.length;
  }
  if (cursor < type.length) {
    parts.push(type.slice(cursor));
  }
  return parts;
}

// A type label with `Id<table>` references linked. Long types are truncated to
// a single line with a chevron to expand them to the full, wrapped type.
function TypeLabel({
  type,
  fullType,
  expanded,
  onToggle,
  onFocusTable,
}: {
  type: string;
  // The full type source, shown when expanded if the compact label hid detail.
  fullType?: string;
  expanded: boolean;
  onToggle: () => void;
  onFocusTable: (table: string) => void;
}) {
  // Expand to reveal the full type when the compact label collapsed something
  // (e.g. an object shown as `{ … }`), or when it might overflow the column.
  const canExpand = fullType !== undefined || type.length > 24;
  const display = expanded && fullType !== undefined ? fullType : type;
  return (
    <div className="flex max-w-[60%] min-w-0 items-start gap-1">
      <span
        title={fullType ?? type}
        className={cn(
          "min-w-0 font-mono text-content-tertiary",
          expanded
            ? "scrollbar block max-h-48 overflow-y-auto wrap-break-word whitespace-pre-wrap"
            : "truncate text-right",
        )}
      >
        {renderTypeTokens(display, onFocusTable)}
      </span>
      {canExpand ? (
        <Button
          variant="unstyled"
          inline
          className="mt-0.5 shrink-0 text-content-tertiary hover:text-content-primary"
          onClick={onToggle}
          aria-label={expanded ? "Collapse type" : "Expand type"}
        >
          {expanded ? (
            <ChevronUpIcon className="size-4" />
          ) : (
            <ChevronDownIcon className="size-4" />
          )}
        </Button>
      ) : (
        // Reserve the chevron's width so types without an expander still line up
        // with those that have one.
        <span aria-hidden className="mt-0.5 size-4 shrink-0" />
      )}
    </div>
  );
}

function UnionVariantSelector({
  union,
  selected,
  onSelect,
}: {
  union: SchemaUnion;
  selected: number;
  onSelect: (variant: number) => void;
}) {
  return (
    <div className="flex flex-wrap gap-1">
      {union.variants.map((variant, i) => (
        <Button
          key={i}
          size="xs"
          variant="neutral"
          focused={selected === i}
          onClick={() => onSelect(i)}
          className="font-mono"
        >
          {variant.label}
        </Button>
      ))}
    </div>
  );
}

export const SchemaSidePanel = forwardRef<
  HTMLDivElement,
  {
    node: SchemaNode;
    onClose: () => void;
    onOpenData: (table: string) => void;
    // Focus another table (e.g. when clicking a reference in a type).
    onFocusTable: (table: string) => void;
  }
>(({ node, onClose, onOpenData, onFocusTable }, ref) => {
  const [expandedTypes, setExpandedTypes] = useState<Set<string>>(new Set());
  const toggleType = (name: string) =>
    setExpandedTypes((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });

  const { union } = node;
  const [selectedVariant, setSelectedVariant] = useState(0);
  useEffect(() => {
    setSelectedVariant(0);
    setExpandedTypes(new Set());
  }, [node.table]);
  const variantIndex =
    union && selectedVariant < union.variants.length ? selectedVariant : 0;
  const fields = useMemo(
    () => (union ? (union.variants[variantIndex]?.fields ?? []) : node.fields),
    [union, variantIndex, node.fields],
  );

  return (
    <div
      ref={ref}
      // Focusable so a keyboard selection can land here; the label names the
      // region for screen readers when it does.
      tabIndex={-1}
      role="region"
      aria-label={`${node.table} table details`}
      className="flex size-full flex-col bg-background-secondary outline-none"
    >
      {/* Header - table name. */}
      <div className="flex items-center gap-2 border-b px-3 py-2.5">
        <div className="flex min-w-0 flex-col">
          <span className="truncate font-mono text-sm font-semibold text-content-primary">
            {node.table}
          </span>
          {node.notInSchema && (
            <span className="text-xs text-content-tertiary">
              Not defined in your schema
            </span>
          )}
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-1">
          <Button
            variant="neutral"
            size="xs"
            icon={<TableIcon />}
            onClick={() => onOpenData(node.table)}
          >
            View data
          </Button>
          <Button
            variant="neutral"
            size="xs"
            inline
            icon={<Cross2Icon />}
            onClick={onClose}
            aria-label="Close schema panel"
          />
        </div>
      </div>

      {/* gap-10 matches the spacing IndexList uses between its own sections. */}
      <div className="scrollbar flex min-h-0 flex-1 flex-col gap-10 overflow-y-auto p-3 text-xs">
        <div className="flex flex-col gap-3">
          {/* Match the IndexList section headers (icon + text-base h5). */}
          <header className="flex items-center gap-1.5 text-content-primary">
            <FieldIcon className="text-[1.25rem] text-content-secondary" />
            <h5 className="text-base font-medium">Fields</h5>
          </header>
          {union && (
            <UnionVariantSelector
              union={union}
              selected={variantIndex}
              onSelect={setSelectedVariant}
            />
          )}
          <div className="rounded-md border">
            {fields.length === 0 ? (
              <div className="px-2 py-1.5 text-content-tertiary italic">
                This table has no fields.
              </div>
            ) : (
              fields.map((field) => (
                <div
                  key={field.name}
                  className="flex items-start gap-1.5 border-b px-2 py-1.5 last:border-b-0"
                >
                  <span className="flex min-w-0 flex-1 items-center gap-1.5">
                    <span className="min-w-0 truncate font-mono text-content-primary">
                      {field.name}
                      {field.optional && (
                        <span className="text-content-tertiary">?</span>
                      )}
                    </span>
                    {union?.discriminator === field.name && (
                      <Tooltip
                        tip="This field's value is what selects this member of the union."
                        aria-label="Discriminator field"
                        className="flex shrink-0"
                      >
                        <TargetIcon className="size-3.5 text-content-tertiary" />
                      </Tooltip>
                    )}
                  </span>
                  <TypeLabel
                    type={field.type}
                    fullType={field.fullType}
                    expanded={expandedTypes.has(field.name)}
                    onToggle={() => toggleType(field.name)}
                    onFocusTable={onFocusTable}
                  />
                </div>
              ))
            )}
          </div>
        </div>

        {/* Reuse the data page's index view so indexes render identically
            (backfill state, staged badges, search/vector details) and stay in
            sync with the deployment's actual indexes. */}
        <IndexList tableName={node.table} />
      </div>
    </div>
  );
});

SchemaSidePanel.displayName = "SchemaSidePanel";
