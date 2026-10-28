import {
  ExclamationTriangleIcon,
  InfoCircledIcon,
  MagnifyingGlassIcon,
} from "@radix-ui/react-icons";
import fuzzy from "fuzzy";
import { useEffect, useRef, useState } from "react";
import { Button } from "@ui/Button";
import { cn } from "@ui/cn";
import { ContextMenu } from "@common/features/data/components/ContextMenu";
import { FunnelIcon, IndexIcon } from "@common/elements/icons";
import {
  DatabaseIndexDef,
  FieldOption,
  SearchIndexDef,
  bestFieldOption,
} from "./filterModel";

// Adds a filter on any field. Fields an index could serve offer a choice
// between the indexes that can serve them and scanning on top of the current
// results; fields without one are added as a scan directly.
export function AddFilterMenu({
  fields,
  optionsFor,
  onAddScan,
  onAddIndexed,
  onAddWithIndex,
  onStartSearch,
}: {
  fields: string[];
  optionsFor(field: string): FieldOption[];
  onAddScan(field: string): void;
  onAddIndexed(field: string): void;
  onAddWithIndex(index: DatabaseIndexDef, field: string): void;
  onStartSearch(index: SearchIndexDef): void;
}) {
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [target, setTarget] = useState<{ x: number; y: number } | null>(null);
  const close = () => setTarget(null);
  const [query, setQuery] = useState("");
  useEffect(() => {
    if (target === null) setQuery("");
  }, [target]);
  const shownFields = fields.filter(
    (field) => query.trim() === "" || fuzzy.test(query.trim(), field),
  );

  const apply = (field: string, option: FieldOption) => {
    switch (option.kind) {
      case "searchFilter":
        onAddIndexed(field);
        break;
      case "index":
        onAddWithIndex(option.index, field);
        break;
      case "search":
        onStartSearch(option.index);
        break;
      default:
        onAddScan(field);
    }
  };

  const addBest = (field: string) =>
    apply(field, bestFieldOption(optionsFor(field)));

  return (
    <>
      <Button
        ref={buttonRef}
        variant="unstyled"
        aria-label="Add a filter"
        aria-haspopup="menu"
        aria-expanded={target !== null}
        onClick={() => {
          const rect = buttonRef.current?.getBoundingClientRect();
          setTarget(target || !rect ? null : { x: rect.left, y: rect.bottom });
        }}
        className={cn(
          "flex h-6 items-center gap-1 rounded-md border border-dashed px-1.5 text-xs",
          target !== null
            ? "border-border-selected text-content-primary"
            : "text-content-secondary hover:border-border-selected hover:text-content-primary",
        )}
        icon={<FunnelIcon />}
        data-testid="add-filter"
      >
        Add filter
      </Button>
      <ContextMenu target={target} onClose={close} placement="bottom-start">
        <div className="mb-1 flex items-center gap-2 border-b px-3 pb-1.5">
          <MagnifyingGlassIcon className="size-3.5 shrink-0 text-content-secondary" />
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              // Enter takes the top match's best option. Arrow keys and
              // Escape fall through to the menu; everything else is typing
              // and must not trigger the menu's type-ahead.
              if (e.key === "Enter") {
                e.preventDefault();
                if (shownFields[0]) {
                  close();
                  addBest(shownFields[0]);
                }
                return;
              }
              if (!["ArrowDown", "ArrowUp", "Escape", "Tab"].includes(e.key)) {
                e.stopPropagation();
              }
            }}
            placeholder="Filter by..."
            aria-label="Filter by field"
            className="w-full bg-transparent py-0.5 text-xs text-content-primary placeholder:text-content-tertiary focus:outline-none"
          />
        </div>
        {shownFields.length === 0 && (
          <div className="px-3 py-1.5 text-xs text-content-tertiary">
            No matching fields.
          </div>
        )}
        {shownFields.map((field) => {
          const options = optionsFor(field);
          // A scan is the only option every field has; on its own it needs no
          // submenu to choose from.
          if (options.length === 1) {
            return (
              <ContextMenu.Item
                key={field}
                label={<code>{field}</code>}
                action={() => onAddScan(field)}
              />
            );
          }
          // Options that drop existing filters are shown after table scan so
          // users see the safe choices first.
          const safeOptions = options.filter(
            (o) =>
              o.kind === "searchFilter" ||
              (o.kind === "index" && o.dropped === 0) ||
              (o.kind === "search" && o.dropped === 0) ||
              o.kind === "scan",
          );
          const destructiveOptions = options.filter(
            (o) =>
              (o.kind === "index" && o.dropped > 0) ||
              (o.kind === "search" && o.dropped > 0),
          );
          return (
            <ContextMenu.Submenu
              key={field}
              label={<code>{field}</code>}
              action={() => addBest(field)}
            >
              {safeOptions.map((option) => (
                <ContextMenu.Item
                  key={optionKey(option)}
                  icon={optionIcon(option)}
                  label={optionLabel(option)}
                  tip={optionTip(option)}
                  tipSide="right"
                  action={() => apply(field, option)}
                />
              ))}
              {destructiveOptions.map((option) => (
                <ContextMenu.Item
                  key={optionKey(option)}
                  icon={optionIcon(option)}
                  label={optionLabel(option, true)}
                  tip={optionTip(option)}
                  tipSide="right"
                  action={() => apply(field, option)}
                />
              ))}
            </ContextMenu.Submenu>
          );
        })}
      </ContextMenu>
    </>
  );
}

function optionKey(option: FieldOption) {
  return option.kind === "scan"
    ? "scan"
    : `${option.kind}/${option.index.name}`;
}

function optionIcon(option: FieldOption) {
  switch (option.kind) {
    case "index":
      return <IndexIcon kind="database" />;
    case "search":
    case "searchFilter":
      return <IndexIcon kind="search" />;
    default:
      return <FunnelIcon />;
  }
}

function optionLabel(option: FieldOption, destructive = false) {
  if (option.kind === "scan") {
    return (
      <span className="flex items-center gap-1">
        Table scan
        <InfoCircledIcon className="ml-0.5 size-3 shrink-0 translate-y-px text-content-secondary" />
      </span>
    );
  }
  const warning = destructive ? (
    <ExclamationTriangleIcon className="ml-0.5 size-3 shrink-0 translate-y-px text-content-secondary" />
  ) : null;
  if (option.kind === "search") {
    return (
      <span className="flex items-center gap-1">
        Search with index <code>{option.index.name}</code>
        {warning}
      </span>
    );
  }
  return (
    <span className="flex items-center gap-1">
      <code>{option.index.name}</code>
      {warning}
    </span>
  );
}

function optionTip(option: FieldOption) {
  switch (option.kind) {
    case "index":
      return option.dropped > 0
        ? `Selecting this will clear ${option.dropped} existing indexed filter${option.dropped === 1 ? "" : "s"}.`
        : undefined;
    case "search":
      return option.dropped > 0
        ? `Selecting this will clear ${option.dropped} existing filter${option.dropped === 1 ? "" : "s"}.`
        : undefined;
    case "scan":
      return "Table scans may be slow on large tables. Prefer an indexed filter.";
    default:
      return undefined;
  }
}
