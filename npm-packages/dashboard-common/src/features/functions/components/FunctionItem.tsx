import React, { useContext } from "react";
import { LockClosedIcon } from "@radix-ui/react-icons";
import { useRouter } from "next/router";
import { cn } from "@ui/cn";
import { DisclosureButton } from "@headlessui/react";
import { ModuleFunction } from "@common/lib/functions/types";
import { useCurrentOpenFunction } from "@common/lib/functions/FunctionsProvider";
import { useCurrentGloballyOpenFunction } from "@common/features/functionRunner/lib/functionRunner";
import { FunctionIcon } from "@common/elements/icons";
import { Tooltip } from "@ui/Tooltip";
import { Button } from "@ui/Button";
import { sidebarLinkClassNames } from "@common/elements/Sidebar";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

export function FunctionItem({
  item,
  showFileName,
  onChangeFunction,
  nestingLevel,
}: {
  item: ModuleFunction;
  showFileName?: boolean;
  onChangeFunction: () => void;
  nestingLevel: number;
}) {
  const router = useRouter();
  const currentOpenFunction = useCurrentOpenFunction();
  const { useLogDeploymentEvent } = useContext(DeploymentInfoContext);
  const log = useLogDeploymentEvent();
  const [, setGloballyOpenFunction] = useCurrentGloballyOpenFunction();
  const isCurrentFunction = currentOpenFunction?.identifier === item.identifier;
  return (
    <DirectoryItem
      href={{ query: { ...router.query, function: item.displayName } }}
      onClick={() => {
        onChangeFunction();
        setGloballyOpenFunction(item);
        log("change function");
      }}
      isActive={isCurrentFunction}
      nestingLevel={nestingLevel}
      key={item.name}
    >
      <FunctionIcon className="size-4 shrink-0 text-content-tertiary" />
      <div className="grow truncate">
        {/* If set to show the full file name, and the file name is not the default export, concatenate the file name with the function name */}
        {showFileName &&
        item.file.name !== item.name &&
        // HTTP actions have unique names
        item.udfType !== "HttpAction"
          ? `${item.file.name}:${item.name}`
          : item.name}
      </div>
      {item.visibility.kind !== "public" && (
        <Tooltip tip="This is an internal function." side="right">
          <LockClosedIcon className="size-3 shrink-0 text-content-tertiary" />
        </Tooltip>
      )}
    </DirectoryItem>
  );
}

function paddingForLevel(level: number) {
  if (level >= 1) {
    return 28;
  }
  return 12;
}

export function DirectoryItem({
  href,
  onClick,
  isActive = false,
  nestingLevel,
  children,
  disclosure = false,
}: {
  href?: { query: { [key: string]: string } };
  onClick?: () => void;
  isActive?: boolean;
  nestingLevel: number;
  children: React.ReactNode[];
  disclosure?: boolean;
}) {
  const { captureMessage } = useContext(DeploymentInfoContext);

  const className = cn(
    sidebarLinkClassNames({
      isActive,
      font: "mono",
      small: true,
    }),
    "h-[30px] w-full max-w-full min-w-full truncate p-0 pr-2",
    "rounded-none",
    isActive &&
      "bg-util-accent/30 font-normal outline outline-util-accent/40 hover:bg-util-accent/30",
    !isActive && "hover:bg-util-accent/20",
    "focus-visible:bg-util-accent/20 focus-visible:ring-0 focus-visible:outline-hidden",
  );

  const buttonChildren = (
    <>
      <div
        className="flex h-full items-center gap-4"
        style={{
          paddingLeft: `${paddingForLevel(nestingLevel)}px`,
        }}
      >
        {nestingLevel !== 0 &&
          Array.from({ length: nestingLevel }).map((_, index) => (
            <div
              key={index}
              className="h-full w-px shrink-0 bg-border-transparent"
            />
          ))}
      </div>
      {children}
    </>
  );

  if (disclosure) {
    if (href) {
      captureMessage("DirectoryItem with href and disclosure", "error");
    }

    return (
      <DisclosureButton className={className} onClick={onClick}>
        {buttonChildren}
      </DisclosureButton>
    );
  }

  return (
    <Button
      variant="unstyled"
      className={className}
      href={href}
      onClickOfAnchorLink={onClick}
    >
      {buttonChildren}
    </Button>
  );
}
