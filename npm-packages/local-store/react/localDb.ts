import {
  DocumentByInfo,
  DocumentByName,
  FilterBuilder,
  GenericDataModel,
  GenericTableInfo,
  IndexNames,
  IndexRange,
  IndexRangeBuilder,
  NamedIndex,
  NamedTableInfo,
  TableNamesInDataModel,
  WithOptionalSystemFields,
} from "convex/server";
import { GenericId } from "convex/values";

export interface LocalDbReader<DataModel extends GenericDataModel> {
  get<T extends TableNamesInDataModel<DataModel>>(
    table: T,
    id: GenericId<T>,
  ): DocumentByName<DataModel, T> | null;
  query<T extends TableNamesInDataModel<DataModel>>(
    table: T,
  ): QueryInitializer<NamedTableInfo<DataModel, T>>;
}

interface QueryInitializer<
  TableInfo extends GenericTableInfo,
> extends Query<TableInfo> {
  withIndex<IndexName extends IndexNames<TableInfo>>(
    indexName: IndexName,
    builder?: (
      q: IndexRangeBuilder<
        DocumentByInfo<TableInfo>,
        NamedIndex<TableInfo, IndexName>
      >,
    ) => IndexRange,
  ): Query<TableInfo>;
}

interface Query<TableInfo extends GenericTableInfo> {
  collect(): DocumentByInfo<TableInfo>[];
  take(n: number): DocumentByInfo<TableInfo>[];
  unique(): DocumentByInfo<TableInfo> | null;
  first(): DocumentByInfo<TableInfo> | null;
  filter(
    filter: (q: FilterBuilder<TableInfo>) => FilterBuilder<TableInfo>,
  ): Query<TableInfo>;
  order(order: "asc" | "desc"): Query<TableInfo>;
}

export interface LocalDbWriter<
  DataModel extends GenericDataModel,
> extends LocalDbReader<DataModel> {
  insert<T extends TableNamesInDataModel<DataModel>>(
    tableName: T,
    id: string,
    document: WithOptionalSystemFields<DocumentByName<DataModel, T>>,
  ): GenericId<T>;
  delete<T extends TableNamesInDataModel<DataModel>>(
    tableName: T,
    id: string,
  ): void;
  replace<T extends TableNamesInDataModel<DataModel>>(
    tableName: T,
    id: string,
    document: WithOptionalSystemFields<DocumentByName<DataModel, T>>,
  ): void;
  patch<T extends TableNamesInDataModel<DataModel>>(
    tableName: T,
    id: string,
    document: Partial<DocumentByName<DataModel, T>>,
  ): void;
}
