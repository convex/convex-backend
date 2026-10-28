import { JSONValue, ValidatorJSON, Value, convexToJson } from "convex/values";
import { GenericDocument } from "convex/server";
import { useCallback, useRef, useState } from "react";
import { UNDEFINED_PLACEHOLDER } from "system-udfs/convex/_system/frontend/lib/values";
import {
  DatabaseIndexFilterClause,
  Filter,
  isTypeFilterOp,
  typeOf,
} from "system-udfs/convex/_system/frontend/lib/filters";
import { Combobox, Option } from "@ui/Combobox";
import { TextInput } from "@ui/TextInput";
import { cn } from "@ui/cn";
import { DateTimePicker } from "@common/features/data/components/DateTimePicker";
import { ObjectEditor } from "@common/elements/ObjectEditor/ObjectEditor";
import {
  IndexedOperator,
  indexedOperatorOf,
  indexedOperatorOptions,
  isRangeClause,
  scanOperatorOptions,
  unparsedText,
  unparsedValue,
  withIndexedOperator,
} from "./filterModel";

const RANGE_ERROR_MESSAGE =
  "The lower bound is higher than the upper bound, so this filter would never match any documents.";

type ValueEditorProps = {
  field: string;
  value: unknown;
  // `edited` is false for the value the editor reports on mount, which is the
  // one it was seeded with rather than one the user typed.
  onChange(value: Value | undefined, edited: boolean): void;
  onError(errors: string[], shown: boolean): void;
  onApply(): void;
  autoFocus?: boolean;
  validator?: ValidatorJSON;
  shouldSurfaceValidatorErrors?: boolean;
  path: string;
  ariaLabel: string;
};

function ValueEditor({
  field,
  value,
  onChange,
  onError,
  onApply,
  autoFocus,
  validator,
  shouldSurfaceValidatorErrors,
  path,
  ariaLabel,
}: ValueEditorProps) {
  // The text as typed, for handing back the part of it that doesn't parse.
  // `onChangeInnerText` fires on edits only, never for the seeded value.
  const text = useRef<string | undefined>(undefined);

  if (field === "_creationTime") {
    return (
      <DateTimePicker
        aria-label={ariaLabel}
        autoFocus={autoFocus}
        date={typeof value === "number" ? new Date(value) : new Date()}
        onChange={(date) => onChange(date.getTime(), true)}
        onSave={onApply}
        className="w-full rounded-sm border bg-background-secondary px-2 py-1 text-xs focus:border-border-selected"
      />
    );
  }
  const unparsed = unparsedText(value as JSONValue);
  return (
    <ObjectEditor
      className="w-full min-w-4 rounded-sm border focus-within:border-border-selected"
      editorClassname="mt-0 rounded-sm bg-background-secondary px-2 py-1 text-xs"
      size="sm"
      placeholder="unset"
      allowTopLevelUndefined
      disableFolding
      disableFind
      defaultValue={
        value === UNDEFINED_PLACEHOLDER || unparsed !== undefined
          ? undefined
          : (value as Value)
      }
      defaultInnerText={unparsed}
      onChange={(next) => onChange(next, text.current !== undefined)}
      onChangeInnerText={(next) => {
        text.current = next;
      }}
      onError={(errors, shown) => {
        onError(errors, shown);
        // The editor hands back errors instead of a value, so put the text
        // itself in the draft: the chip and a reopened editor keep showing
        // what was typed.
        if (errors.length > 0 && text.current !== undefined) {
          onChange(unparsedValue(text.current) as Value, true);
        }
      }}
      deferErrorsUntilEdit
      path={path}
      autoFocus={autoFocus}
      saveAction={onApply}
      enterSaves
      mode="editField"
      validator={validator}
      shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
      aria-label={ariaLabel}
    />
  );
}

// An unparsed value is already JSON, and `convexToJson` would reject the
// `$`-prefixed key it is marked with.
const toJson = (v?: Value): JSONValue | undefined => {
  if (v === undefined) return undefined;
  const unparsed = unparsedText(v);
  return unparsed === undefined ? convexToJson(v) : unparsedValue(unparsed);
};

