import { ROUTABLE_HTTP_METHODS } from "convex/server";
import {
  displayName,
  functionIdentifierFromValue,
} from "@common/lib/functions/generateFileTree";
import { Tooltip } from "@ui/Tooltip";
import { PuzzlePieceIcon } from "@common/elements/icons";

function splitFunctionName(functionName: string) {
  for (const method of ROUTABLE_HTTP_METHODS) {
    if (functionName.startsWith(`${method} `)) {
      return {
        secondary: `${method} `,
        primary: functionName.substring(method.length + 1),
      };
    }
  }

  const separatorPos = functionName.indexOf(":");
  if (separatorPos === -1) {
    return { secondary: "", primary: functionName };
  }

  return {
    secondary: functionName.substring(0, separatorPos + 1),
    primary: functionName.substring(separatorPos + 1),
  };
}

const MAX_CHARS_SHOWN = 36;

export function FunctionNameOption({
  label,
  oneLine = false,
  disableTruncation = false,
  maxChars = MAX_CHARS_SHOWN,
  error = false,
}: {
  label: string;
  oneLine?: boolean;
  disableTruncation?: boolean;
  maxChars?: number;
  error?: boolean;
}) {
  const functionIdentifier = functionIdentifierFromValue(label);
  const { componentPath } = functionIdentifier;
  let { primary, secondary } = splitFunctionName(
    displayName(functionIdentifier.identifier),
  );

  const fullName = secondary + primary;

  // Leave some room for the component icon if there is a component path.
  const maxCharsShown = componentPath ? maxChars - 2 : maxChars;

  const isTruncated =
    !disableTruncation &&
    (primary.length > maxCharsShown ||
      secondary.length + primary.length > maxCharsShown);

  if (primary.length > maxCharsShown) {
    primary = `…${primary.slice(primary.length - maxCharsShown + 1)}`;

    secondary = "";
  } else if (secondary.length + primary.length > maxCharsShown) {
    secondary = `…${secondary.slice(
      secondary.length - (maxCharsShown - primary.length) + 1,
    )}`;
  }

  if (primary === "_other" && secondary === "") {
    return <span className="w-full">Other functions</span>;
  }

  const content = (
    <div className="flex w-full items-center space-x-1">
      {componentPath && (
        <Tooltip tip={componentPath} className="shrink-0">
          <PuzzlePieceIcon />
        </Tooltip>
      )}
      <span
        aria-label={fullName}
        className={oneLine ? "min-w-0 truncate" : undefined}
      >
        <span
          className={error ? "text-content-error" : "text-content-secondary"}
        >
          {secondary}
        </span>
        <span className={error ? "text-content-error" : "text-content-primary"}>
          {primary}
        </span>
      </span>
    </div>
  );

  if (isTruncated) {
    return <Tooltip tip={fullName}>{content}</Tooltip>;
  }

  return content;
}
