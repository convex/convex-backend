import React, { ReactNode, useCallback, useState } from "react";
import {
  Dialog,
  DialogPanel,
  DialogTitle,
  Transition,
  TransitionChild,
  Description,
} from "@headlessui/react";
import classNames from "classnames";
import { ClosePanelButton } from "@ui/ClosePanelButton";
import { cn } from "@ui/cn";

type ModalProps = {
  onClose: () => void;
  // Runs on a close request (Escape, overlay click, or close button) before the
  // exit animation starts. Return false to veto the close, e.g. to keep the
  // modal open after the user cancels a "discard unsaved changes?" prompt. The
  // veto must happen here rather than in onClose because onClose only fires
  // after the modal has already animated shut.
  onBeforeClose?: () => boolean;
  // Ignores overlay clicks and Escape so the modal only closes through the
  // close button or controls in `children`. Use when accidentally dismissing
  // would lose information the user can't get back.
  requireExplicitClose?: boolean;
  title: string | ReactNode;
  description?: string | ReactNode;
  children: ReactNode;
  size?: "sm" | "md" | "lg";
  contentClassName?: string;
};

export function Modal({
  onClose,
  onBeforeClose,
  requireExplicitClose = false,
  title,
  description,
  children,
  size = "sm",
  contentClassName,
}: ModalProps) {
  const [open, setOpen] = useState(true);
  const handleClose = useCallback(() => {
    if (onBeforeClose && !onBeforeClose()) {
      return;
    }
    setOpen(false);
  }, [onBeforeClose]);

  return (
    <Transition show={open} appear afterLeave={onClose}>
      <Dialog
        static
        as="div"
        data-testid="modal"
        className="fixed inset-0 z-40 overflow-hidden"
        open // Real openness status is controlled by Transition above
        onClose={requireExplicitClose ? () => {} : handleClose}
      >
        <div className="flex sm:min-h-screen sm:items-center sm:justify-center sm:px-4">
          <TransitionChild
            enter="ease-out duration-300"
            enterFrom="opacity-0"
            enterTo="opacity-100"
            leave="ease-in duration-200"
            leaveFrom="opacity-100"
            leaveTo="opacity-0"
          >
            <div
              className="fixed inset-0 bg-black/50 transition-opacity"
              data-testid="modal-overlay"
            />
          </TransitionChild>

          <TransitionChild
            enter="ease-out duration-300"
            enterFrom="opacity-0 translate-y-12"
            enterTo="opacity-100 translate-y-0"
            leave="ease-in duration-200"
            leaveFrom="opacity-100 translate-y-0"
            leaveTo="opacity-0 translate-y-12"
          >
            <DialogPanel
              className={classNames(
                "inline-block bg-background-secondary rounded-xl",
                "text-content-primary",
                "text-left shadow-xl dark:border transform",
                "transition-all align-middle",
                "rounded-b-none sm:rounded-b-xl",
                "absolute bottom-0 sm:relative",
                size === "lg"
                  ? "sm:max-w-6xl"
                  : size === "md"
                    ? "sm:max-w-3xl"
                    : "sm:max-w-xl",
                "w-full",
              )}
            >
              {/* Header */}
              <div className="p-6 pb-2">
                <div className="flex items-start justify-between">
                  <div>
                    <DialogTitle as="h4">{title}</DialogTitle>
                    <Description className="mt-1 text-sm">
                      {description}
                    </Description>
                  </div>
                  <ClosePanelButton onClose={handleClose} />
                </div>
              </div>

              {/* Contents */}
              <div
                className={cn(
                  "mx-6 mb-12 max-h-[80dvh] overflow-y-auto sm:mb-6",
                  contentClassName,
                )}
              >
                {children}
              </div>
            </DialogPanel>
          </TransitionChild>
        </div>
      </Dialog>
    </Transition>
  );
}
