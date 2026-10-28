import { useQuery } from "convex/react";
import { GenericDocument } from "convex/server";
import { useMemo } from "react";
import { parseAndFilterToSingleTable } from "system-udfs/convex/_system/frontend/lib/filters";
import udfs from "@common/udfs";
import { useNents } from "@common/lib/useNents";
import { defaultValueForValidator } from "@common/lib/defaultValueForValidator";

export const useDefaultDocument = (tableName: string): GenericDocument => {
  const schemas = useQuery(udfs.getSchemas.default, {
    componentId: useNents().selectedNent?.id ?? null,
  });

  const activeSchema = schemas?.active;

  const tableDef = activeSchema
    ? parseAndFilterToSingleTable(tableName, activeSchema)?.tables[0]
    : undefined;
  const defaultValueForSchema =
    tableDef &&
    defaultValueForValidator(tableDef.documentType ?? { type: "any" });
  return useMemo(
    () => (defaultValueForSchema as GenericDocument) || {},
    [defaultValueForSchema],
  );
};
