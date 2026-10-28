import { useState } from "react";
import {
  ConvexSubscriptionId,
  IndexRangeBounds,
  PageArguments,
  PageResult,
} from "../shared/types";
import { compareKeys, maximalKey, minimalKey } from "../shared/compare";
import { cursorForSyncObject } from "../server/resolvers";
import { Key } from "../shared/types";
import { Divider } from "./Divider";
import { ChevronRightIcon } from "@radix-ui/react-icons";
import { ChevronDownIcon } from "@radix-ui/react-icons";
import { SchemaDefinition } from "convex/server";
import { compareValues } from "convex/values";

export function PageTimelineOld({ pages }: { pages: any[] }) {
  const [selectedPage, setSelectedPage] = useState<number | null>(null);
  const [showDocuments, setShowDocuments] = useState(false);
  const [expandedDocs, setExpandedDocs] = useState<Set<string>>(new Set());

  const toggleDocument = (id: string) => {
    const newExpanded = new Set(expandedDocs);
    if (newExpanded.has(id)) {
      newExpanded.delete(id);
    } else {
      newExpanded.add(id);
    }
    setExpandedDocs(newExpanded);
  };

  return (
    <div className="mt-4 space-y-4">
      <div className="relative h-16 flex items-center justify-center">
        <div className="flex items-center gap-2 relative">
          <div className="absolute left-4 right-4 top-1/2 h-px bg-gray-300 -z-10" />

          <div className="text-gray-500">-∞</div>

          {pages.map((page, i) => {
            const isLoading = page.state.kind === "loading";
            const numDocs = isLoading ? 1 : page.state.result.results.length;

            return (
              <div
                key={i}
                className={`h-10 cursor-pointer
                    ${
                      isLoading
                        ? "border-2 border-dashed border-gray-300"
                        : "border border-gray-400"
                    }
                    ${selectedPage === i ? "border-blue-500 border-2" : ""}
                    rounded-lg flex items-center justify-center bg-white
                    hover:border-blue-400 transition-colors`}
                style={{
                  width: `${Math.max(32, Math.min(numDocs * 8, 96))}px`,
                }}
                title={`${isLoading ? "Loading..." : `${numDocs} documents`}`}
                onClick={() => setSelectedPage(selectedPage === i ? null : i)}
              >
                {isLoading ? (
                  <div className="w-1.5 h-1.5 rounded-full bg-gray-400" />
                ) : (
                  <div className="flex gap-0.5 p-1">
                    {Array(page.state.result.results.length)
                      .fill(0)
                      .map((_, i) => (
                        <div
                          key={i}
                          className="w-1 h-1 rounded-full bg-blue-500"
                        />
                      ))}
                  </div>
                )}
              </div>
            );
          })}

          <div className="text-gray-500">+∞</div>
        </div>
      </div>

      {/* Page Details */}
      {selectedPage !== null && (
        <div className="bg-gray-50 rounded-lg p-4 text-sm space-y-2">
          {pages[selectedPage].state.kind === "loading" ? (
            <div className="text-gray-500">Loading page...</div>
          ) : (
            <>
              <div className="font-medium">Page Details:</div>
              <div className="space-y-1">
                <div>
                  <span className="text-gray-500">Lower Bound:</span>{" "}
                  {pages[selectedPage].state.result.lowerBound.kind ===
                  "predecessor"
                    ? "-∞"
                    : JSON.stringify(
                        pages[selectedPage].state.result.lowerBound.value,
                      )}
                </div>
                <div>
                  <span className="text-gray-500">Upper Bound:</span>{" "}
                  {pages[selectedPage].state.result.upperBound.kind ===
                  "successor"
                    ? "+∞"
                    : JSON.stringify(
                        pages[selectedPage].state.result.upperBound.value,
                      )}
                </div>

                {/* Combined Documents section */}
                <div>
                  <button
                    onClick={() => setShowDocuments(!showDocuments)}
                    className="flex items-center gap-1 text-gray-700 hover:text-gray-900"
                  >
                    <span
                      className="transform transition-transform duration-200"
                      style={{
                        display: "inline-block",
                        transform: `rotate(${
                          showDocuments ? "90deg" : "0deg"
                        })`,
                      }}
                    >
                      ▶
                    </span>
                    <span className="text-gray-500">Documents:</span>{" "}
                    {pages[selectedPage].state.result.results.length}
                  </button>

                  {showDocuments && (
                    <div className="mt-2 space-y-1 pl-4">
                      {pages[selectedPage].state.result.results.map(
                        (doc: any) => (
                          <div key={doc._id} className="space-y-1">
                            <button
                              onClick={() => toggleDocument(doc._id)}
                              className="flex items-center gap-1 text-gray-600 hover:text-gray-900"
                            >
                              <span
                                className="transform transition-transform duration-200"
                                style={{
                                  display: "inline-block",
                                  transform: `rotate(${
                                    expandedDocs.has(doc._id) ? "90deg" : "0deg"
                                  })`,
                                }}
                              >
                                ▶
                              </span>
                              {doc._id}
                            </button>

                            {expandedDocs.has(doc._id) && (
                              <pre className="pl-4 text-xs bg-white p-2 rounded border border-gray-200 overflow-auto">
                                {JSON.stringify(doc, null, 2)}
                              </pre>
                            )}
                          </div>
                        ),
                      )}
                    </div>
                  )}
                </div>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

type BaseDocEvent = {
  kind: "doc";
  key: Key;
  value: any;
};

type DocEvent = BaseDocEvent & {
  exactEvents?: Array<"target" | "upperBound" | "lowerBound" | "inRange">;
  predecessorEvents?: Array<"target" | "upperBound" | "lowerBound">;
  successorEvents?: Array<"target" | "upperBound" | "lowerBound">;
};

type KeyEventKind =
  | "target"
  | "upperBound"
  | "lowerBound"
  | "rangeLowerBound"
  | "rangeUpperBound";

type KeyEvent = {
  kind: "key";
  key: Key;
  events: Array<KeyEventKind>;
};

type Event = DocEvent | KeyEvent;

function addEventToDoc(
  doc: DocEvent,
  category: "exact" | "predecessor" | "successor",
  kind: "target" | "upperBound" | "lowerBound" | "inRange",
) {
  if (category === "exact") {
    doc.exactEvents = doc.exactEvents || [];
    doc.exactEvents.push(kind);
  } else if (category === "predecessor") {
    doc.predecessorEvents = doc.predecessorEvents || [];
    doc.predecessorEvents.push(kind as any);
  } else {
    doc.successorEvents = doc.successorEvents || [];
    doc.successorEvents.push(kind as any);
  }
}

function addKeyEvent(events: Array<KeyEvent>, kind: KeyEventKind, key: Key) {
  const event = events.find((e) => compareKeys(e.key, key) === 0);
  if (event === undefined) {
    events.push({ kind: "key", key, events: [kind] });
  } else {
    event.events.push(kind);
  }
}

export function PageTimeline({
  orderedPages,
  rangeBounds,
  syncSchema,
}: {
  orderedPages: Array<{
    args: PageArguments;
    state: { kind: "loading" } | { kind: "loaded"; result: PageResult };
    pageSubscriptionId: ConvexSubscriptionId;
  }>;
  rangeBounds?: IndexRangeBounds;
  syncSchema: SchemaDefinition<any, any>;
}) {
  const [selectedPage, setSelectedPage] = useState<number | null>(null);
  const rangeLowerBound =
    rangeBounds !== undefined ? minimalKey(rangeBounds) : undefined;
  const rangeUpperBound =
    rangeBounds !== undefined ? maximalKey(rangeBounds) : undefined;

  return (
    <div className="mt-4 space-y-4">
      <div className="flex flex-col items-center gap-2">
        {orderedPages.map((page, i) => {
          const pageState = page.state;
          const isLoading = pageState.kind === "loading";
          if (isLoading) {
            return <div>Loading...</div>;
          }
          const target = page.args.target;
          const lowerBound = pageState.result.lowerBound;
          const upperBound = pageState.result.upperBound;
          const docs = pageState.result.results;
          const docsWithKeys: Array<DocEvent> = docs.map((doc) => ({
            kind: "doc" as const,
            value: doc,
            key: cursorForSyncObject(
              syncSchema,
              page.args.syncTableName,
              page.args.index,
              doc,
            ),
          }));
          let foundTarget = false;
          let foundLowerBound = false;
          let foundUpperBound = false;
          for (const doc of docsWithKeys) {
            if (
              rangeLowerBound !== undefined &&
              rangeUpperBound !== undefined &&
              compareKeys(doc.key, rangeLowerBound) >= 0 &&
              compareKeys(doc.key, rangeUpperBound) <= 0
            ) {
              addEventToDoc(doc, "exact", "inRange");
            }
            if (
              compareValues(doc.key.value as any, target.value as any) === 0
            ) {
              addEventToDoc(doc, "exact", "target");
              foundTarget = true;
            }
            if (compareKeys(doc.key, lowerBound) === 0) {
              addEventToDoc(doc, lowerBound.kind, "lowerBound");
              foundLowerBound = true;
            }
            if (compareKeys(doc.key, upperBound) === 0) {
              addEventToDoc(doc, upperBound.kind, "upperBound");
              foundUpperBound = true;
            }
          }

          const keyEvents: Array<KeyEvent> = [];
          if (!foundTarget) {
            addKeyEvent(keyEvents, "target", target);
          }
          if (!foundLowerBound) {
            addKeyEvent(keyEvents, "lowerBound", lowerBound);
          }
          if (!foundUpperBound) {
            addKeyEvent(keyEvents, "upperBound", upperBound);
          }
          if (rangeLowerBound !== undefined) {
            addKeyEvent(keyEvents, "rangeLowerBound", rangeLowerBound);
          }
          if (rangeUpperBound !== undefined) {
            addKeyEvent(keyEvents, "rangeUpperBound", rangeUpperBound);
          }
          const allEvents: Array<Event> = [...docsWithKeys, ...keyEvents];
          allEvents.sort((a, b) => compareKeys(a.key, b.key));

          return (
            <div
              className="flex flex-col w-full"
              onMouseEnter={() => setSelectedPage(i)}
              onMouseLeave={() => setSelectedPage(null)}
            >
              {allEvents.map((event) => {
                if (event.kind === "doc") {
                  return renderDocEvent(event, selectedPage === i);
                }
                const isSelected = selectedPage === i;
                const isRangeEvent =
                  event.events.includes("rangeLowerBound") ||
                  event.events.includes("rangeUpperBound");
                return (
                  <Divider
                    key={JSON.stringify(event.key)}
                    style={
                      isSelected && isRangeEvent
                        ? "border-blue-500 border-dashed"
                        : isSelected
                          ? "border-gray-900"
                          : "border-gray-300"
                    }
                  >
                    <div className="flex flex-col items-end gap-1 text-xs">
                      <AbbreviatedIndexKey indexKey={event.key} />
                      {event.events.join(", ")}
                    </div>
                  </Divider>
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function renderDocEvent(event: DocEvent, isSelected: boolean) {
  const isInRange =
    event.exactEvents !== undefined && event.exactEvents.includes("inRange");
  const exactEventsWithoutInRange =
    event.exactEvents !== undefined
      ? event.exactEvents.filter((e) => e !== "inRange")
      : [];
  const docTrigger = (
    <div
      key={event.value._id}
      className="flex flex-row items-center justify-between w-full"
    >
      <div className="flex flex-row items-center gap-2">
        {isInRange && <div className="w-1 h-1 rounded-full bg-blue-500" />}
        {event.value._id}
      </div>
      <div className="flex flex-col items-end gap-1">
        <AbbreviatedIndexKey indexKey={event.key} />
        {exactEventsWithoutInRange.length > 0 && (
          <div className="flex flex-row items-center gap-1 text-xs">
            {exactEventsWithoutInRange.join(", ")}
          </div>
        )}
      </div>
    </div>
  );
  return (
    <div
      className={
        isSelected
          ? "flex flex-col w-full border my-1 rounded-lg p-2 border-gray-900"
          : "flex flex-col w-full border my-1 rounded-lg p-2 border-gray-200"
      }
    >
      {event.predecessorEvents !== undefined && (
        <Divider style={isSelected ? "border-gray-900" : "border-gray-300"}>
          <div className="flex flex-col items-end gap-1 text-xs">
            {event.predecessorEvents.join(", ")}
            <AbbreviatedIndexKey indexKey={event.key} />
          </div>
        </Divider>
      )}

      <Collapsible trigger={docTrigger}>
        <div className="p-2">
          <pre className="text-xs bg-white p-2 rounded border border-gray-200 overflow-auto">
            {JSON.stringify(event.value, null, 2)}
          </pre>
        </div>
      </Collapsible>
      {event.successorEvents !== undefined && (
        <Divider style={isSelected ? "border-gray-900" : "border-gray-300"}>
          <div className="flex flex-col items-end gap-1 text-xs">
            {event.successorEvents.join(", ")}
            <AbbreviatedIndexKey indexKey={event.key} />
          </div>
        </Divider>
      )}
    </div>
  );
}

function Collapsible({
  trigger,
  children,
}: {
  trigger: React.ReactNode;
  children: React.ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  return (
    <div>
      <div
        className="text-xs cursor-pointer hover:text-blue-500 flex flex-row items-center gap-1 w-full"
        onClick={() => setExpanded(!expanded)}
      >
        {expanded ? <ChevronDownIcon /> : <ChevronRightIcon />}
        {trigger}
      </div>
      {expanded && children}
    </div>
  );
}

function AbbreviatedIndexKey({ indexKey }: { indexKey: Key }) {
  const [expanded, setExpanded] = useState(false);
  const truncatedKey = JSON.stringify(indexKey).slice(0, 20) + "...";
  const fullKey = JSON.stringify(indexKey);

  return (
    <span
      className="text-xs cursor-pointer hover:text-blue-500"
      onClick={() => setExpanded(!expanded)}
    >
      {expanded ? fullKey : truncatedKey}
    </span>
  );
}
