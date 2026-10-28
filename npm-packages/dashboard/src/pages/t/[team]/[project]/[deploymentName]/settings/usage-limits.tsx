import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { UsageLimitsView } from "@common/features/settings/components/UsageLimitsView";

export { getServerSideProps } from "lib/ssr";

export default withAuthenticatedPage(UsageLimitsView);
