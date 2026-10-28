import { jsonToConvex, JSONValue } from "convex/values";
import { stringifyValue } from "../lib/stringifyValue";

self.onmessage = async (message: MessageEvent<JSONValue>) => {
  self.postMessage(stringifyValue(jsonToConvex(message.data), true));
};
