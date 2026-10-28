import {
  optimisticallyUpdateValueInPaginatedQuery,
  useMutation,
} from "convex/react";
import { GenericId, Value, ConvexError } from "convex/values";
import { useRouter } from "next/router";
import udfs from "@common/udfs";
import { useCallback } from "react";
import omitBy from "lodash/omitBy";
import { isFilterValidationError } from "system-udfs/convex/_system/frontend/lib/filters";
import { UNDEFINED_PLACEHOLDER } from "system-udfs/convex/_system/frontend/lib/values";
import { useNents } from "@common/lib/useNents";
import { toast } from "@common/lib/utils";
import { filterParamForQuery } from "@common/features/data/lib/useTableFilters";

export function usePatchDocumentField(tableName: string) {
  const router = useRouter();
  const { selectedNent } = useNents();

  const patchDocument = useMutation(
    udfs.patchDocumentsFields.default,
  ).withOptimisticUpdate((localStore, { ids, fields }) => {
    if (!ids || ids.length !== 1) {
      throw new Error("Attempted to patch more than one document at a time.");
    }
    if (Object.keys(fields).length !== 1) {
      throw new Error("Attempted to patch more than one field at a time.");
    }

    const documentId = ids[0];
    optimisticallyUpdateValueInPaginatedQuery(
      localStore,
      udfs.paginatedTableDocuments.default,
      {
        componentId: selectedNent?.id ?? null,
        table: tableName,
        // Must match the sanitized param `useQueryFilteredTable` subscribes
        // with, or the optimistic update misses the cached paginated query.
        filters: filterParamForQuery((router.query.filters as string) || null),
      },
      (currentValue) => {
        if (
          // This is disgusting, but because we don't have any other way to return errors
          // from a paginated query, this type check is necessary.
          !isFilterValidationError(currentValue) &&
          documentId === currentValue._id
        ) {
          return {
            ...omitBy(
              currentValue,
              (value, key) => fields[key] === UNDEFINED_PLACEHOLDER,
            ),
            ...omitBy(fields, (value) => value === UNDEFINED_PLACEHOLDER),
          };
        }
        return currentValue;
      },
    );
  });

  return useCallback(
    async (
      table: string,
      id: GenericId<string>,
      field: string,
      value: Value,
    ) => {
      try {
        await patchDocument({
          componentId: selectedNent?.id ?? null,
          table,
          ids: [id],
          fields: { [field]: value },
        });
      } catch (error: any) {
        if (error instanceof ConvexError) {
          toast("error", error.data, undefined, false);
        } else {
          throw error;
        }
      }
    },
    [patchDocument, selectedNent],
  );
}
