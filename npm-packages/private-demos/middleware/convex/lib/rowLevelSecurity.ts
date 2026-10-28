import {
  GenericDatabaseReader,
  GenericDatabaseWriter,
  DocumentByInfo,
  DocumentByName,
  Expression,
  FilterBuilder,
  GenericDataModel,
  GenericTableInfo,
  IndexRange,
  IndexRangeBuilder,
  Indexes,
  GenericMutationCtx,
  NamedIndex,
  NamedSearchIndex,
  NamedTableInfo,
  OrderedQuery,
  PaginationOptions,
  PaginationResult,
  Query,
  GenericQueryCtx,
  QueryInitializer,
  SearchFilter,
  SearchFilterBuilder,
  SearchIndexes,
  TableNamesInDataModel,
  WithoutSystemFields,
  DefaultFunctionArgs,
} from "convex/server";
import { GenericId } from "convex/values";

type Rule<Ctx, D> = (ctx: Ctx, doc: D) => Promise<boolean>;

export type Rules<Ctx, DataModel extends GenericDataModel> = {
  [T in TableNamesInDataModel<DataModel>]?: {
    read?: Rule<Ctx, DocumentByName<DataModel, T>>;
    modify?: Rule<Ctx, DocumentByName<DataModel, T>>;
    insert?: Rule<Ctx, WithoutSystemFields<DocumentByName<DataModel, T>>>;
  };
};

/**
 * Apply row level security (RLS) to queries and mutations with the returned
 * middleware functions.
 *
 * Example:
 * ```
 * // Defined in a common file so it can be used by all queries and mutations.
 * import { Auth } from "convex/server";
 * import { DataModel } from "./_generated/dataModel";
 * import { DatabaseReader } from "./_generated/server";
 * import { RowLevelSecurity } from "./rowLevelSecurity";
 *
 * export const {withMutationRLS} = RowLevelSecurity<{auth: Auth, db: DatabaseReader}, DataModel>(
 *  {
 *    cookies: {
 *      read: async ({auth}, cookie) => !cookie.eaten,
 *      modify: async ({auth, db}, cookie) => {
 *        const user = await getUser(auth, db);
 *        return user.isParent;  // only parents can reach the cookies.
 *      },
 *  }
 * );
 * // Mutation with row level security enabled.
 * export const eatCookie = mutation(withMutationRLS(
 *  async ({db}, {cookieId}) => {
 *   // throws "does not exist" error if cookie is already eaten or doesn't exist.
 *   // throws "write access" error if authorized user is not a parent.
 *   await db.patch(cookieId, {eaten: true});
 * }));
 * ```
 *
 * Notes:
 * * Rules may read any row in `db` -- rules do not apply recursively within the
 *   rule functions themselves.
 * * Tables with no rule default to full access.
 * * Middleware functions like `withUser` can be composed with RowLevelSecurity
 *   to cache fetches in `ctx`. e.g.
 * ```
 * const {withQueryRLS} = RowLevelSecurity<{user: Doc<"users">}, DataModel>(
 *  {
 *    cookies: async ({user}, cookie) => user.isParent,
 *  }
 * );
 * export default query(withUser(withRLS(...)));
 * ```
 *
 * @param rules - rule for each table, determining whether a row is accessible.
 *  - "read" rule says whether a document should be visible.
 *  - "modify" rule says whether to throw an error on `replace`, `patch`, and `delete`.
 *  - "insert" rule says whether to throw an error on `insert`.
 *
 * @returns Functions `withQueryRLS` and `withMutationRLS` to be passed to
 * `query` or `mutation` respectively.
 *  For each row read, modified, or inserted, the security rules are applied.
 */
