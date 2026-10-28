import { CopyIcon } from "@radix-ui/react-icons";
import classNames from "classnames";
import { useState } from "react";
import { copyTextToClipboard } from "@common/lib/utils";
import { Button } from "@ui/Button";

export function CopyTextButton({
  text,
  className,
  textHidden,
}: {
  text: string;
  className?: string;
  textHidden?: boolean;
}) {
  const [copied, setCopied] = useState(false);
  const copyText = () => {
    let canceled = false;
    void (async () => {
      if (text) {
        await copyTextToClipboard(text);
        if (canceled) return;
        setCopied(true);

        setTimeout(() => {
          if (canceled) return;
          setCopied(false);
        }, 1000);
      }
    })();
    return () => {
      canceled = true;
    };
  };

  return (
    <div className="group relative flex items-center gap-1">
      <Button
        variant="neutral"
        size="xs"
        className={classNames("text-xs transition-all", className)}
        onClick={copyText}
      >
        <span>{textHidden ? "•".repeat(text.length) : text}</span>
        <span className="absolute right-1 bottom-1.5 hidden items-center justify-center rounded-sm bg-background-primary/20 backdrop-blur-[2px] group-hover:flex">
          {!copied ? <CopyIcon /> : "Copied!"}
        </span>
      </Button>
    </div>
  );
}
