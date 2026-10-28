import { ValidatorJSON, Value } from "convex/values";
import { useCallback, useEffect, useRef, useState } from "react";
import { UNDEFINED_PLACEHOLDER } from "system-udfs/convex/_system/frontend/lib/values";
import { stringifyValue } from "@common/lib/stringifyValue";
import { cn } from "@ui/cn";
import { processCode } from "./ObjectEditor";
import { ConvexSchemaValidationError } from "./ast/types";

// Parse and validate editor text the same way the Monaco editor does, but
// return plain error message strings instead of driving Monaco markers, since a
// textarea has no marker API.
function parseObjectEditorCode(
  code: string,
  mode: "editField" | "addDocuments" | "editDocument" | "patchDocuments",
  validator: ValidatorJSON | undefined,
  allowTopLevelUndefined: boolean,
  shouldSurfaceValidatorErrors: boolean,
): { value?: Value; errors: string[] } {
  try {
    const { value, errors } = processCode(
      code,
      mode,
      validator,
      allowTopLevelUndefined,
      shouldSurfaceValidatorErrors,
    );
    const messages = errors
      .filter(
        (e) =>
          shouldSurfaceValidatorErrors ||
          !(e instanceof ConvexSchemaValidationError),
      )
      .map((e) => e.message);
    return { value: messages.length ? undefined : value, errors: messages };
  } catch (e: any) {
    if (e instanceof SyntaxError) {
      const result = e.message.match(/(.*) \(([0-9]+):([0-9]+)\)/);
      return { errors: [result ? result[1] : e.message] };
    }
    throw e;
  }
}

export type PlainObjectEditorProps = {
  defaultValue?: Value;
  defaultInnerText?: string;
  onChange(v?: Value): void;
  onError(errors: string[], shown: boolean): void;
  onChangeInnerText?(v: string): void;
  deferErrorsUntilEdit?: boolean;
  mode: "editField" | "addDocuments" | "editDocument" | "patchDocuments";
  validator?: ValidatorJSON;
  allowTopLevelUndefined?: boolean;
  shouldSurfaceValidatorErrors?: boolean;
  disabled?: boolean;
  autoFocus?: boolean;
  // Fill the parent's height instead of using the default min height.
  fullHeight?: boolean;
  // Called when Ctrl/Cmd+Enter (or Enter, if enterSaves) is pressed.
  saveAction?(): void;
  enterSaves?: boolean;
  placeholder?: string;
  className?: string;
  "aria-label"?: string;
};

/**
 * A plain <textarea> alternative to the Monaco-based ObjectEditor, running the
 * same parsing and validation. Monaco is hard to use on touch devices (tapping
 * to focus, selection, and paste are all unreliable), so ObjectEditor renders
 * this instead on narrow screens.
 */
export function PlainObjectEditor({
  defaultValue,
  defaultInnerText,
  onChange,
  onError,
  onChangeInnerText,
  mode,
  validator,
  allowTopLevelUndefined = false,
  deferErrorsUntilEdit = false,
  shouldSurfaceValidatorErrors = false,
  disabled = false,
  autoFocus = false,
  fullHeight = false,
  saveAction,
  enterSaves = false,
  placeholder,
  className,
  "aria-label": ariaLabel,
}: PlainObjectEditorProps) {
  const [text, setText] = useState(() =>
    defaultInnerText !== undefined
      ? defaultInnerText
      : defaultValue === undefined || defaultValue === UNDEFINED_PLACEHOLDER
        ? ""
        : stringifyValue(defaultValue, true),
  );
  const [hasError, setHasError] = useState(false);
  const edited = useRef(false);

  const saveActionRef = useRef(saveAction);
  useEffect(() => {
    saveActionRef.current = saveAction;
  }, [saveAction]);

  const validate = useCallback(
    (code: string) => {
      const { value, errors } = parseObjectEditorCode(
        code,
        mode,
        validator,
        allowTopLevelUndefined,
        shouldSurfaceValidatorErrors,
      );
      const shown = !deferErrorsUntilEdit || edited.current;
      setHasError(shown && errors.length > 0);
      onError(errors, shown);
      if (errors.length === 0) {
        onChange(value);
      }
    },
    [
      mode,
      validator,
      allowTopLevelUndefined,
      deferErrorsUntilEdit,
      shouldSurfaceValidatorErrors,
      onError,
      onChange,
    ],
  );

  // Surface the initial value's parsed result and any errors on mount, matching
  // the Monaco editor's behavior.
  useEffect(() => {
    validate(text);
    // Only on mount; remounts (via `key`) re-seed the value.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <textarea
      aria-label={ariaLabel}
      value={text}
      disabled={disabled}
      autoFocus={autoFocus}
      placeholder={placeholder}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
      className={cn(
        "scrollbar w-full resize-none rounded-sm border bg-background-secondary px-2 py-1 font-mono text-content-primary",
        "focus:border-border-selected focus:outline-hidden",
        "disabled:cursor-not-allowed disabled:bg-background-tertiary disabled:text-content-secondary",
        fullHeight ? "h-full" : "min-h-16",
        hasError && "border-content-error",
        className,
      )}
      onChange={(e) => {
        const code = e.target.value;
        edited.current = true;
        setText(code);
        onChangeInnerText?.(code);
        validate(code);
      }}
      onKeyDown={(e) => {
        if (
          saveActionRef.current &&
          e.key === "Enter" &&
          (enterSaves ? !e.shiftKey : e.metaKey || e.ctrlKey)
        ) {
          e.preventDefault();
          saveActionRef.current();
        }
      }}
    />
  );
}
