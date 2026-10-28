import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { IntegrationsView } from "@common/features/settings/components/integrations/IntegrationsView";
import { usePostHog } from "hooks/usePostHog";

export { getServerSideProps } from "lib/ssr";

function IntegrationsWithAnalytics() {
  const { capture } = usePostHog();
  return (
    <IntegrationsView
      onAddedIntegration={(kind) => {
        capture("added_integration", { kind });
      }}
    />
  );
}

export default withAuthenticatedPage(IntegrationsWithAnalytics);