export const RowLevelSecurity = <RuleCtx, DataModel extends GenericDataModel>(
  rules: Rules<RuleCtx, DataModel>,
) => {
  const withMutationRLS = <
    Ctx extends GenericMutationCtx<DataModel>,
    Args extends ArgsArray,
    Output,
  >(
    f: Handler<Ctx, Args, Output>,
  ): Handler<Ctx, Args, Output> => {
    return ((ctx: any, ...args: any[]) => {
      const wrappedDb = new WrapWriter(ctx, ctx.db, rules);
      return (f as any)({ ...ctx, db: wrappedDb }, ...args);
    }) as Handler<Ctx, Args, Output>;
  };
  const withQueryRLS = <
    Ctx extends GenericQueryCtx<DataModel>,
    Args extends ArgsArray,
    Output,
  >(
    f: Handler<Ctx, Args, Output>,
  ): Handler<Ctx, Args, Output> => {
    return ((ctx: any, ...args: any[]) => {
      const wrappedDb = new WrapReader(ctx, ctx.db, rules);
      return (f as any)({ ...ctx, db: wrappedDb }, ...args);
    }) as Handler<Ctx, Args, Output>;
  };
  return {
    withMutationRLS,
    withQueryRLS,
  };
};

type ArgsArray = [] | [DefaultFunctionArgs];
type Handler<Ctx, Args extends ArgsArray, Output> = (
  ctx: Ctx,
  ...args: Args
) => Output;

type AuthPredicate<T extends GenericTableInfo> = (
  doc: DocumentByInfo<T>,
) => Promise<boolean>;

async function asyncFilter<T>(
  arr: T[],
  predicate: (d: T) => Promise<boolean>,
): Promise<T[]> {
  const results = await Promise.all(arr.map(predicate));
  return arr.filter((_v, index) => results[index]);
}

class WrapQuery<T extends GenericTableInfo> implements Query<T> {
  q: Query<T>;
  p: AuthPredicate<T>;
  iterator?: AsyncIterator<any>;
  constructor(q: Query<T> | OrderedQuery<T>, p: AuthPredicate<T>) {
    this.q = q as Query<T>;
    this.p = p;
  }
  // internal
  limit(n: number): this {
    return new WrapQuery(this.q.limit(n), this.p) as this;
  }
  filter(predicate: (q: FilterBuilder<T>) => Expression<boolean>): this {
    return new WrapQuery(this.q.filter(predicate), this.p) as this;
  }
  order(order: "asc" | "desc"): WrapQuery<T> {
    return new WrapQuery(this.q.order(order), this.p);
  }
  async paginate(
    paginationOpts: PaginationOptions,
  ): Promise<PaginationResult<DocumentByInfo<T>>> {
    const result = await this.q.paginate(paginationOpts);
    result.page = await asyncFilter(result.page, this.p);
    return result;
  }
  async collect(): Promise<DocumentByInfo<T>[]> {
    const results = await this.q.collect();
    return await asyncFilter(results, this.p);
  }
  async take(n: number): Promise<DocumentByInfo<T>[]> {
    const results = [];
    for await (const result of this) {
      results.push(result);
      if (results.length >= n) {
        break;
      }
    }
    return results;
  }
  async first(): Promise<DocumentByInfo<T> | null> {
    for await (const result of this) {
      return result;
    }
    return null;
  }
  async unique(): Promise<DocumentByInfo<T> | null> {
    let uniqueResult = null;
    for await (const result of this) {
      if (uniqueResult === null) {
        uniqueResult = result;
      } else {
        throw new Error("not unique");
      }
    }
    return uniqueResult;
  }
  [Symbol.asyncIterator](): AsyncIterator<DocumentByInfo<T>, any, undefined> {
    this.iterator = this.q[Symbol.asyncIterator]();
    return this;
  }
  async next(): Promise<IteratorResult<any>> {
    for (;;) {
      const { value, done } = await this.iterator!.next();
      if (value && (await this.p(value))) {
        return { value, done };
      }
      if (done) {
        return { value: null, done: true };
      }
    }
  }
  return() {
    return this.iterator!.return!();
  }
}

class WrapQueryInitializer<
  T extends GenericTableInfo,
