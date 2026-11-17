import { useCallback, useState } from "react";
import { IndexRangeBounds, PaginatorSubscriptionId } from "../shared/types";
// import { SyncQueryManager } from "../local-store/syncQueryManager";
import { useMutation } from "convex/react";
import { LocalDbReaderImpl } from "../browser/localDbReader";
import { PageTimeline } from "./PageTimeline";
import { TableViewer } from "./TableView";
import { anyApi } from "convex/server";
import { LocalStoreClient } from "../browser/ui";

interface DebugViewProps {
  onClose: () => void;
}

function renderBounds(bounds: IndexRangeBounds) {
  return (
    <span className="text-gray-500">
      <span className="text-gray-900 px-1 font-bold">
        {bounds.lowerBoundInclusive ? "[" : "("}
      </span>
      <span className="text-gray-500">
        {bounds.lowerBound.length > 0
          ? JSON.stringify(bounds.lowerBound)
          : "-∞"}
      </span>
      <span className="text-gray-900 px-1 font-bold">{"→"}</span>
      <span className="text-gray-500">
        {bounds.upperBound.length > 0 ? JSON.stringify(bounds.upperBound) : "∞"}
      </span>
      <span className="text-gray-900 px-1 font-bold">
        {bounds.upperBoundInclusive ? "]" : ")"}
      </span>
    </span>
  );
}

type DebugTab = "tables" | "documents" | "mutations" | "queries" | "indexeddb";

function TabContent({ tab }: { tab: DebugTab }) {
  const _syncQueryManager: LocalStoreClient = (globalThis as any).localDb;

  if (tab === "tables") {
    return (
      <div className="overflow-auto max-h-[600px]">
        <TableViewer />
      </div>
    );
  }

  if (tab === "indexeddb") {
    return (
      <div className="overflow-auto max-h-[600px]">
        <IndexedDBViewer />
      </div>
    );
  }

  if (tab === "queries") {
    return (
      <div className="overflow-auto max-h-[600px]">
        <QueryViewer />
      </div>
    );
  }

  return (
    <div className="flex items-center justify-center h-64 text-gray-500">
      Coming soon...
    </div>
  );
}

