import { ReactNode } from "react";
import { ClosePanelButton } from "@ui/ClosePanelButton";
import { Panel } from "react-resizable-panels";
import { Modal } from "@ui/Modal";
import { ResizeHandle } from "@common/layouts/SidebarDetailLayout";
import { useIsMobileDevice } from "@ui/useIsMobileDevice";

export interface DataPanelProps {
  title: ReactNode;
  onClose: () => void;
  // Runs before the panel closes; return false to veto (used to confirm
  // discarding unsaved edits). On mobile devices the panel is a Modal whose
  // close is animation-driven, so the veto has to happen on the close request
  // rather than after teardown — passing the confirm as onClose would let the
  // modal animate shut before the user can cancel.
  onBeforeClose?: () => boolean;
  children: ReactNode;
  // When rendered as a modal on mobile devices, fill most of the viewport
  // height instead of sizing to the content. Used by panels whose contents
  // (e.g. the document editor) need room to be useful and manage their own
  // internal scrolling.
  fillHeight?: boolean;
  "data-testid"?: string;
}

export function DataPanel({
  title,
  onClose,
  onBeforeClose,
  children,
  fillHeight = false,
  ...props
}: DataPanelProps) {
  const isMobile = useIsMobileDevice();

  if (isMobile) {
    return (
      <Modal
        onClose={onClose}
        onBeforeClose={onBeforeClose}
        title={title}
        size="lg"
        contentClassName="mx-0"
      >
        <div
          {...props}
          className={fillHeight ? "flex h-[70dvh] flex-col" : undefined}
        >
          {children}
        </div>
      </Modal>
    );
  }

  return (
    <>
      <ResizeHandle direction="left" collapsed={false} className="ml-6" />
      <Panel
        className="z-40 flex h-full max-w-2xl min-w-56 shrink overflow-x-auto"
        defaultSize={20}
      >
        <div
          className="w-full border-l bg-background-secondary shadow-xl"
          {...props}
        >
          <div className="flex h-full max-h-full flex-col">
            <div className="mb-1 px-4 pt-6 sm:px-6">
              <div className="flex flex-wrap items-center justify-between gap-4 gap-y-2">
                <h4 className="flex-1 wrap-break-word">{title}</h4>
                <ClosePanelButton
                  onClose={() => {
                    if (!onBeforeClose || onBeforeClose()) {
                      onClose();
                    }
                  }}
                  className="ml-auto"
                />
              </div>
            </div>
            <div className="flex grow flex-col overflow-y-auto">{children}</div>
          </div>
        </div>
      </Panel>
    </>
  );
}