> implements QueryInitializer<T> {
  q: QueryInitializer<T>;
  p: AuthPredicate<T>;
  constructor(q: QueryInitializer<T>, p: AuthPredicate<T>) {
    this.q = q;
    this.p = p;
  }
  // internal
  count(): Promise<number> {
    return this.q.count();
  }
  // internal
  limit(n: number): this {
    return this.fullTableScan().limit(n) as this;
  }
  fullTableScan(): Query<T> {
    return new WrapQuery(this.q.fullTableScan(), this.p);
  }
  withIndex<IndexName extends keyof Indexes<T>>(
    indexName: IndexName,
    indexRange?:
      | ((
          q: IndexRangeBuilder<DocumentByInfo<T>, NamedIndex<T, IndexName>, 0>,
        ) => IndexRange)
      | undefined,
  ): Query<T> {
    return new WrapQuery(this.q.withIndex(indexName, indexRange), this.p);
  }
  withSearchIndex<IndexName extends keyof SearchIndexes<T>>(
    indexName: IndexName,
    searchFilter: (
      q: SearchFilterBuilder<DocumentByInfo<T>, NamedSearchIndex<T, IndexName>>,
    ) => SearchFilter,
  ): OrderedQuery<T> {
    return new WrapQuery(
      this.q.withSearchIndex(indexName, searchFilter),
      this.p,
    );
  }
  filter(predicate: (q: FilterBuilder<T>) => Expression<boolean>): this {
    return this.fullTableScan().filter(predicate) as this;
  }
  order(order: "asc" | "desc"): OrderedQuery<T> {
    return this.fullTableScan().order(order);
  }
  async paginate(
    paginationOpts: PaginationOptions,
  ): Promise<PaginationResult<DocumentByInfo<T>>> {
    return this.fullTableScan().paginate(paginationOpts);
  }
  collect(): Promise<DocumentByInfo<T>[]> {
    return this.fullTableScan().collect();
  }
  take(n: number): Promise<DocumentByInfo<T>[]> {
    return this.fullTableScan().take(n);
  }
  first(): Promise<DocumentByInfo<T> | null> {
    return this.fullTableScan().first();
  }
  unique(): Promise<DocumentByInfo<T> | null> {
    return this.fullTableScan().unique();
  }
  [Symbol.asyncIterator](): AsyncIterator<DocumentByInfo<T>, any, undefined> {
    return this.fullTableScan()[Symbol.asyncIterator]();
  }
}

class WrapReader<Ctx, DataModel extends GenericDataModel> implements Omit<
  GenericDatabaseReader<DataModel>,
  "system"
> {
  ctx: Ctx;
  db: GenericDatabaseReader<DataModel>;
  rules: Rules<Ctx, DataModel>;

  constructor(
    ctx: Ctx,
    db: GenericDatabaseReader<DataModel>,
    rules: Rules<Ctx, DataModel>,
  ) {
    this.ctx = ctx;
    this.db = db;
    this.rules = rules;
  }

  async predicate<T extends GenericTableInfo>(
    tableName: string,
    doc: DocumentByInfo<T>,
  ): Promise<boolean> {
    if (!this.rules[tableName]?.read) {
      return true;
    }
    return await this.rules[tableName]!.read!(this.ctx, doc);
  }

  async get<TableName extends string>(id: GenericId<TableName>): Promise<any> {
    // eslint-disable-next-line @convex-dev/explicit-table-ids -- old internal demo project
    const doc = await this.db.get(id);
    if (doc) {
      const tableName = this._tableName(id);
      if (tableName !== null && !(await this.predicate(tableName, doc))) {
        return null;
      }
      return doc;
    }
    return null;
  }

  query<TableName extends string>(
    tableName: TableName,
  ): QueryInitializer<NamedTableInfo<DataModel, TableName>> {
    return new WrapQueryInitializer(
      this.db.query(tableName),
      async (d) => await this.predicate(tableName, d),
    );
  }

  normalizeId<TableName extends TableNamesInDataModel<DataModel>>(
    tableName: TableName,
    id: string,
  ): GenericId<TableName> | null {
    return this.db.normalizeId(tableName, id);
  }

  private _tableName<TableName extends string>(
    id: GenericId<TableName>,
  ): TableName | null {
    for (const tableName of Object.keys(this.rules)) {
      if (this.db.normalizeId(tableName, id) !== null) {
        return tableName as TableName;
      }
    }
    return null;
  }
}

