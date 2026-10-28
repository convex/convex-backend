import { useMemo } from "react";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { DataView } from "@common/features/data/components/DataView";
import { dedupeByShape } from "@common/features/data/lib/filterAnalytics";
import { usePostHog } from "hooks/usePostHog";

export { getServerSideProps } from "lib/ssr";

function DataViewWithAnalytics() {
  const { capture } = usePostHog();
  const onFiltersApplied = useMemo(
    () =>
      dedupeByShape((properties) =>
        capture("data_filters_applied", properties),
      ),
    [capture],
  );
  return (
    <DataView
      onTableCreated={() => capture("created_table")}
      onDocumentsAdded={(count) => capture("add_documents", { count })}
      onFiltersApplied={onFiltersApplied}
    />
  );
}

export default withAuthenticatedPage(DataViewWithAnalytics);
