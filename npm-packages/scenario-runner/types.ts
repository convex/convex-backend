import { RoutableMethod } from "convex/server";
import {
  ScenarioCountMetric,
  ScenarioError,
  ScenarioLatencyMetric,
  ScenarioName,
} from "./metrics";

export const ACTION_TIMEOUT = 5000;
export const HTTP_ACTION_TIMEOUT = 5000;
// If the query doesn't return after 2s, give up on it because udf timeout is 1s,
// and there's some network and backend overhead.
export const QUERY_TIMEOUT = 2000;
// If a mutation does not resolve after 2s, give up on it because udf timeout is 1s,
// and the commit follows the UDF.
export const MUTATION_TIMEOUT = 2000;
// Export should take less than 2 hours with our 500MB db limit
export const EXPORT_TIMEOUT = 2 * 60 * 60 * 1000;
// `ConvexClient.close()` resolves only when the WebSocket `onclose` event
// fires, which can hang indefinitely on a half-open / wedged connection. Bound
// it so a stuck close can't block the scenario loop forever.
export const CLOSE_TIMEOUT = 10000;

export const UNDEFINED_UPDATE_MSG =
  "Client unexpectedly called onUpdate when query result was undefined.";

export const UNDEFINED_UPDATE = "undefined_update";

export const MESSAGES_TABLE = "messages";

export const OPENCLAURD_TABLE = "openclaurd";

// Matches openai's text embeddings.
export const EMBEDDING_SIZE = 1536;
export type Metric =
  | {
      type: "Latency";
      value: number;
      name: ScenarioLatencyMetric;
      scenario: ScenarioName;
      path?: string;
    }
  | {
      type: "Count";
      value: number;
      name: ScenarioCountMetric;
      scenario: ScenarioName;
      path?: string;
    };
export type Error = {
  msg: string;
  name: ScenarioError;
  scenario: ScenarioName;
};

export type Event = Metric | Error;

export type FnType = "query" | "mutation" | "action";

export type ScenarioMessage = {
  scenario: ScenarioSpec;
  rate: number | null;
  threads?: number;
};
export type ScenarioSpec =
  | {
      name: "RunFunction";
      path: string;
      fn_type: FnType;
    }
  | {
      name: "RunHttpAction";
      path: string;
      method: RoutableMethod;
    }
  | {
      name: "ObserveInsert";
      search_indexes: boolean;
    }
  | {
      name: "ManyIntersections";
      num_subscriptions: number;
    }
  | {
      name: "HoldSubscriptions";
      num_subscriptions: number;
      hold_duration_secs: number;
      invalidation_interval_secs?: number;
      num_invalidations?: number;
    }
  | {
      name: "Search";
    }
  | {
      name: "VectorSearch";
    }
  | {
      name: "SnapshotExport";
    }
  | {
      name: "CloudBackup";
    };
