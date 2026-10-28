import classNames from "classnames";
import { useQuery } from "convex/react";
import { Link } from "@ui/Link";
import { ReactNode, useContext, useRef, useState } from "react";
import { useLocalStorage } from "react-use";
import { gt } from "semver";
import udfs from "@common/udfs";
import { useRouter } from "next/router";
import {
  ImperativePanelHandle,
  Panel,
  PanelGroup,
  PanelResizeHandle,
} from "react-resizable-panels";
import {
  DotFilledIcon,
  DragHandleDots2Icon,
  HamburgerMenuIcon,
} from "@radix-ui/react-icons";
import { cn } from "@ui/cn";

import { PageContent } from "@common/elements/PageContent";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { Tooltip } from "@ui/Tooltip";
import { ClosePanelButton } from "@ui/ClosePanelButton";
import { Button } from "@ui/Button";
import { Modal } from "@ui/Modal";
import { useIsNarrowScreen } from "@ui/useIsNarrowScreen";

export function SidebarDetailLayout({
  sidebarComponent,
  contentComponent,
  panelSizeKey,
  resizeHandleTitle,
  mobileBarContent,
}: {
  sidebarComponent: ReactNode;
  contentComponent: ReactNode;
  panelSizeKey: string;
  resizeHandleTitle: string;
  mobileBarContent?: ReactNode;
}) {
  const router = useRouter();

  const cleanPath = router.asPath.split("?")[0];

  const [collapsed, setCollapsed] = useState(false);
  const panelRef = useRef<ImperativePanelHandle>(null);

  const { ErrorBoundary } = useContext(DeploymentInfoContext);

  const isNarrow = useIsNarrowScreen();

  const content = (
    <PageContent>
      <ErrorBoundary key={cleanPath}>
        <div className="h-full animate-fadeInFromLoading overflow-auto">
          {contentComponent}
        </div>
      </ErrorBoundary>
      <NpmConvexServerVersionBanner />
    </PageContent>
  );

  if (isNarrow) {
    return (
      <div className="flex h-full grow flex-col overflow-hidden">
        <div className="flex items-center gap-2 border-b bg-background-secondary px-3 py-1.5">
          {mobileBarContent}
          <MobileSidebarModal
            key={router.asPath}
            title={resizeHandleTitle}
            sidebarComponent={sidebarComponent}
          />
        </div>
        <div className="relative h-full grow overflow-x-auto">{content}</div>
      </div>
    );
  }

  return (
    <div className="flex h-full grow items-stretch overflow-hidden">
      <PanelGroup
        direction="horizontal"
        className="flex h-full grow items-stretch overflow-hidden"
        autoSaveId={panelSizeKey}
      >
        <Panel
          ref={panelRef}
          collapsible
          minSize={10}
          defaultSize={20}
          maxSize={75}
          className={classNames(
            "h-full flex",
            !collapsed && "min-w-[10rem] max-w-[26rem]",
          )}
          collapsedSize={0}
          onCollapse={() => setCollapsed(true)}
          onExpand={() => setCollapsed(false)}
        >
          {!collapsed && sidebarComponent}
        </Panel>

        <ResizeHandle
          collapsed={collapsed}
          direction="right"
          panelRef={panelRef}
          handleTitle={resizeHandleTitle}
        />
        <Panel
          className="relative h-full grow overflow-x-auto"
          defaultSize={80}
        >
          {content}
        </Panel>
      </PanelGroup>
    </div>
  );
}

function MobileSidebarModal({
  title,
  sidebarComponent,
}: {
  title: string;
  sidebarComponent: ReactNode;
}) {
  const [isOpen, setIsOpen] = useState(false);
  return (
    <>
      <Button
        variant="neutral"
        size="sm"
        icon={<HamburgerMenuIcon />}
        onClick={() => setIsOpen(true)}
      >
        {title}
      </Button>
      {isOpen && (
        <Modal
          onClose={() => setIsOpen(false)}
          title={title}
          contentClassName="mx-0"
        >
          <div className="h-[70dvh]">{sidebarComponent}</div>
        </Modal>
      )}
    </>
  );
}

function NpmConvexServerVersionBanner() {
  const upgradeRequiredVersion = "0.19.1";
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const currentVersion = useQuery(
    udfs.getVersion.default,
    canViewData ? undefined : "skip",
  );
  const [dismissedVersion, setDismissedVersion] = useLocalStorage<string>(
    "dismissedVersionNotification",
  );
  const newVersionAvailable =
    upgradeRequiredVersion &&
    currentVersion &&
    gt(upgradeRequiredVersion, currentVersion) &&
    (!dismissedVersion || gt(upgradeRequiredVersion, dismissedVersion))
      ? upgradeRequiredVersion
      : undefined;

  const { useCurrentDeployment } = useContext(DeploymentInfoContext);
  const deployment = useCurrentDeployment();
  const isProd = deployment?.deploymentType === "prod";

  return isProd && newVersionAvailable ? (
    <div className="absolute right-0 bottom-0 flex w-full items-center justify-between border-b bg-background-warning px-5 py-2 text-sm text-content-primary">
      <div>
        This deployment's{" "}
        <Link
          href="https://www.npmjs.com/package/convex"
          passHref
          target="_blank"
        >
          convex package
        </Link>{" "}
        version ({currentVersion}) is deprecated and will no longer be supported
        soon. View{" "}
        <Link
          href="https://news.convex.dev/tag/releases/"
          passHref
          target="_blank"
        >
          release notes.
        </Link>
      </div>
      <Tooltip
        tip="Dismiss this notification until the next update is available."
        side="left"
        asChild
      >
        <ClosePanelButton
          onClose={() => setDismissedVersion(upgradeRequiredVersion)}
        />
      </Tooltip>
    </div>
  ) : null;
}

export function ResizeHandle({
  collapsed,
  direction = "left",
  panelRef,
  className,
  handleTitle,
}: {
  collapsed: boolean;
  direction: "left" | "right";
  panelRef?: React.RefObject<ImperativePanelHandle | null>;
  className?: string;
  handleTitle?: string;
}) {
  const [dragging, setDragging] = useState(false);
  return (
    <PanelResizeHandle
      className={cn("relative", className)}
      onDragging={setDragging}
      hitAreaMargins={{ coarse: 4, fine: 3 }}
    >
      {!collapsed ? (
        <div
          className={cn(
            "flex h-full w-2 items-center justify-center border-l bg-background-secondary/70 transition-all duration-300",
            direction === "right" ? "border-r" : "border-l",
            dragging && "bg-util-accent/10",
          )}
        >
          <div className="flex flex-col gap-0">
            <DotFilledIcon className="text-content-tertiary/50" />
          </div>
        </div>
      ) : (
        <Button
          variant="unstyled"
          onClick={() => panelRef?.current?.expand()}
          disabled={!collapsed}
          className={cn(
            "absolute top-1/2 left-0 z-20 flex -translate-y-1/2 flex-col items-center gap-1 border bg-background-secondary px-0.5 py-2 text-xs transition-all",
            dragging && "border-4 border-util-accent text-content-primary",
            direction === "right"
              ? "rounded-r-md border-l-0"
              : "-ml-5 rounded-l-md border-r-0",
          )}
          icon={<DragHandleDots2Icon className="text-content-secondary" />}
        >
          {handleTitle && collapsed && (
            <span
              style={{ writingMode: "vertical-rl" }}
              className={cn(
                direction === "right" && "rotate-180",
                dragging ? "text-content-primary" : "text-content-secondary",
              )}
            >
              {handleTitle}
            </span>
          )}
        </Button>
      )}
    </PanelResizeHandle>
  );
}
