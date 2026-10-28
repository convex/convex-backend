import React, { Fragment, MutableRefObject, useEffect, useState } from "react";
import {
  useFloating,
  autoUpdate,
  offset as offsetMiddleware,
  flip,
  shift,
  Placement,
} from "@floating-ui/react";
import {
  Popover as HeadlessPopover,
  PopoverPanel as HeadlessPopoverPanel,
  PopoverButton as HeadlessPopoverButton,
  Portal,
} from "@headlessui/react";
import classNames from "classnames";
import { cn } from "@ui/cn";
import { useIsNarrowScreen } from "./useIsNarrowScreen";

// Copied from HeadlessUI Types
type FunctionalChild = (bag: {
  open: boolean;
  close: (
    focusableElement?: HTMLElement | MutableRefObject<HTMLElement | null>,
  ) => void;
}) => React.ReactElement<any, string | React.JSXElementConstructor<any>>;

type PopoverProps = {
  children: React.ReactNode | FunctionalChild;
  className?: string;
  button: React.ReactNode | FunctionalChild;
  placement?: Placement;
  offset?: [number | null | undefined, number | null | undefined];
  onOpen?(): void;
  onClose?(): void;
  // If true, will render in a Portal
  portal?: boolean;
  padding?: boolean;
  focus?: boolean;
  /**
   * On narrow screens, shows the panel as a sheet along the bottom of the
   * screen over a dimmed backdrop, like `Combobox` does, instead of next to
   * the button. The sheet is always portaled.
   */
  sheetOnNarrowScreens?: boolean;
} & (
  | {
      asChild?: false;
      /** Carried by the wrapper around `button` while the panel is open. */
      openButtonClassName?: string;
    }
  | {
      /**
       * Hands the trigger's behavior to `button` itself instead of wrapping it
       * in a div. The wrapper carries HeadlessUI's `aria-expanded`, which
       * belongs on the control it describes, so a `button` that is already a
       * real button should claim it.
       */
      asChild: true;
      /**
       * There is no wrapper to carry it, and HeadlessUI's types have no
       * `className` to hand a fragment. `button` is given `open` instead, so
       * the trigger styles its own open state.
       */
      openButtonClassName?: never;
    }
);

export function Popover({
  className,
  openButtonClassName = "",
  children,
  button,
  asChild = false,
  placement = "bottom",
  offset = [0, 8],
  onOpen,
  onClose,
  portal,
  padding = true,
  focus = false,
  sheetOnNarrowScreens = false,
}: PopoverProps) {
  const isSheet = useIsNarrowScreen() && sheetOnNarrowScreens;
  const [referenceElement, setReferenceElement] =
    useState<HTMLButtonElement | null>(null);
  const [popperElement, setPopperElement] = useState<HTMLElement | null>();
  const { floatingStyles } = useFloating({
    placement,
    middleware: [
      offsetMiddleware({ mainAxis: offset[1] ?? 0, crossAxis: offset[0] ?? 0 }),
      flip(),
      shift(),
    ],
    whileElementsMounted: autoUpdate,
    elements: { reference: referenceElement, floating: popperElement },
  });

  useEffect(() => {
    const isOpen = !!popperElement;
    const fn = isOpen ? onOpen : onClose;
    fn?.();
  }, [popperElement, onOpen, onClose]);

  return (
    <HeadlessPopover>
      {({ open, close }) => {
        const panel = isSheet ? (
          // Headless UI renders the panel only while open, but not this
          // wrapper, whose backdrop would otherwise cover the page.
          open && (
            <div className="fixed inset-0 z-50 flex items-end">
              {/* Headless UI counts the popover's portal as inside it, so a
                click on the backdrop wouldn't close the panel on its own.
                Escape closes it from the keyboard. */}
              <div
                className="absolute inset-0 bg-black/50"
                aria-hidden="true"
                onClick={() => close()}
              />
              <HeadlessPopoverPanel
                ref={setPopperElement}
                focus={focus}
                className={cn(
                  "border bg-background-secondary",
                  padding && "px-5 py-4",
                  className,
                  "relative w-full rounded-t-xl border-b-0 shadow-xl",
                )}
              >
                {children}
              </HeadlessPopoverPanel>
            </div>
          )
        ) : (
          <HeadlessPopoverPanel
            ref={setPopperElement}
            style={floatingStyles}
            focus={focus}
            className={classNames(
              "z-50 bg-background-secondary shadow-md border rounded-lg",
              padding && "py-4 px-5",
              className,
            )}
          >
            {children}
          </HeadlessPopoverPanel>
        );
        return (
          <>
            {asChild ? (
              <HeadlessPopoverButton as={Fragment} ref={setReferenceElement}>
                {button as any /* TODO(react-18-upgrade) */}
              </HeadlessPopoverButton>
            ) : (
              <HeadlessPopoverButton
                ref={setReferenceElement}
                as="div"
                className={open ? openButtonClassName : ""}
              >
                {button as any /* TODO(react-18-upgrade) */}
              </HeadlessPopoverButton>
            )}
            {portal || isSheet ? <Portal>{panel}</Portal> : panel}
          </>
        );
      }}
    </HeadlessPopover>
  );
}
