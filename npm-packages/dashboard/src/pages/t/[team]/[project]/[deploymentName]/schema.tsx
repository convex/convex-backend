import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { SchemaView } from "@common/features/schema/components/SchemaView";

export { getServerSideProps } from "lib/ssr";

export default withAuthenticatedPage(SchemaView);
