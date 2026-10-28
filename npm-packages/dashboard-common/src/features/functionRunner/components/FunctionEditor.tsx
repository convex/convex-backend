import { BeforeMount, Editor } from "@monaco-editor/react";
import { PlayIcon } from "@radix-ui/react-icons";
import classNames from "classnames";
import { FunctionResult } from "convex/browser";
import { useQuery } from "convex/react";
// special case: too annoying to move convexServerTypes to a separate file right now
import {
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import udfs from "@common/udfs";
import { Uri } from "monaco-editor/esm/vs/editor/editor.api";
import { Button } from "@ui/Button";
import { Loading } from "@ui/Loading";
import { stringifyValue } from "@common/lib/stringifyValue";
import { SchemaJson, displaySchema } from "@common/lib/format";
import { useRunTestFunction } from "@common/features/functionRunner/lib/client";
import { ComponentId } from "@common/lib/useNents";
import { useCurrentTheme } from "@common/lib/useCurrentTheme";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";
import { Result } from "@common/features/functionRunner/components/Result";
import {
  RunHistory,
  RunHistoryItem,
  useRunHistory,
} from "@common/features/functionRunner/components/RunHistory";
import convexServerTypes from "../../../lib/generated/convexServerTypes.json";

// Used for typechecking
const globals = `
import type { QueryBuilder } from "./convex/server"
import type { DataModel } from "./_generated/dataModel"

declare global {
  /**
   * Define a query in this Convex app's public API.
   *
   * This function will be allowed to read your Convex database and will be accessible from the client.
   *
   * @param func - The query function. It receives a {@link QueryCtx} as its first argument.
   * @returns The wrapped query. Include this as an \`export\` to name it and make it accessible.
   */
  const query: QueryBuilder<DataModel, "public">;

  /**
   * Define a query that is only accessible from other Convex functions (but not from the client).
   *
   * This function will be allowed to read from your Convex database. It will not be accessible from the client.
   *
   * @param func - The query function. It receives a {@link QueryCtx} as its first argument.
   * @returns The wrapped query. Include this as an \`export\` to name it and make it accessible.
   */
  const internalQuery: QueryBuilder<DataModel, "public">;

  const process: {
    env: {
      [key: string]: string | undefined;
      CONVEX_CLOUD_URL: string;
      CONVEX_SITE_URL: string;
    }
  }
}
`;

// Used when bundling and at runtime
const preamble = `
import { query, internalQuery } from "convex:/_system/repl/wrappers.js";
`;

const generatedServer = `import { DataModelFromSchemaDefinition, GenericQueryCtx } from "../convex/server";
import { DataModel } from "./dataModel";
export type QueryCtx = GenericQueryCtx<DataModel>;
`;

const CONVEX_SERVER_FILES = {
  ...convexServerTypes,
  "file:///globals.d.ts": globals,
  "file://_generated/server.d.ts": generatedServer,
};

const generatedDataModelWithoutSchema = `
import { AnyDataModel } from "../convex/server";
import type { GenericId } from "../convex/values";

/**
 * The type of a document stored in Convex.
 */
export type Doc = any;

/**
 * An identifier for a document in Convex.
 *
 * Convex documents are uniquely identified by their \`Id\`, which is accessible
 * on the \`_id\` field. To learn more, see [Document IDs](https://docs.convex.dev/using/document-ids).
 *
 * Documents can be loaded using \`db.get(tableName, id)\` in query and mutation functions.
 *
 * IDs are just strings at runtime, but this type can be used to distinguish them from other
 * strings when type checking.
 */
export type Id<TableName extends TableNames = TableNames> =
  GenericId<TableName>;

/**
 * A type describing your Convex data model.
 *
 * This type includes information about what tables you have, the type of
 * documents stored in those tables, and the indexes defined on them.
 *
 * This type is used to parameterize methods like \`queryGeneric\` and
 * \`mutationGeneric\` to make them type-safe.
 */
export type DataModel = AnyDataModel;
`;

const generatedDataModelWithSchema = `import type {
  DataModelFromSchemaDefinition,
  DocumentByName,
  TableNamesInDataModel,
  SystemTableNames,
} from "../convex/server";
import type { GenericId } from "../convex/values";
import schema from "../schema";

/**
 * The names of all of your Convex tables.
 */
export type TableNames = TableNamesInDataModel<DataModel>;

/**
 * The type of a document stored in Convex.
 *
 * @typeParam TableName - A string literal type of the table name (like "users").
 */
export type Doc<TableName extends TableNames> = DocumentByName<
  DataModel,
  TableName
>;

/**
 * An identifier for a document in Convex.
 *
 * Convex documents are uniquely identified by their \`Id\`, which is accessible
 * on the \`_id\` field. To learn more, see [Document IDs](https://docs.convex.dev/using/document-ids).
 *
 * Documents can be loaded using \`db.get(tableName, id)\` in query and mutation functions.
 *
 * IDs are just strings at runtime, but this type can be used to distinguish them from other
 * strings when type checking.
 *
 * @typeParam TableName - A string literal type of the table name (like "users").
 */
export type Id<TableName extends TableNames | SystemTableNames> =
  GenericId<TableName>;


/**
 * A type describing your Convex data model.
 *
 * This type includes information about what tables you have, the type of
 * documents stored in those tables, and the indexes defined on them.
 *
 * This type is used to parameterize methods like \`queryGeneric\` and
 * \`mutationGeneric\` to make them type-safe.
 */
export type DataModel = DataModelFromSchemaDefinition<typeof schema>;
`;

function defaultCode(tableName: string) {
  return `export default query({
  handler: async (ctx) => {
    console.log("Write and test your query function here!");
    return await ctx.db.query("${tableName}").take(10);
  },
})`;
}

export function useFunctionEditor(
  initialTableName: string | null,
  componentId: ComponentId,
  runHistoryItem: RunHistoryItem | undefined,
  setRunHistoryItem: (item: RunHistoryItem) => void,
  onRanCustomQuery?: () => void,
) {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canRunTestQuery = useIsOperationAllowed("RunTestQuery");
  const canViewData = useIsOperationAllowed("ViewData");
  const currentTheme = useCurrentTheme();
  const prefersDark = currentTheme === "dark";

  const [prevInitialTable, setPrevInitialTable] = useState<
    string | null | undefined
  >(undefined);

  const [code, setCode] = useState<string>();

  // Skip the schema query when the member can't view data — otherwise the
  // backend rejects the request and the resulting throw escapes the
  // ErrorBoundary on the next subscription notify.
  const schemas = useQuery(
    udfs.getSchemas.default,
    canViewData ? { componentId } : "skip",
  );
  const schema = useMemo(() => {
    if (schemas === undefined) {
      return undefined;
    }
    return schemas.active !== undefined
      ? displaySchema(JSON.parse(schemas.active) as SchemaJson, "../")
      : null;
  }, [schemas]);

  const [isInFlight, setIsInFlight] = useState(false);
  const [lastRequestTiming, setLastRequestTiming] = useState<{
    startedAt: number;
    endedAt: number;
  }>();

  useEffect(() => {
    if (runHistoryItem) {
      setResult(undefined);
      setLastRequestTiming(undefined);
    }
  }, [runHistoryItem]);

  const [monaco, setMonaco] = useState<Parameters<BeforeMount>[0]>();
  // We store this in state to avoid importing the Uri class /facepalm
  const [monacoModelUri, setMonacoModelUri] = useState<Uri>();

  if (prevInitialTable !== initialTableName) {
    setPrevInitialTable(initialTableName);
    setCode(defaultCode(initialTableName ?? "YOUR_TABLE_NAME"));
  }

  // Refresh files related to the schema
  useEffect(() => {
    if (schema === undefined || monaco === undefined) {
      return;
    }

    const dataModel =
      schema === null
        ? generatedDataModelWithoutSchema
        : generatedDataModelWithSchema;

    const dataModelUri = monaco.Uri.parse("file:///_generated/dataModel.d.ts");
    monaco.editor.getModel(dataModelUri)?.dispose();
    monaco.editor.createModel(dataModel, "typescript", dataModelUri);

    if (schema !== null) {
      const schemaUri = monaco.Uri.parse("file:///schema.ts");
      const model = monaco.editor.getModel(schemaUri);
      model?.dispose();
      monaco.editor.createModel(schema, "typescript", schemaUri);
    }
  }, [schema, monaco]);

  const [result, setResult] = useState<FunctionResult>();

  const runTestFunction = useRunTestFunction();

  const { appendRunHistory } = useRunHistory("_testQuery", componentId);

  const onSave = useCallback(async () => {
    if (monaco === undefined || monacoModelUri === undefined) {
      return;
    }
    let functionResult: FunctionResult | undefined;
    const startedAt = Date.now();
    setIsInFlight(true);
    try {
      const worker = await monaco.languages.typescript.getTypeScriptWorker();
      const client = await worker(monacoModelUri);
      const compiled = await client.getEmitOutput(monacoModelUri.toString());
      functionResult = await runTestFunction(
        preamble + compiled.outputFiles[0].text,
        componentId || undefined,
      );
    } catch (e: any) {
      functionResult = {
        success: false,
        errorMessage: e.message,
        logLines: [],
      };
    } finally {
      // Wait a moment before re-enabling the button to
      // avoid the user accidently re-running the function.
      setTimeout(() => {
        setIsInFlight(false);
      }, 100);
      const endedAt = Date.now();
      setLastRequestTiming({
        startedAt,
        endedAt,
      });
      setResult(functionResult);
      appendRunHistory({
        type: "custom",
        startedAt,
        endedAt,
        code: code || "",
      });
    }
  }, [
    monaco,
    monacoModelUri,
    runTestFunction,
    componentId,
    appendRunHistory,
    code,
  ]);

  // So the editor has a callback ref to call.
  const saveActionRef = useRef(onSave);
  useEffect(() => {
    saveActionRef.current = onSave;
  }, [onSave]);

  const queryEditor =
    schemas === undefined || !code ? (
      <Loading />
    ) : (
      // Setting a min-h makes sure the editor is able to properly resize when the
      // function tester is expanded/collapsed
      <div className="flex grow flex-col gap-2">
        <div className="flex w-full items-center justify-between">
          <h5 className="text-xs text-content-secondary">Custom Query</h5>
          <RunHistory
            functionIdentifier="_testQuery"
            componentId={componentId}
            selectItem={(item) => {
              if (item.type === "custom") setCode(item.code);
              setRunHistoryItem(item);
            }}
          />
        </div>
        <div
          className="h-full min-h-0 animate-fadeInFromLoading rounded-sm border"
          key={runHistoryItem ? stringifyValue(runHistoryItem) : ""}
        >
          <Editor
            path="/queryEditor"
            className="pt-2"
            options={{
              automaticLayout: true,
              overviewRulerBorder: false,
              scrollBeyondLastLine: false,
              tabFocusMode: true,
              lineNumbers: "off",
              lineNumbersMinChars: 0,
              lineDecorationsWidth: 0,
              minimap: { enabled: false },
              overviewRulerLanes: 0,
              theme: prefersDark ? "vs-dark" : "vs",
              scrollbar: {
                horizontalScrollbarSize: 8,
                verticalScrollbarSize: 8,
                useShadows: false,
                vertical: "visible",
              },
              contextmenu: false,
              bracketPairColorization: { enabled: false },
              guides: {
                bracketPairs: false,
                bracketPairsHorizontal: false,
                highlightActiveBracketPair: false,
                indentation: false,
                highlightActiveIndentation: false,
              },
              selectionHighlight: false,
              occurrencesHighlight: false,
              renderLineHighlight: "none",
            }}
            height="100%"
            defaultLanguage="typescript"
            value={code}
            beforeMount={(monaco_) => {
              setMonaco(monaco_);
              monaco_.languages.typescript.typescriptDefaults.setCompilerOptions(
                {
                  target: monaco_.languages.typescript.ScriptTarget.ESNext,
                  moduleResolution:
                    monaco_.languages.typescript.ModuleResolutionKind.NodeJs,
                  allowNonTsExtensions: true,
                  isolatedModules: true,
                  strict: true,
                  typeRoots: ["file:///convex", "file:///_generated"],
                },
              );

              for (const [fileName, content] of Object.entries(
                CONVEX_SERVER_FILES,
              )) {
                const uri = monaco_.Uri.parse(fileName);
                if (!monaco_.editor.getModel(uri)) {
                  monaco_.editor.createModel(content, "typescript", uri);
                }
              }
            }}
            onMount={(editor, m) => {
              editor.setPosition({ lineNumber: 10, column: 0 });
              setMonacoModelUri(editor.getModel()!.uri);
              const keybindings = [m.KeyMod.CtrlCmd | m.KeyCode.Enter];
              editor.addAction({
                id: "saveAction",
                label: "Save value",
                keybindings,
                run() {
                  if (!isInFlight) {
                    void saveActionRef.current();
                  }
                },
              });
            }}
            onChange={(value) => {
              if (value) {
                setCode(value);
              }
            }}
          />
        </div>
      </div>
    );

  return {
    queryEditor,
    customQueryResult: (
      <Result
        result={result}
        loading={isInFlight}
        lastRequestTiming={lastRequestTiming}
        requestFilter={null}
        startCursor={0}
      />
    ),
    runCustomQueryButton: (
      <Button
        onClick={() => {
          void onSave();
          onRanCustomQuery?.();
        }}
        size="sm"
        className={classNames("items-center justify-center", "w-full")}
        disabled={!canRunTestQuery}
        tip={
          !canRunTestQuery ? (
            <PermissionDeniedTip
              message="You do not have permission to run custom queries in this deployment."
              action="deployment:functions:runTestQuery"
            />
          ) : undefined
        }
        loading={isInFlight}
        icon={<PlayIcon />}
      >
        Run Custom Query
      </Button>
    ),
  };
}
