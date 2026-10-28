import { queryPrivateSystem } from "../secretSystemTables";
import { Doc, Id } from "../../_generated/dataModel";

export const list = queryPrivateSystem("ViewData")({
  args: {},
  handler: async function ({ db }) {
    const componentDocs = await db.query("_components").collect();
    const idToDoc = new Map<Id<"_components">, Doc<"_components">>(
      componentDocs.map((doc) => [doc._id, doc]),
    );
    const idToPath = new Map<string, string>();
    function computeIdToPath(doc: Doc<"_components">): string {
      if (idToPath.has(doc._id)) {
        return idToPath.get(doc._id)!;
      }
      let path = "";
      if (!doc.parent) {
        // Root component
        path = "";
      } else {
        const parentPath = computeIdToPath(idToDoc.get(doc.parent)!);
        if (parentPath.length === 0) {
          path = doc.name!;
        } else {
          path = `${parentPath}/${doc.name!}`;
        }
      }
      idToPath.set(doc._id, path);
      return path;
    }
    return componentDocs.map((doc) => ({
      id: doc._id,
      name: doc.name,
      path: computeIdToPath(doc),
      args: Object.fromEntries(doc.args ?? []),
      state: doc.state ?? "active",
      httpPrefix: doc.httpPrefix ?? null,
    }));
  },
});
