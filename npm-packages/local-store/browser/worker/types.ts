import { convexToJson, jsonToConvex } from "convex/values";
import { z } from "zod";
import {
  ConvexSubscriptionId,
  LowerBound,
  UpperBound,
  MutationInfo,
  MutationId,
} from "../../shared/types";
import { Page } from "../core/protocol";
import { getFunctionName, makeFunctionReference } from "convex/server";

// Each instance of the `SyncWorkerClient` class receives a unique instance ID.
export type ClientId = string;

const storedIndexPrefix = z.array(z.any());
const storedMinimalKey = z.object({
  kind: z.literal("predecessor"),
  value: z.array(z.never()),
});
const storedLowerBound = z.union([
  z.object({ kind: z.literal("successor"), value: storedIndexPrefix }),
  storedMinimalKey,
]);
const storedIndexKey = z.array(z.any());
const storedExactKey = z.object({
  kind: z.literal("exact"),
  value: storedIndexKey,
});
const storedMaximalKey = z.object({
  kind: z.literal("successor"),
  value: z.array(z.never()),
});
const storedUpperBound = z.union([storedExactKey, storedMaximalKey]);

export const storedPageValidator = z.object({
  table: z.string(),
  index: z.string(),
  convexSubscriptionId: z.string(),
  serializedLowerBound: z.string(),
  lowerBound: storedLowerBound,
  upperBound: storedUpperBound,
  documents: z.array(z.any()),
});
export type StoredPage = z.infer<typeof storedPageValidator>;

export function pageToStoredPage(page: Page): StoredPage | null {
  if (page.state.kind === "loading") {
    return null;
  }
  return storedPageValidator.parse({
    table: page.tableName,
    index: page.indexName,
    convexSubscriptionId: page.convexSubscriptionId,
    serializedLowerBound: JSON.stringify(page.state.value.lowerBound),
    lowerBound: page.state.value.lowerBound,
    upperBound: page.state.value.upperBound,
    documents: page.state.value.results,
  });
}

export function storedPageToPage(storedPage: StoredPage): Page {
  return {
    tableName: storedPage.table,
    indexName: storedPage.index,
    convexSubscriptionId:
      storedPage.convexSubscriptionId as ConvexSubscriptionId,
    state: {
      kind: "loaded",
      value: {
        lowerBound: storedPage.lowerBound as LowerBound,
        upperBound: storedPage.upperBound as UpperBound,
        results: storedPage.documents,
      },
    },
  };
}

export const storedMutationValidator = z.object({
  mutationName: z.string(),
  mutationId: z.string(),
  mutationPath: z.string(),
  optUpdateArgs: z.any().optional(),
  serverArgs: z.any().optional(),
});
export type StoredMutation = z.infer<typeof storedMutationValidator>;

export function mutationToStoredMutation(
  mutation: MutationInfo,
): StoredMutation {
  return storedMutationValidator.parse({
    mutationName: mutation.mutationName,
    mutationId: mutation.mutationId,
    mutationPath: getFunctionName(mutation.mutationPath),
    optUpdateArgs: convexToJson(mutation.optUpdateArgs as any),
    serverArgs: convexToJson(mutation.serverArgs as any),
  });
}

export function storedMutationToMutation(
  storedMutation: StoredMutation,
): MutationInfo {
  return {
    mutationName: storedMutation.mutationName,
    mutationId: storedMutation.mutationId as MutationId,
    mutationPath: makeFunctionReference(storedMutation.mutationPath),
    optUpdateArgs: jsonToConvex(storedMutation.optUpdateArgs) as any,
    serverArgs: jsonToConvex(storedMutation.serverArgs) as any,
  };
}

export const followerMessage = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("join"),
    clientId: z.string(),
    name: z.string(),
    address: z.string(),
  }),
  z.object({
    type: z.literal("persist"),
    clientId: z.string(),
    persistId: z.string(),
    mutationInfos: z.array(storedMutationValidator),
    pages: z.array(storedPageValidator),
  }),
]);
export type FollowerMessage = z.infer<typeof followerMessage>;

export const leaderMessage = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("joinResult"),
    requestingClientId: z.string(),
    leaderClientId: z.string(),
    result: z.discriminatedUnion("type", [
      z.object({
        type: z.literal("success"),
        pages: z.array(storedPageValidator),
        mutations: z.array(z.any()),
      }),
      z.object({
        type: z.literal("failure"),
        error: z.string(),
      }),
    ]),
  }),
  z.object({
    type: z.literal("persistResult"),
    requestingClientId: z.string(),
    leaderClientId: z.string(),
    persistId: z.string(),
    result: z.discriminatedUnion("type", [
      z.object({
        type: z.literal("success"),
      }),
      z.object({
        type: z.literal("failure"),
        error: z.string(),
      }),
    ]),
  }),
]);
export type LeaderMessage = z.infer<typeof leaderMessage>;
