import React, { ReactNode, useContext } from "react";
import { HamburgerMenuIcon } from "@radix-ui/react-icons";
import { useMeasure } from "react-use";
import {
  SettingsSidebar,
  SettingsPageKind,
} from "@common/layouts/SettingsSidebar";
import { PageContent } from "@common/elements/PageContent";
import { Popover } from "@ui/Popover";
import { Button } from "@ui/Button";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

export function DeploymentSettingsLayout({
  page,
  children,
}: {
  page: SettingsPageKind;
  children: ReactNode;
}) {
  const sidebar = <SettingsSidebar selectedPage={page} />;
  const [ref, { width }] = useMeasure<HTMLDivElement>();
  const isWide = width > 700;
  // Catch errors thrown by the page contents *inside* the layout so the
  // settings sidebar stays mounted — without this the outer
  // ErrorBoundary in `_app.tsx` replaces the whole settings page and
  // navigation between settings sub-pages breaks. We key by `page` so
  // navigating to a different sub-page resets the boundary.
  const { ErrorBoundary } = useContext(DeploymentInfoContext);

  return (
    <PageContent>
      <div
        className="flex size-full max-h-full flex-col overflow-y-hidden"
        ref={ref}
      >
        {isWide ? (
          <SettingsMenuHeader />
        ) : (
          <Popover
            placement="bottom-start"
            className="bg-background-secondary"
            offset={[0, -4]}
            button={<SettingsMenuButton open={false} />}
          >
            {sidebar}
          </Popover>
        )}
        {/* Make space for the header above */}
        <div className="flex size-full overflow-y-hidden">
          {isWide && sidebar}
          <div className="scrollbar flex w-full min-w-88 grow overflow-auto">
            <div className="flex h-fit grow flex-col gap-6 p-6 sm:max-w-260">
              <ErrorBoundary key={page}>{children}</ErrorBoundary>
            </div>
          </div>
        </div>
      </div>
    </PageContent>
  );
}

function SettingsMenuHeader({ isMenu = false }: { isMenu?: boolean }) {
  return (
    <h2 className="flex w-full items-center gap-2 border-b bg-background-secondary p-4">
      {isMenu ? <HamburgerMenuIcon className="mt-0.5 min-w-4" /> : null}
      <span className="truncate">Deployment Settings</span>
    </h2>
  );
}

function SettingsMenuButton({ open }: { open: boolean }) {
  return (
    <Button
      inline
      focused={open}
      variant="unstyled"
      size="sm"
      className="w-full"
    >
      <SettingsMenuHeader isMenu />
    </Button>
  );
}
