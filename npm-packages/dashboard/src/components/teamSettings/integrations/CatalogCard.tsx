import { BookOpenIcon } from "@heroicons/react/24/outline";
import { DotsVerticalIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Link } from "@ui/Link";
import { Menu } from "@ui/Menu";
import { Modal } from "@ui/Modal";
import { Sheet } from "@ui/Sheet";
import { cn } from "@ui/cn";
import {
  integrationCategory,
  integrationName,
  integrationToLogo,
} from "@common/lib/integrationHelpers";
import { ReactNode, useState } from "react";
import { IntegrationType } from "system-udfs/convex/_system/frontend/common";

/** The chrome every card in the integrations catalog shares. */
export function CatalogCard({
  logo,
  name,
  category,
  menu,
  description,
  footer,
}: {
  logo: ReactNode;
  name: string;
  category: string;
  /** The card's actions, in its top-right corner. */
  menu: ReactNode;
  description: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <Sheet padding={false} className="flex flex-col gap-3 p-4">
      <div className="flex items-start gap-3">
        {logo}
        <div className="min-w-0 grow">
          <h3 className="text-sm font-semibold">{name}</h3>
          <p className="text-xs text-content-secondary">{category}</p>
        </div>
        {menu}
      </div>
      <p className="line-clamp-2 grow text-content-secondary">{description}</p>
      {/* Reserved even when empty, so a card keeps its height as its status
          comes and goes. */}
      <div className="flex min-h-4 items-center justify-between gap-2">
        {footer}
      </div>
    </Sheet>
  );
}

export function CatalogMenu({
  name,
  children,
}: {
  name: string;
  children: React.ReactElement | (React.ReactElement | null)[];
}) {
  return (
    <Menu
      placement="bottom-end"
      buttonProps={{
        "aria-label": `${name} options`,
        icon: <DotsVerticalIcon />,
        size: "xs",
        variant: "neutral",
        inline: true,
      }}
    >
      {children}
    </Menu>
  );
}

export function CatalogStatus({
  dot,
  children,
}: {
  dot?: string;
  children: ReactNode;
}) {
  return (
    <span className="flex min-w-0 items-center gap-1.5 text-xs text-content-secondary">
      {dot && <span className={cn("size-2 shrink-0 rounded-full", dot)} />}
      <span className="truncate">{children}</span>
    </span>
  );
}

export function DeploymentIntegrationCard({
  kind,
  description,
  docsUrl,
}: {
  kind: IntegrationType;
  description: string;
  docsUrl: string;
}) {
  const [showDetails, setShowDetails] = useState(false);
  const name = integrationName(kind);
  return (
    <>
      <CatalogCard
        logo={integrationToLogo(kind).logo}
        name={name}
        category={integrationCategory(kind)}
        description={description}
        menu={
          <Button
            size="xs"
            variant="neutral"
            inline
            icon={<BookOpenIcon className="size-4" />}
            aria-label={`How to set up ${name}`}
            tip="Setup instructions"
            onClick={() => setShowDetails(true)}
          />
        }
      />
      {showDetails && (
        <Modal
          title={
            <span className="flex items-center gap-2">
              {integrationToLogo(kind, true).logo}
              {name}
            </span>
          }
          onClose={() => setShowDetails(false)}
        >
          <div className="flex flex-col gap-3 text-sm">
            <p>{description}</p>
            <p className="text-content-secondary">
              Each deployment has its own {name} configuration. Open the
              deployment, then go to Settings › Integrations to set it up.
            </p>
            <Link href={docsUrl} target="_blank">
              Set up {name} in the Convex docs
            </Link>
          </div>
        </Modal>
      )}
    </>
  );
}
