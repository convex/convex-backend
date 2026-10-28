import { ConvexReactClient, Watch } from "convex/react";
import { getFunctionName, FunctionReference } from "convex/server";

// TODO(ari): Use convex-test
export function mockConvexReactClient(): MockConvexReactClient &
  ConvexReactClient {
  return new MockConvexReactClient() as any;
}

class MockConvexReactClient {
  // These are written to and read from type safe APIs (`registerQueryFake`, `watchQuery`),
  // so it's ok to be looser with the types here since they're never directly accessed.
  private queries: Record<string, (...args: any[]) => any>;

  private mutations: Record<string, (...args: any[]) => any>;

  constructor() {
    this.queries = {};
    this.mutations = {};
  }

  registerQueryFake<FuncRef extends FunctionReference<"query", "public">>(
    funcRef: FuncRef,
    impl: (args: FuncRef["_args"]) => FuncRef["_returnType"],
  ): this {
    this.queries[getFunctionName(funcRef)] = impl;
    return this;
  }

  registerMutationFake<FuncRef extends FunctionReference<"mutation", "public">>(
    funcRef: FuncRef,
    impl: (args: FuncRef["_args"]) => FuncRef["_returnType"],
  ): this {
    this.mutations[getFunctionName(funcRef)] = impl;
    return this;
  }

  setAuth() {
    throw new Error("Auth is not implemented");
  }

  clearAuth() {
    throw new Error("Auth is not implemented");
  }

  watchQuery<Query extends FunctionReference<"query">>(
    query: FunctionReference<"query">,
    ...args: Query["_args"]
  ): Watch<Query["_returnType"]> {
    return {
      localQueryResult: () => {
        const name = getFunctionName(query);
        const queryImpl = this.queries && this.queries[name];
        if (queryImpl) {
          return queryImpl(...args);
        }
        throw new Error(
          `Unexpected query: ${name}. Try providing a function for this query in the mock client constructor.`,
        );
      },
      onUpdate: () => () => ({
        unsubscribe: () => null,
      }),
      journal: () => void 0,
      localQueryLogs: () => {
        throw new Error("not implemented");
      },
    };
  }

  watchPaginatedQuery<Query extends FunctionReference<"query">>(
    query: Query,
    args: Query["_args"],
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    options: { initialNumItems: number; id: number },
  ) {
    return {
      onUpdate: () => () => ({
        unsubscribe: () => null,
      }),
      localQueryResult: () => {
        const name = getFunctionName(query);
        const queryImpl = this.queries && this.queries[name];
        if (queryImpl) {
          const paginationResult = queryImpl(args);
          // Transform PaginationResult to PaginatedQueryResult
          return {
            results: paginationResult.page,
            status: "Exhausted" as const,
            loadMore: () => false,
          };
        }
        throw new Error(
          `Unexpected query: ${name}. Try providing a function for this query in the mock client constructor.`,
        );
      },
    };
  }

  mutation<Mutation extends FunctionReference<"mutation">>(
    mutation: Mutation,
    ...args: Mutation["_args"]
  ): Promise<Mutation["_returnType"]> {
    const name = getFunctionName(mutation);
    const mutationImpl = this.mutations && this.mutations[name];
    if (mutationImpl) {
      return mutationImpl(args[0]);
    }
    throw new Error(
      `Unexpected mutation: ${name}. Try providing a function for this mutation in the mock client constructor.`,
    );
  }

  action(): Promise<any> {
    throw new Error("Actions are not implemented");
  }

  connectionState() {
    return {
      hasInflightRequests: false,
      isWebSocketConnected: true,
    };
  }

  close() {
    return Promise.resolve();
  }
}