export function IndexedClauseEditor({
  field,
  clause,
  isLast,
  onChange,
  onError,
  onApply,
  validator,
  shouldSurfaceValidatorErrors,
  path,
}: {
  field: string;
  clause: DatabaseIndexFilterClause;
  isLast: boolean;
  onChange(clause: DatabaseIndexFilterClause): void;
  onError(errors: string[], shown: boolean): void;
  onApply(): void;
  validator?: ValidatorJSON;
  shouldSurfaceValidatorErrors?: boolean;
  path: string;
}) {
  const op = indexedOperatorOf(clause);
  // Refs let callbacks read the latest values even when both bound editors
  // fire before React re-renders (avoids stale closure on valueErrors/rangeError).
  const valueErrorsRef = useRef<string[]>([]);
  const rangeErrorRef = useRef<string | undefined>(undefined);

  const report = useCallback(
    (
      nextValueErrors: string[],
      nextRangeError: string | undefined,
      shown = true,
    ) => {
      valueErrorsRef.current = nextValueErrors;
      rangeErrorRef.current = nextRangeError;
      onError(
        [...nextValueErrors, ...(nextRangeError ? [nextRangeError] : [])],
        shown,
      );
    },
    [onError],
  );

  const checkRange = (lower: unknown, upper: unknown) =>
    lower !== undefined &&
    lower !== null &&
    upper !== undefined &&
    upper !== null &&
    typeof lower === typeof upper &&
    (lower as any) > (upper as any)
      ? RANGE_ERROR_MESSAGE
      : undefined;

  const options = indexedOperatorOptions.map((o) => ({
    ...o,
    disabled: !isLast && o.value !== "eq",
  }));

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <span className="shrink-0 font-mono text-xs">{field}</span>
        <Combobox
          label="Operator"
          size="sm"
          optionsWidth="fixed"
          disableSearch
          options={options}
          selectedOption={op}
          setSelectedOption={(next: IndexedOperator | null) => {
            if (!next || next === op) return;
            report([], undefined);
            onChange(withIndexedOperator(clause, next));
          }}
          buttonClasses="w-fit"
          innerButtonClasses="text-xs"
        />
        {!isLast && (
          <span className="text-xs text-content-tertiary">
            Only the last indexed filter can be a range.
          </span>
        )}
      </div>
      {!isRangeClause(clause) ? (
        <ValueEditor
          key={`eq-${field}`}
          field={field}
          value={clause.value}
          autoFocus
          onChange={(v) => onChange({ ...clause, value: toJson(v) })}
          onError={(errors, shown) => report(errors, undefined, shown)}
          onApply={onApply}
          validator={validator}
          shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
          path={`${path}-eq`}
          ariaLabel={`${field} value`}
        />
      ) : op === "between" ? (
        <div className="flex flex-col gap-1">
          <ValueEditor
            key={`lower-${field}`}
            field={field}
            value={clause.lowerValue}
            autoFocus
            onChange={(v, edited) => {
              const lowerValue = toJson(v);
              onChange({ ...clause, lowerValue });
              report(
                valueErrorsRef.current,
                checkRange(lowerValue, clause.upperValue),
                edited,
              );
            }}
            onError={(errors, shown) =>
              report(errors, rangeErrorRef.current, shown)
            }
            onApply={onApply}
            validator={validator}
            shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
            path={`${path}-lower`}
            ariaLabel={`${field} lower bound`}
          />
          <span className="pl-1 text-xs text-content-secondary">and</span>
          <ValueEditor
            key={`upper-${field}`}
            field={field}
            value={clause.upperValue}
            onChange={(v, edited) => {
              const upperValue = toJson(v);
              onChange({ ...clause, upperValue });
              report(
                valueErrorsRef.current,
                checkRange(clause.lowerValue, upperValue),
                edited,
              );
            }}
            onError={(errors, shown) =>
              report(errors, rangeErrorRef.current, shown)
            }
            onApply={onApply}
            validator={validator}
            shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
            path={`${path}-upper`}
            ariaLabel={`${field} upper bound`}
          />
        </div>
      ) : (
        <ValueEditor
          key={`${op}-${field}`}
          field={field}
          value={clause.lowerOp ? clause.lowerValue : clause.upperValue}
          autoFocus
          onChange={(v) =>
            onChange(
              clause.lowerOp
                ? { ...clause, lowerValue: toJson(v) }
                : { ...clause, upperValue: toJson(v) },
            )
          }
          onError={(errors, shown) => report(errors, undefined, shown)}
          onApply={onApply}
          validator={validator}
          shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
          path={`${path}-${op}`}
          ariaLabel={`${field} value`}
        />
      )}
    </div>
  );
}

