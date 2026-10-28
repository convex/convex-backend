import React from "react";
import { cn } from "@ui/cn";
import { ExternalLinkIcon } from "@radix-ui/react-icons";
import { Button, ButtonProps } from "@ui/Button";
import { Sheet } from "@ui/Sheet";

export function EmptySection({
  Icon,
  color = "purple",
  header,
  body,
  action,
  learnMoreButton,
  sheet = true,
}: {
  Icon?: React.FunctionComponent<{ className: string | undefined }>;
  color?: "yellow" | "red" | "purple" | "green" | "none";
  header: string;
  body: React.ReactNode;
  action?: React.ReactNode;
  learnMoreButton?: ButtonProps & { href: string };
  sheet?: boolean;
}) {
  const Parent = sheet ? Sheet : "div";

  return (
    <Parent className="size-full" padding={sheet ? false : undefined}>
      <div className="flex h-full animate-fadeInFromLoading flex-col items-center justify-center p-4 text-center">
        {Icon && (
          <div
            className={cn(
              "mb-4 flex aspect-square h-10.5 shrink-0 items-center justify-center rounded-lg bg-util-accent bg-linear-to-tr shadow-md",
              color === "yellow" && "from-yellow-200 to-util-brand-yellow",
              color === "red" &&
                // eslint-disable-next-line no-restricted-syntax
                "from-red-300 to-util-brand-red",
              color === "purple" && "from-purple-200 to-util-brand-purple",
              color === "green" && "from-util-success to-util-success",
              color === "none" && "bg-transparent shadow-none",
            )}
          >
            <Icon className="size-6 text-white" />
          </div>
        )}
        <h2 className="mx-2 mb-2">{header}</h2>

        <p className="mb-2 max-w-prose text-balance text-content-tertiary">
          {body}
        </p>

        {action}

        {learnMoreButton && (
          <Button
            target="_blank"
            inline
            icon={<ExternalLinkIcon />}
            {...learnMoreButton}
            className={cn(learnMoreButton.className, "text-left text-wrap")}
          />
        )}
      </div>
    </Parent>
  );
}
