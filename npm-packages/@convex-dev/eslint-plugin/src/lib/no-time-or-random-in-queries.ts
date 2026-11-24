import type { TSESTree } from "@typescript-eslint/types";
import { createRule, getRegisteredFunction } from "../util.js";
import {
  analyzeNondeterminism,
  isRandomChain,
} from "./nondeterminism-analysis.js";
import { CodePathTracker } from "./interprocedural-analysis.js";

type MessageIds = "time-in-query" | "random-in-query";
const QUERY_REGISTRARS = ["query", "internalQuery"];

export const noTimeOrRandomInQueries = createRule<[], MessageIds>({
  name: "no-time-or-random-in-queries",
  meta: {
    type: "problem",
    docs: {
      description:
        "Disallow reading the current time or generating random values in Convex queries.",
    },
    messages: {
      "time-in-query":
        "This query is reading the time ({{chain}}). Since Convex queries are automatically cached, the result can be outdated. Also, reading the time can cause the Convex query cache to be invalidated more frequently than necessary. Learn more at https://docs.convex.dev/understanding/best-practices/#date-in-queries",
      "random-in-query":
        "This query is generating random values ({{chain}}). Since Convex queries are automatically cached, the returned value is going to be the same as long as the query cache isn’t invalidated. This is probably not what you want: consider using a mutation instead.",
    },
    schema: [],
  },
  defaultOptions: [],
  create: (context) => {
    if (context.filename.includes("_generated")) {
      return {};
    }
    const codePaths = new CodePathTracker();
    // Inline handlers, or names of handlers defined elsewhere.
    const handlers = new Set<TSESTree.Node>();
    return {
      CallExpression(node) {
        const registered = getRegisteredFunction(node, QUERY_REGISTRARS);
        if (registered?.handler) {
          handlers.add(registered.handler);
          return;
        }
        if (!registered) return;
        // `query(listMessages)`
        if (!registered.objectArg) {
          handlers.add(registered.call.arguments[0]);
          return;
        }
        // `query({ handler: listMessages })`
        const property = registered.objectArg.properties.find(
          (prop): prop is TSESTree.Property =>
            prop.type === "Property" &&
            !prop.computed &&
            (prop.key.type === "Identifier"
              ? prop.key.name === "handler"
              : prop.key.type === "Literal" && prop.key.value === "handler"),
        );
        if (property) handlers.add(property.value);
      },
      "Program:exit"() {
        if (handlers.size === 0) return;
        const analysis = analyzeNondeterminism({
          filename: context.filename,
          sourceCode: context.sourceCode,
          codePaths,
          roots: [...handlers],
        });
        // A handler shared by several registrations is reported once.
        const reported = new Set<TSESTree.Node>();
        for (const handler of handlers) {
          for (const finding of analysis.findingsFor(handler)) {
            const node = finding.node ?? handler;
            if (!finding.chain || reported.has(node)) continue;
            reported.add(node);
            context.report({
              node,
              messageId: isRandomChain(finding.chain)
                ? "random-in-query"
                : "time-in-query",
              data: { chain: finding.chain.join(" → ") },
            });
          }
        }
      },
    };
  },
});
