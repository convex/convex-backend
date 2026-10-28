import { type NodeProps, type NodeTypes } from "@xyflow/react";
import { TableIcon } from "@radix-ui/react-icons";
import { cn } from "@ui/cn";
import { SchemaNode } from "@common/features/schema/lib/buildSchemaGraph";
import {
  NODE_WIDTH,
  NODE_HEADER_HEIGHT,
  NODE_ROW_HEIGHT,
  MAX_VISIBLE_ROWS,
  MAX_VISIBLE_INDEXES,
} from "@common/features/schema/lib/layout";
import { IndexIcon, INDEX_KIND_LABEL } from "@common/elements/icons";
import { TableFlowNode } from "@common/features/schema/components/schemaFlowTypes";

function pluralize(count: number, singular: string, plural: string): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

export function nodeAriaLabel(node: SchemaNode, references: string[]): string {
  const parts = [pluralize(node.fields.length, "field", "fields")];
  if (node.indexes.length > 0) {
    parts.push(pluralize(node.indexes.length, "index", "indexes"));
  }
  if (references.length > 0) {
    parts.push(`references ${pluralize(references.length, "table", "tables")}`);
  }
  return `Table ${node.table}. ${parts.join(", ")}.`;
}

function overflow<T>(
  items: T[],
  max: number,
): { visible: T[]; hidden: number } {
  if (items.length <= max + 1) {
    return { visible: items, hidden: 0 };
  }
  return { visible: items.slice(0, max), hidden: items.length - max };
}

export function FlowTableNode({ data }: NodeProps<TableFlowNode>) {
  const { node, isSelected, highlightedFields, highlightedIndexes, onHover } =
    data;
  const fields = overflow(node.fields, MAX_VISIBLE_ROWS);
  const indexes = overflow(node.indexes, MAX_VISIBLE_INDEXES);

  return (
    <div
      className={cn(
        "flex flex-col overflow-hidden rounded-lg border bg-background-secondary",
        isSelected && "border-border-selected outline-2 outline-util-accent",
      )}
      style={{ width: NODE_WIDTH }}
      onMouseLeave={() => onHover(null)}
    >
      <div
        className="flex items-center gap-1.5 border-b bg-[rgb(226,224,221)] px-2.5 font-mono text-sm font-medium text-content-primary dark:bg-[rgb(74,72,69)]"
        style={{ height: NODE_HEADER_HEIGHT }}
        title={node.table}
        onMouseEnter={() => onHover({ kind: "header", table: node.table })}
      >
        <TableIcon className="size-3.5 shrink-0 text-content-secondary" />
        <div className="flex min-w-0 items-center gap-0.5">
          <span className="truncate">{node.table}</span>
          {node.notInSchema && (
            <span
              title="This table is not defined in your schema."
              className="shrink-0 font-sans text-content-tertiary"
            >
              *
            </span>
          )}
        </div>
        {node.union && (
          <span
            title={
              node.union.discriminator
                ? `Union of ${node.union.variants.length} members, discriminated by "${node.union.discriminator}". Open this table to switch between members.`
                : `Union of ${node.union.variants.length} members. Open this table to switch between members.`
            }
            className="ml-auto shrink-0 rounded-sm border px-1 font-sans text-[10px] font-medium text-content-tertiary"
          >
            union
          </span>
        )}
      </div>

      <div className="flex flex-col">
        <div className="flex flex-col py-1">
          {node.fields.length === 0 && (
            <div
              className="flex items-center px-2.5 text-xs text-content-tertiary italic"
              style={{ height: NODE_ROW_HEIGHT }}
            >
              no fields
            </div>
          )}
          {fields.visible.map((field) => {
            const highlighted = highlightedFields.has(field.name);
            return (
              <div
                key={field.name}
                className={cn(
                  "flex items-center gap-1.5 px-2.5 text-xs",
                  highlighted && "bg-util-accent/15",
                )}
                style={{ height: NODE_ROW_HEIGHT }}
                onMouseEnter={() =>
                  onHover({
                    kind: "field",
                    table: node.table,
                    field: field.name,
                  })
                }
              >
                <span
                  className={cn(
                    "min-w-0 shrink truncate font-mono text-content-primary",
                    highlighted && "font-semibold",
                  )}
                >
                  {field.name}
                  {field.optional && (
                    <span className="text-content-tertiary">?</span>
                  )}
                </span>
                {/* Yields space first so a long type never truncates the name. */}
                <span className="ml-auto min-w-0 shrink-10000 truncate pl-2 font-mono text-[10px] text-content-tertiary">
                  {field.type}
                </span>
              </div>
            );
          })}
          {fields.hidden > 0 && (
            <div
              className="flex items-center px-2.5 text-xs text-content-tertiary"
              style={{ height: NODE_ROW_HEIGHT }}
            >
              +{fields.hidden} more field{fields.hidden === 1 ? "" : "s"}
            </div>
          )}
        </div>

        {node.indexes.length > 0 && (
          <div className="flex flex-col border-t py-1">
            <div
              className="flex items-center px-2.5 text-[10px] font-medium tracking-wide text-content-tertiary uppercase"
              style={{ height: NODE_ROW_HEIGHT }}
            >
              Indexes
            </div>
            {indexes.visible.map((index) => {
              const highlighted = highlightedIndexes.has(index.name);
              return (
                <div
                  key={`${index.kind}:${index.name}`}
                  className={cn(
                    "flex items-center gap-1.5 px-2.5 text-xs",
                    highlighted && "bg-util-accent/15",
                  )}
                  style={{ height: NODE_ROW_HEIGHT }}
                  onMouseEnter={() =>
                    onHover({
                      kind: "index",
                      table: node.table,
                      index: index.name,
                    })
                  }
                >
                  <span title={INDEX_KIND_LABEL[index.kind]} className="flex">
                    <IndexIcon
                      kind={index.kind}
                      className="text-content-secondary"
                    />
                  </span>
                  <span
                    className={cn(
                      "min-w-0 shrink truncate font-mono text-content-primary",
                      highlighted && "font-semibold",
                    )}
                  >
                    {index.name}
                  </span>
                  {/* Truncates last, like the field rows above. */}
                  <span className="ml-auto min-w-0 shrink-10000 truncate pl-2 font-mono text-[10px] text-content-tertiary">
                    {index.fields.join(", ")}
                  </span>
                </div>
              );
            })}
            {indexes.hidden > 0 && (
              <div
                className="flex items-center px-2.5 text-xs text-content-tertiary"
                style={{ height: NODE_ROW_HEIGHT }}
              >
                +{indexes.hidden} more index
                {indexes.hidden === 1 ? "" : "es"}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

export const nodeTypes: NodeTypes = { table: FlowTableNode };
