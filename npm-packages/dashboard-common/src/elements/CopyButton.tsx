import React, { useState } from "react";
import { CopyIcon } from "@radix-ui/react-icons";
import { copyTextToClipboard } from "@common/lib/utils";
import { Button, ButtonSize } from "@ui/Button";

export function CopyButton({
  text,
  className,
  inline = false,
  tip,
  tipSide,
  disabled,
  onCopied,
  size = "xs",
  label = "Copy",
}: {
  text: string;
  className?: string;
  inline?: boolean;
  tip?: string;
  tipSide?: "top" | "bottom" | "left" | "right";
  disabled?: boolean;
  onCopied?: () => void;
  size?: ButtonSize;
  label?: string;
}) {
  const [copied, setCopied] = useState(false);

  const copyText = () => {
    let canceled = false;
    void (async () => {
      if (text) {
        await copyTextToClipboard(text);
        if (canceled) return;
        setCopied(true);
        onCopied?.();

        setTimeout(() => {
          if (canceled) return;
          setCopied(false);
        }, 2000);
      }
    })();
    return () => {
      canceled = true;
    };
  };

  return (
    <Button
      onClick={copyText}
      size={size}
      icon={<CopyIcon />}
      variant="neutral"
      inline={inline}
      className={className}
      tip={tip}
      tipSide={tipSide}
      disabled={disabled}
    >
      {copied ? "Copied!" : !inline && label}
    </Button>
  );
}