const typeOptions: Option<string>[] = [
  "string",
  "boolean",
  "number",
  "bigint",
  "null",
  "object",
  "array",
  "id",
  "bytes",
  "unset",
].map((t) => ({ value: t, label: t }));

export function ScanClauseEditor({
  clause,
  defaultDocument,
  onChange,
  onError,
  onApply,
  validator,
  shouldSurfaceValidatorErrors,
  path,
}: {
  clause: Filter;
  defaultDocument: GenericDocument;
  onChange(clause: Filter): void;
  onError(errors: string[], shown: boolean): void;
  onApply(): void;
  validator?: ValidatorJSON;
  shouldSurfaceValidatorErrors?: boolean;
  path: string;
}) {
  // Bumped whenever the operator changes so the value editor is remounted
  // with the freshly derived default value.
  const [editorKey, setEditorKey] = useState(0);
  const reset = (next: Filter) => {
    onError([], true);
    setEditorKey((k) => k + 1);
    onChange(next);
  };
  const op = clause.op === "anyOf" || clause.op === "noneOf" ? "eq" : clause.op;
  const { field } = clause;

  const selectOperator = (next: Filter["op"] | null) => {
    if (next === null || next === op || next === "anyOf" || next === "noneOf")
      return;
    if (isTypeFilterOp(next) && !isTypeFilterOp(op)) {
      reset({
        ...clause,
        op: next,
        value: field ? typeOf(defaultDocument[field]) : "unset",
      });
      return;
    }
    if (!isTypeFilterOp(next) && isTypeFilterOp(op)) {
      const sample =
        field && field in defaultDocument
          ? toJson(defaultDocument[field])
          : field === "_id"
            ? ""
            : field === "_creationTime"
              ? Date.now()
              : null;
      reset({ ...clause, op: next, value: sample } as Filter);
      return;
    }
    onChange({ ...clause, op: next } as Filter);
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <span className="shrink-0 font-mono text-xs">{field ?? "field"}</span>
        <Combobox
          label="Operator"
          size="sm"
          optionsWidth="fixed"
          disableSearch
          options={scanOperatorOptions}
          selectedOption={op}
          setSelectedOption={selectOperator}
          buttonClasses="w-fit"
          innerButtonClasses="text-xs"
        />
      </div>
      {isTypeFilterOp(op) ? (
        <Combobox
          label="Type"
          size="sm"
          optionsWidth="fixed"
          disableSearch
          options={typeOptions}
          selectedOption={clause.value as string | undefined}
          setSelectedOption={(v) =>
            v && onChange({ ...clause, op, value: v } as Filter)
          }
          buttonClasses="w-fit"
          innerButtonClasses="text-xs"
        />
      ) : (
        <ValueEditor
          key={`${editorKey}-${op}-${field}`}
          field={field ?? ""}
          value={clause.value}
          autoFocus
          onChange={(v) => onChange({ ...clause, value: toJson(v) } as Filter)}
          onError={onError}
          onApply={onApply}
          validator={validator}
          shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
          path={path}
          ariaLabel="Filter value"
        />
      )}
    </div>
  );
}

export function SearchTextEditor({
  field,
  value,
  onChange,
  onApply,
}: {
  field: string;
  value: string;
  onChange(value: string): void;
  onApply(): void;
}) {
  return (
    <TextInput
      id="search-filter-text"
      label={`Search ${field}`}
      labelHidden
      size="sm"
      type="search"
      autoFocus
      value={value}
      placeholder={`Search ${field}…`}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          onApply();
        }
      }}
      className={cn("text-xs")}
    />
  );
}

export function SearchFilterClauseEditor({
  field,
  value,
  onChange,
  onError,
  onApply,
  validator,
  shouldSurfaceValidatorErrors,
  path,
}: {
  field: string;
  value: unknown;
  onChange(value: Value | undefined): void;
  onError(errors: string[], shown: boolean): void;
  onApply(): void;
  validator?: ValidatorJSON;
  shouldSurfaceValidatorErrors?: boolean;
  path: string;
}) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2 text-xs">
        <span className="shrink-0 font-mono">{field}</span>
        <span className="text-content-secondary">=</span>
      </div>
      <ValueEditor
        field={field}
        value={value}
        autoFocus
        onChange={(v) =>
          onChange(v === undefined ? undefined : (toJson(v) as any))
        }
        onError={onError}
        onApply={onApply}
        validator={validator}
        shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
        path={path}
        ariaLabel={`${field} value`}
      />
    </div>
  );
}
