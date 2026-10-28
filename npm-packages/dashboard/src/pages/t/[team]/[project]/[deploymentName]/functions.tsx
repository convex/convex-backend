import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { FunctionsView } from "@common/features/functions/components/FunctionsView";

function FunctionsPage() {
  return <FunctionsView />;
}

export { getServerSideProps } from "lib/ssr";

export default withAuthenticatedPage(FunctionsPage);
