import { Loading } from "@ui/Loading";
import {
  TableSchemaContainer,
  useSingleTableSchemaStatus,
} from "@common/features/data/components/TableSchema";
import { DataPanel } from "@common/features/data/components/DataPanel";
import { Link } from "@ui/Link";

export function TableSchemaPanel({
  tableName,
  highlightField,
  onClose,
}: {
  tableName: string;
  // When set, the panel scrolls to and highlights this field within the
  // table's schema instead of highlighting the whole table.
  highlightField?: string;
  onClose: () => void;
}) {
  return (
    <DataPanel
      title={
        <>
          Schema for table{" "}
          <span className="font-mono text-[1.0625rem]">{tableName}</span>
        </>
      }
      onClose={onClose}
    >
      <SchemaBody tableName={tableName} highlightField={highlightField} />
    </DataPanel>
  );
}

function SchemaBody({
  tableName,
  highlightField,
}: {
  tableName: string;
  highlightField?: string;
}) {
  const tableSchemaStatus = useSingleTableSchemaStatus(tableName);
  if (tableSchemaStatus === undefined) {
    return <Loading />;
  }
  return (
    <>
      <LearnMoreLink
        name="schemas"
        link="https://docs.convex.dev/database/schemas"
      />
      <div className="px-1 sm:px-3">
        <TableSchemaContainer
          tableName={tableName}
          highlightField={highlightField}
        />
      </div>
    </>
  );
}

function LearnMoreLink({ name, link }: { name: string; link: string }) {
  return (
    <div className="mb-2 px-4 text-xs text-content-primary sm:px-6">
      Learn more about{" "}
      <Link passHref href={link} target="_blank" externalIcon>
        {name}
      </Link>
    </div>
  );
}