class WrapWriter<Ctx, DataModel extends GenericDataModel> implements Omit<
  GenericDatabaseWriter<DataModel>,
  "system"
> {
  ctx: Ctx;
  db: GenericDatabaseWriter<DataModel>;
  reader: Omit<GenericDatabaseReader<DataModel>, "system">;
  rules: Rules<Ctx, DataModel>;
  // Plain values with no access-control surface; pass them through.
  vars: GenericDatabaseWriter<DataModel>["vars"];

  async modifyPredicate<T extends GenericTableInfo>(
    tableName: string,
    doc: DocumentByInfo<T>,
  ): Promise<boolean> {
    if (!this.rules[tableName]?.modify) {
      return true;
    }
    return await this.rules[tableName]!.modify!(this.ctx, doc);
  }

  constructor(
    ctx: Ctx,
    db: GenericDatabaseWriter<DataModel>,
    rules: Rules<Ctx, DataModel>,
  ) {
    this.ctx = ctx;
    this.db = db;
    this.reader = new WrapReader(ctx, db, rules);
    this.rules = rules;
    this.vars = db.vars;
  }
  async insert<TableName extends string>(
    table: TableName,
    value: any,
  ): Promise<any> {
    if (
      this.rules[table]?.insert &&
      !(await this.rules[table]!.insert!(this.ctx, value))
    ) {
      throw new Error("insert access not allowed");
    }
    return await this.db.insert(table, value);
  }
  async checkAuth<TableName extends string>(id: GenericId<TableName>) {
    // Note all writes already do a `db.get` internally, so this isn't
    // an extra read; it's just populating the cache earlier.
    // Since we call `this.get`, read access controls apply and this may return
    // null even if the document exists.
    const doc = await this.get(id);
    if (doc === null) {
      throw new Error("no read access or doc does not exist");
    }
    const tableName = this._tableName(id);
    if (tableName === null) {
      return;
    }
    if (!(await this.modifyPredicate(tableName, doc))) {
      throw new Error("write access not allowed");
    }
  }
  async patch<TableName extends string>(
    id: GenericId<TableName>,
    value: Partial<any>,
  ): Promise<void> {
    await this.checkAuth(id);
    // eslint-disable-next-line @convex-dev/explicit-table-ids -- old internal demo project
    return await this.db.patch(id, value);
  }
  async replace<TableName extends string>(
    id: GenericId<TableName>,
    value: any,
  ): Promise<void> {
    await this.checkAuth(id);
    // eslint-disable-next-line @convex-dev/explicit-table-ids -- old internal demo project
    return await this.db.replace(id, value);
  }
  async delete(id: GenericId<string>): Promise<void> {
    await this.checkAuth(id);
    // eslint-disable-next-line @convex-dev/explicit-table-ids -- old internal demo project
    return await this.db.delete(id);
  }
  get<TableName extends string>(id: GenericId<TableName>): Promise<any> {
    return this.reader.get(id);
  }
  query<TableName extends string>(tableName: TableName): QueryInitializer<any> {
    return this.reader.query(tableName);
  }

  normalizeId<TableName extends TableNamesInDataModel<DataModel>>(
    tableName: TableName,
    id: string,
  ): GenericId<TableName> | null {
    return this.reader.normalizeId(tableName, id);
  }

  private _tableName<TableName extends string>(
    id: GenericId<TableName>,
  ): TableName | null {
    for (const tableName of Object.keys(this.rules)) {
      if (this.db.normalizeId(tableName, id) !== null) {
        return tableName as TableName;
      }
    }
    return null;
  }
}