function IndexedDBViewer() {
  const _syncQueryManager: LocalStoreClient = (globalThis as any).localDb;
  const [pages, _setPages] = useState<any[]>([]);
  const [selectedPage, setSelectedPage] = useState<any>(null);
  const [expandedDocs, setExpandedDocs] = useState<Set<string>>(new Set());

  // TODO -- restore this
  // const loadPages = async () => {
  //   const allPages =
  //     await syncQueryManager.clientStore.serverStore.indexedDbClient.db
  //       .table("pages")
  //       .toArray();
  //   setPages(allPages);
  // };

  // useEffect(() => {
  //   void loadPages();
  // }, [syncQueryManager]);

  const toggleDocument = (id: string) => {
    const newExpanded = new Set(expandedDocs);
    if (newExpanded.has(id)) {
      newExpanded.delete(id);
    } else {
      newExpanded.add(id);
    }
    setExpandedDocs(newExpanded);
  };

  const handleDeleteDB = async () => {
    if (
      window.confirm(
        "Are you sure you want to delete the IndexedDB? This action cannot be undone.",
      )
    ) {
      // TODO -- restore this
      // await syncQueryManager.clientStore.serverStore.indexedDbClient.db.delete();
      window.location.reload(); // Reload the page to reinitialize the DB
    }
  };

  const clearAll = useMutation(anyApi.sync.misc.clearAll);
  const handleClearAll = async () => {
    if (
      window.confirm(
        "Are you sure you want to clear all data? This action cannot be undone.",
      )
    ) {
      await clearAll();
      // TODO: We shouldn't need to do this.
      // await syncQueryManager.clientStore.serverStore.indexedDbClient.db.delete();
      window.location.reload(); // Reload the page to reinitialize the DB
    }
  };

  return (
    <div className="grid grid-cols-3 gap-8 h-[600px]">
      {/* Left Column - Pages View */}
      <div className="col-span-2">
        <h3 className="font-medium text-gray-900 mb-4">Stored Pages</h3>
        <div className="overflow-auto pr-2 h-[calc(100%-2rem)]">
          <div className="grid grid-cols-1 gap-4">
            {pages.map((page, i) => (
              <div key={i} className="border rounded-lg p-4">
                <button
                  onClick={() =>
                    setSelectedPage(selectedPage === page ? null : page)
                  }
                  className="flex items-center gap-2 w-full text-left"
                >
                  <span
                    className="transform transition-transform duration-200"
                    style={{
                      display: "inline-block",
                      transform: `rotate(${
                        selectedPage === page ? "90deg" : "0deg"
                      })`,
                    }}
                  >
                    ▶
                  </span>
                  <div>
                    <div className="font-medium">
                      {page.table} / {page.index}
                    </div>
                    <div className="text-sm text-gray-500">
                      {page.documents.length} documents
                    </div>
                  </div>
                </button>

                {selectedPage === page && (
                  <div className="mt-4 pl-6 space-y-4">
                    <div className="space-y-1">
                      <div>
                        <span className="text-gray-500">Lower Bound:</span>{" "}
                        {JSON.stringify(page.lowerBound)}
                      </div>
                      <div>
                        <span className="text-gray-500">Upper Bound:</span>{" "}
                        {JSON.stringify(page.upperBound)}
                      </div>
                    </div>

                    <div>
                      {page.documents.map((doc: any) => (
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
                      ))}
                    </div>
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Right Column - Actions */}
      <div>
        <h3 className="font-medium text-gray-900 mb-4">Actions</h3>
        <div className="border rounded-lg p-4 bg-red-50 space-y-4">
          <h4 className="font-medium text-red-900">Danger Zone</h4>
          <p className="text-sm text-red-700">
            Deleting the IndexedDB will remove all cached data. The application
            will need to reload and refetch data from the server.
          </p>
          <button
            onClick={() => {
              void handleDeleteDB();
            }}
            className="bg-red-600 text-white px-4 py-2 rounded hover:bg-red-700 
                     transition-colors text-sm font-medium focus:outline-none focus:ring-2 
                     focus:ring-red-500 focus:ring-offset-2"
          >
            Delete IndexedDB
          </button>

          <p className="text-sm text-red-700">
            Clear all data from the server and also delete the IndexedDB.
          </p>
          <button
            onClick={() => void handleClearAll()}
            className="bg-red-600 text-white px-4 py-2 rounded hover:bg-red-700 
                     transition-colors text-sm font-medium focus:outline-none focus:ring-2 
                     focus:ring-red-500 focus:ring-offset-2"
          >
            Clear All Data
          </button>
        </div>
      </div>
    </div>
  );
}

function QueryViewer() {
  const queries: Map<string, LocalDbReaderImpl> =
    (globalThis as any).debugSyncQueries ?? new Map();

  return (
    <div className="space-y-6">
      {Array.from(queries.entries()).map(([id, query]) => (
        <div key={id} className="border rounded-lg p-4">
          <h3 className="text-lg font-semibold text-gray-800 mb-2">
            {id.startsWith("random:") ? "useSyncQuery" : id}
          </h3>
          <div className="pl-4">
            <SingleQueryView query={query} />
          </div>
        </div>
      ))}
    </div>
  );
}

function SingleQueryView({ query }: { query: LocalDbReaderImpl }) {
  const indexRanges = Array.from(query.debugIndexRanges.entries());

  return (
    <div className="space-y-4">
      {indexRanges.map(([id, indexRange]) => (
        <IndexRangeView
          id={id as PaginatorSubscriptionId}
          indexRange={indexRange}
        />
      ))}
    </div>
  );
}

function IndexRangeView({
  indexRange,
  id,
}: {
  id: PaginatorSubscriptionId;
  indexRange: {
    table: string;
    index: string;
    indexRangeBounds: IndexRangeBounds;
    order: "asc" | "desc";
    limit: number;
  };
}) {
  const syncQueryManager: LocalStoreClient = (globalThis as any).localDb;

  // TODO -- restore this
  const orderedPages: any[] = [];
  // const indexPaginator = syncQueryManager.coreLocalStore.paginator.getIndexPaginator(
  //   indexRange.table,
  //   indexRange.index,
  // );
  // const paginatorSubscription = indexPaginator.subscriptions.get(id)!;
  // const pageSubscriptionIds = paginatorSubscription.pageSubscriptionIds;
  // const fulfilled = indexPaginator.tryFulfillSingleSubscription(
  //   paginatorSubscription,
  // );
  // if (fulfilled.state !== "fulfilled") {
  //   return <div>Waiting on loading page</div>;
  // }
  // // NOTE there's also fulfilled.pageSubscriptionIds which should be the same.
  // const orderedPages =
  //   syncQueryManager.clientStore.serverStore.orderedPageResults(
  //     pageSubscriptionIds,
  //   );

  return (
    <div key={id} className="bg-white rounded-lg border border-gray-200 p-4">
      <div className="flex items-center gap-2 text-sm text-gray-500 mb-1">
        <span className="font-medium">Index Range Subscription Id:</span>
        <code className="bg-gray-100 px-1 py-0.5 rounded">{id}</code>
      </div>
      <div className="flex items-center gap-2 text-sm">
        <span className="font-medium text-gray-700">{indexRange.table}</span>
        <span className="text-gray-700">.</span>
        <span className="font-medium text-gray-700">{indexRange.index}</span>
      </div>
      <div className="text-xs text-gray-500">
        <div>Range: {renderBounds(indexRange.indexRangeBounds)}</div>
        <div>Order: {indexRange.order}</div>
        <div>
          Limit:{" "}
          {indexRange.limit === Number.POSITIVE_INFINITY
            ? "∞"
            : indexRange.limit}
        </div>
        <PageTimeline
          syncSchema={syncQueryManager.syncSchema}
          orderedPages={orderedPages}
          rangeBounds={indexRange.indexRangeBounds}
        />
      </div>
    </div>
  );
}

export function DebugView({ onClose }: DebugViewProps) {
  const [activeTab, setActiveTab] = useState<DebugTab>("tables");
  const handleBackdropClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) {
        onClose();
      }
    },
    [onClose],
  );

  return (
    <div
      style={{
        position: "fixed",
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        display: "flex",
        alignItems: "start",
        justifyContent: "center",
        overflow: "auto",
        padding: "8px",
        zIndex: 50,
      }}
      onClick={handleBackdropClick}
    >
      <div
        style={{
          backgroundColor: "white",
          maxHeight: "calc(100vh - 16px)",
          marginTop: "8px",
          marginBottom: "8px",
          marginLeft: "auto",
          marginRight: "auto",
          padding: "16px",
          width: "100vw",
        }}
      >
        <h1 className="text-2xl font-bold mb-6 text-gray-800">Debug View</h1>

        {/* Tabs */}
        <div className="border-b border-gray-200 mb-6">
          <nav className="-mb-px flex space-x-8">
            {[
              { id: "tables", label: "Table Viewer" },
              { id: "documents", label: "Documents" },
              { id: "mutations", label: "Mutations" },
              { id: "queries", label: "Local Sync Queries" },
              { id: "indexeddb", label: "IndexedDB" },
            ].map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id as DebugTab)}
                className={`
                  py-2 px-1 border-b-2 font-medium text-sm
                  ${
                    activeTab === tab.id
                      ? "border-blue-500 text-blue-600"
                      : "border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300"
                  }
                `}
              >
                {tab.label}
              </button>
            ))}
          </nav>
        </div>

        {/* Tab Content */}
        <TabContent tab={activeTab} />
      </div>
    </div>
  );
}
