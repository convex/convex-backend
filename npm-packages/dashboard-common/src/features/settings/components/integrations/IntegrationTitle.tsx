import { Tooltip } from "@ui/Tooltip";
import { ReactNode } from "react";
import { IntegrationType } from "system-udfs/convex/_system/frontend/common";
import {
  integrationCategory,
  integrationName,
} from "@common/lib/integrationHelpers";

export function IntegrationTitle({
  logo,
  integrationKind,
  description,
}: {
  logo: ReactNode;
  integrationKind: IntegrationType;
  description: ReactNode;
}) {
  return (
    <div className="flex items-center gap-2">
      {logo}

      <p className="text-sm font-semibold">
        {integrationName(integrationKind)}
      </p>
      <Tooltip tip={description}>
        <p className="max-w-fit rounded-sm border p-1 text-xs">
          {integrationCategory(integrationKind)}
        </p>
      </Tooltip>
    </div>
  );
}
