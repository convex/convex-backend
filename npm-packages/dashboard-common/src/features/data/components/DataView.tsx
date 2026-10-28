import React, { useContext, useEffect, useMemo } from "react";
import { useQuery } from "convex/react";
import { useRouter } from "next/router";
import udfs from "@common/udfs";
import { SidebarDetailLayout } from "@common/layouts/SidebarDetailLayout";
import { EmptyData } from "@common/features/data/components/EmptyData";
import {
  DataContent,
  DataContentSkeleton,
} from "@common/features/data/components/DataContent";
import {
  DataSidebar,
  DataSideBarSkeleton,
} from "@common/features/data/components/DataSidebar";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { NentSwitcher } from "@common/elements/NentSwitcher";
import { useTableMetadataAndUpdateURL } from "@common/lib/useTableMetadata";
import { useNents } from "@common/lib/useNents";
import { SchemaJson } from "@common/lib/format";
import { LoadingTransition } from "@ui/Loading";
import { DeploymentPageTitle } from "@common/elements/DeploymentPageTitle";
import { NoPermissionMessage } from "@common/elements/NoPermissionMessage";
import { FiltersAppliedProperties } from "@common/features/data/lib/filterAnalytics";
import { useDataPageSize } from "./Table/utils/useQueryFilteredTable";

export function DataView({
  onTableCreated,
  onDocumentsAdded,
  onFiltersApplied,
}: {
  onTableCreated?: () => void;
  onDocumentsAdded?: (count: number) => void;
  onFiltersApplied?: (properties: FiltersAppliedProperties) => void;
}) {
  const { useCurrentDeployment, ErrorBoundary, deploymentsURI } = useContext(
    DeploymentInfoContext,
  );

  // Older CLI versions link to `/data?showSchema=true` to show schema push
  // progress; that view lives on the Schema page, so forward them there.
  const router = useRouter();
  useEffect(() => {
    if (router.query.showSchema) {
      const query: Record<string, string> = { showSchema: "true" };
      if (typeof router.query.component === "string") {
        query.component = router.query.component;
      }
      void router.replace({ pathname: `${deploymentsURI}/schema`, query });
    }
  }, [router, deploymentsURI]);
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const deployment = useCurrentDeployment() ?? {
    id: undefined,
    kind: undefined,
  };

  const deploymentId = deployment && "id" in deployment ? deployment.id : null;

  const tableMetadata = useTableMetadataAndUpdateURL();

  const canViewData = useIsOperationAllowed("ViewData");

  const componentId = useNents().selectedNent?.id;
  const schemas = useQuery(
    udfs.getSchemas.default,
    canViewData ? { componentId: componentId ?? null } : "skip",
  );

  const [currentPageSize, setPageSize] = useDataPageSize(
    componentId ?? null,
    tableMetadata?.name ?? "",
  );

  const { activeSchema } = useMemo(() => {
    if (!schemas) {
      return { activeSchema: undefined };
    }

    return {
      activeSchema: schemas.active
        ? (JSON.parse(schemas.active) as SchemaJson)
        : null,
    };
  }, [schemas]);

  if (!canViewData) {
    return (
      <>
        <DeploymentPageTitle title="Data" />
        <NoPermissionMessage
          message="You do not have permission to view data in this deployment."
          missingPermission="deployment:data:view"
        />
      </>
    );
  }

  return (
    <>
      <DeploymentPageTitle
        subtitle={tableMetadata?.name ? "Data" : undefined}
        title={tableMetadata?.name || "Data"}
      />
      <LoadingTransition
        loadingProps={{ shimmer: false }}
        loadingState={
          <div className="size-full">
            <div className="flex h-full">
              <SidebarDetailLayout
                resizeHandleTitle="Tables"
                panelSizeKey={`${deploymentId}/data`}
                sidebarComponent={<DataSideBarSkeleton />}
                contentComponent={<DataContentSkeleton />}
              />
            </div>
          </div>
        }
      >
        {tableMetadata !== undefined && (
          <SidebarDetailLayout
            panelSizeKey={`${deploymentId}/data`}
            mobileBarContent={<NentSwitcher className="w-40" />}
            sidebarComponent={
              <DataSidebar
                tableData={tableMetadata}
                onTableCreated={onTableCreated}
              />
            }
            resizeHandleTitle="Tables"
            contentComponent={
              tableMetadata.name === null ? (
                <EmptyData />
              ) : (
                <LoadingTransition
                  loadingState={<DataContentSkeleton />}
                  loadingProps={{ shimmer: false }}
                >
                  {activeSchema !== undefined && (
                    <ErrorBoundary
                      fallback={(props) => (
                        <HandleTimeout
                          {...props}
                          setPageSize={setPageSize}
                          currentPageSize={currentPageSize}
                        />
                      )}
                    >
                      <DataContent
                        key={tableMetadata.name}
                        tableName={tableMetadata.name}
                        componentId={componentId ?? null}
                        activeSchema={activeSchema}
                        onDocumentsAdded={onDocumentsAdded}
                        onFiltersApplied={onFiltersApplied}
                      />
                    </ErrorBoundary>
                  )}
                </LoadingTransition>
              )
            }
          />
        )}
      </LoadingTransition>
    </>
  );
}

const PAGE_TIMEOUT_MESSAGES = [
  "Function execution timed out",
  "Your request timed out performing too many system operations.",
];

function isPageTimeoutError(error: unknown) {
  return (
    error instanceof Error &&
    error.message.startsWith(
      "[CONVEX Q(_system/frontend/paginatedTableDocuments:default)]",
    ) &&
    PAGE_TIMEOUT_MESSAGES.some((message) => error.message.includes(message))
  );
}

function HandleTimeout({
  error,
  resetError,
  setPageSize,
  currentPageSize,
}: {
  error: unknown;
  resetError(): void;
  currentPageSize: number;
  setPageSize: (pageSize: number) => void;
}) {
  const canRetryWithSmallerPage =
    isPageTimeoutError(error) && currentPageSize > 1;

  useEffect(() => {
    if (canRetryWithSmallerPage) {
      setPageSize(Math.max(Math.floor(currentPageSize / 2), 1));
      resetError();
    }
  }, [canRetryWithSmallerPage, currentPageSize, setPageSize, resetError]);

  if (!canRetryWithSmallerPage) {
    throw error;
  }
  return null;
}
