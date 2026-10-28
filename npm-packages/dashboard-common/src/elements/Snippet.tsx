import { CopyIcon } from "@radix-ui/react-icons";
import classNames from "classnames";
import { useCallback } from "react";
import { Button } from "@ui/Button";
import { useCopy } from "@common/lib/useCopy";

type Props = {
  value: string;
  monospace?: boolean;
  className?: string;
  copying?: string; // used for success and error toasts
};

export function Snippet({ value, monospace, className, copying }: Props) {
  const copyToClipboard = useCopy(copying || "");
  const copy = useCallback(
    () => copyToClipboard(value),
    [value, copyToClipboard],
  );
  const innerClasses = classNames(
    "block w-0 placeholder-content-secondary",
    "text-content-primary text-xs shrink grow whitespace-nowrap overflow-auto",
    "scrollbar-none",
  );

  return (
    <div
      className={classNames(
        "rounded-sm border bg-background-secondary flex items-center justify-between pl-2 py-1",
        className,
      )}
    >
      {monospace ? (
        <pre className={innerClasses}>{value}</pre>
      ) : (
        <span className={innerClasses}>{value}</span>
      )}
      {copying && (
        <Button
          size="sm"
          onClick={copy}
          className="float-right mr-1"
          variant="neutral"
          inline
          icon={<CopyIcon />}
          title="Copy"
        />
      )}
    </div>
  );
}
