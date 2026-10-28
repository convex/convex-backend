import { omit } from "lodash-es";
import { ReactNode, useState } from "react";
import {
  Menu as HeadlessMenu,
  MenuButton as HeadlessMenuButton,
  MenuItems as HeadlessMenuItems,
  MenuItem as HeadlessMenuItem,
  Portal,
} from "@headlessui/react";
import {
  useFloating,
  autoUpdate,
  offset as offsetMiddleware,
  flip,
  shift,
  Placement,
} from "@floating-ui/react";
import classNames from "classnames";
import { Button, ButtonProps } from "@ui/Button";
import { Key, KeyboardShortcut } from "@ui/KeyboardShortcut";
import { Tooltip, TooltipSide } from "@ui/Tooltip";

export type MenuProps = {
  children: React.ReactElement | (React.ReactElement | null)[];
  buttonProps: ButtonProps;
  placement?: Placement;
  offset?: number;
};

export function Menu({
  children,
  buttonProps,
  placement = "bottom",
  offset = 4,
}: MenuProps) {
  const [referenceElement, setReferenceElement] =
    useState<HTMLButtonElement | null>(null);
  const [popperElement, setPopperElement] = useState<HTMLElement | null>();
  const { floatingStyles } = useFloating({
    placement,
    middleware: [offsetMiddleware(offset), flip(), shift()],
    whileElementsMounted: autoUpdate,
    elements: { reference: referenceElement, floating: popperElement },
  });

  return (
    <HeadlessMenu>
      {({ open }) => (
        <>
          <Tooltip
            tip={buttonProps?.tip}
            side={buttonProps?.tipSide}
            disableHoverableContent={buttonProps?.tipDisableHoverableContent}
          >
            <HeadlessMenuButton
              ref={setReferenceElement}
              as={Button}
              data-testid="open-menu"
              {
                // <HeadlessMenuButton as={Button} tip="…" /> causes a state update loop since Headless UI 2.0
                // (presumably because Headless UI and the tooltip both want to update the ref).
                // To circumvent this issue, we place the <Tooltip /> component as a parent of <HeadlessMenuButton />
                ...omit(
                  buttonProps,
                  "tip",
                  "tipSide",
                  "tipDisableHoverableContent",
                )
              }
              focused={open}
            />
          </Tooltip>
          <Portal>
            <HeadlessMenuItems
              ref={setPopperElement}
              style={floatingStyles}
              className="z-50 flex max-h-128 flex-col gap-1 overflow-auto rounded-lg border bg-background-secondary py-2 text-sm whitespace-nowrap shadow-md focus:outline-hidden"
            >
              {children}
            </HeadlessMenuItems>
          </Portal>
        </>
      )}
    </HeadlessMenu>
  );
}

export function MenuItem({
  variant = "default",
  children,
  action,
  href,
  disabled = false,
  tip,
  tipSide,
  shortcut,
}: {
  variant?: "default" | "danger";
  children: ReactNode;
  disabled?: boolean;
  tip?: ReactNode;
  tipSide?: TooltipSide;
  shortcut?: Key[];
} & (
  | {
      action: () => void;
      href?: never;
    }
  | {
      action?: never;
      href: string;
    }
)) {
  const actionProp = href ? { href } : { onClick: action };

  return (
    <HeadlessMenuItem>
      {({ focus }) => (
        <Button
          tip={tip}
          tipSide={tipSide}
          variant="unstyled"
          className={classNames(
            "mx-1 flex gap-2 items-center p-2 rounded-xs",
            disabled
              ? "cursor-not-allowed fill-content-tertiary text-content-tertiary"
              : "hover:bg-background-tertiary",
            focus && "bg-background-primary",
            !disabled && variant === "danger"
              ? "text-content-errorSecondary"
              : "text-content-primary",
          )}
          disabled={disabled}
          {...actionProp}
        >
          {children}
          {shortcut && (
            <KeyboardShortcut
              value={shortcut}
              className="ml-auto pl-6 text-content-tertiary"
            />
          )}
        </Button>
      )}
    </HeadlessMenuItem>
  );
}

export function MenuLink({
  children,
  href,
  disabled = false,
  selected = false,
  shortcut,
  target,
}: React.PropsWithChildren<{
  href: string;
  disabled?: boolean;
  selected?: boolean;
  shortcut?: Key[];
  target?: "_blank";
}>) {
  return (
    <HeadlessMenuItem disabled={disabled}>
      {({ focus, close }) => (
        <a
          href={href}
          target={target}
          aria-disabled={disabled}
          onClick={disabled ? (e) => e.preventDefault() : () => close()}
          className={classNames(
            "rounded-xs flex gap-2 items-center mx-1 px-2 py-2 text-content-primary",
            disabled &&
              "cursor-not-allowed fill-content-secondary bg-background-tertiary text-content-secondary",

            focus || selected
              ? "bg-background-primary"
              : "hover:bg-background-tertiary",
          )}
        >
          {children}
          {shortcut && (
            <KeyboardShortcut
              value={shortcut}
              className="ml-auto pl-6 text-content-tertiary"
            />
          )}
        </a>
      )}
    </HeadlessMenuItem>
  );
}
