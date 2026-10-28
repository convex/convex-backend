import { useQuery } from "convex/react";
import { useMemo } from "react";
import { Shape } from "shapes";
import udfs from "@common/udfs";
import { parseAndFilterToSingleTable } from "system-udfs/convex/_system/frontend/lib/filters";
import { SchemaJson } from "@common/lib/format";
import { useNents } from "@common/lib/useNents";
import { useTableShapes } from "@common/lib/deploymentApi";
import { LoadingTransition } from "@ui/Loading";
import { ShowSchema } from "@common/features/data/components/ShowSchema";
import { ValidatorJSON } from "convex/values";

export interface TableSchemaStatus {
  tableName: string;
  isDefined: boolean;
  isDefinedInInProgressSchema: boolean;
  referencedByTable: string | undefined;
  isValidationRunning: boolean;
}

interface Schema {
  tables: Table[];
  schemaValidation: boolean;
}
export interface Table {
  tableName: string;
  documentType: Validator | null;
}
type Validator =
  | { type: "null" }
  | { type: "number" }
  | { type: "bigint" }
  | { type: "boolean" }
  | { type: "string" }
  | { type: "boolean" }
  | { type: "bytes" }
  | { type: "any" }
  | { type: "literal"; value: any }
  | { type: "id"; tableName: string }
  | { type: "array"; value: Validator }
  | { type: "record"; keys: Validator; values: Validator }
  | { type: "union"; value: Validator[] }
  | {
      type: "object";
      value: Record<string, { fieldType: Validator; optional: boolean }>;
    };

function validatorReferencesTable(
  validator: Validator,
  tableName: string,
): boolean {
  switch (validator.type) {
    case "id":
      return validator.tableName === tableName;
    case "array":
      return validatorReferencesTable(validator.value, tableName);
    case "record":
      return (
        validatorReferencesTable(validator.keys, tableName) ||
        validatorReferencesTable(validator.values, tableName)
      );
    case "union":
      return validator.value.some((v) =>
        validatorReferencesTable(v, tableName),
      );
    case "object":
      return Object.values(validator.value).some((v) =>
        validatorReferencesTable(v.fieldType, tableName),
      );
    default:
      return false;
  }
}

export function useSingleTableSchemaStatus(
  tableName: string,
): TableSchemaStatus | undefined {
  const schemas = useQuery(udfs.getSchemas.default, {
    componentId: useNents().selectedNent?.id ?? null,
  });

  const { isDefined, referencedByTable } = useMemo(() => {
    const active: Schema | undefined = schemas?.active
      ? JSON.parse(schemas.active)
      : undefined;

    return {
      isDefined:
        active?.tables.find((table) => table.tableName === tableName) !==
        undefined,
      referencedByTable: active?.tables.find((table) =>
        validatorReferencesTable(
          table.documentType ?? { type: "any" },
          tableName,
        ),
      )?.tableName,
    };
  }, [schemas?.active, tableName]);

  const isDefinedInInProgressSchema = useMemo(() => {
    const inProgress: Schema | undefined = schemas?.inProgress
      ? JSON.parse(schemas.inProgress)
      : undefined;

    return (
      inProgress?.tables.find((table) => table.tableName === tableName) !==
      undefined
    );
  }, [schemas?.inProgress, tableName]);

  const isValidationRunning = schemas?.inProgress !== undefined;
  return schemas
    ? {
        tableName,
        isDefined,
        isDefinedInInProgressSchema,
        referencedByTable,
        isValidationRunning,
      }
    : undefined;
}

// Adds a comment '// Other tables here...' into code, assuming that code
// represents a valid schema.ts file containing only one table whose name is
// the given tableName.
function insertOtherTablesComment(tableName: string, code: string): string {
  const splitLines = code.split("\n");

  // Use a token to find the start of the table definition
  // assumes that the string 'tableName: defineTable' is unique in the file
  const tableNameToken = `${tableName}: defineTable`;
  const tableStartIndex = splitLines.findIndex((value) =>
    value.trim().includes(tableNameToken),
  );

  // Check for a single line table definition definition like:
  // export default defineSchema({ Test: defineTable({ my_id: s.string() }) });
  const tableStartLine = splitLines[tableStartIndex];
  const isSingleLineDefinition = tableStartLine
    .trim()
    .startsWith("export default defineSchema");
  if (isSingleLineDefinition) {
    const tableStartIndexInLine = tableStartLine.indexOf(tableNameToken);

    const chars = tableStartLine.split("");
    // -1 to remove the ' ' character
    chars.splice(tableStartIndexInLine - 1, 1, "\n  ");
    // export default defineSchema({
    //   Test: defineTable({ my_id: s.string() }) });
    // -1 to remove the ' ' character
    chars.splice(chars.length - 4, 1, "\n");
    // export default defineSchema({
    //   Test: defineTable({ my_id: s.string() })
    // });
    const linesToAdd = chars.join("").split("\n");
    splitLines.splice(tableStartIndex, 1, ...linesToAdd);
  }

  splitLines.splice(
    tableStartIndex + (isSingleLineDefinition ? 1 : 0),
    0,
    "  // Other tables here...\n",
  );
  // export default defineSchema({
  //   // Other tables here...
  //
  //   Test: defineTable({ my_id: s.string() })
  // });

  return splitLines.join("\n");
}

function TableSchema({
  tables,
  tableName,
  activeSchema,
  inProgressSchema,
  hadShapeError,
  highlightField,
}: {
  tables: Map<string, Shape>;
  tableName: string;
  activeSchema?: SchemaJson;
  inProgressSchema?: SchemaJson;
  hadShapeError: boolean;
  highlightField?: string;
}) {
  const tableShape = tables.get(tableName);
  if ((!tableShape || tableShape.type === "Never") && !activeSchema) {
    return (
      <div className="sm:px-2">
        Add at least one document to your table to see a suggested schema here.
      </div>
    );
  }

  const singleTableShapes = new Map();
  singleTableShapes.set(tableName, tableShape);
  const lineHighlighter = (code: string) => {
    const splitLines = code.split("\n");
    const tableStartIndex =
      splitLines.findIndex((value) => value.trim().startsWith(tableName)) + 1;

    if (highlightField) {
      // Highlight only the selected field. Find its line within the table
      // body, then extend through any lines more indented than it so
      // multi-line validators (objects, unions) are highlighted in full.
      const fieldStart = splitLines.findIndex(
        (value, index) =>
          index >= tableStartIndex &&
          value.trim().startsWith(`${highlightField}:`),
      );
      if (fieldStart !== -1) {
        const fieldIndent = splitLines[fieldStart].search(/\S/);
        let fieldEnd = fieldStart;
        for (let i = fieldStart + 1; i < splitLines.length; i++) {
          if (splitLines[i].trim() === "") {
            continue;
          }
          if (splitLines[i].search(/\S/) <= fieldIndent) {
            break;
          }
          fieldEnd = i;
        }
        return {
          startLineNumber: fieldStart + 1,
          endLineNumber: fieldEnd + 1,
        };
      }
    }

    return {
      startLineNumber: tableStartIndex,
      endLineNumber: splitLines.length - 1,
    };
  };
  const codeTransformation = (code: string) =>
    insertOtherTablesComment(tableName, code);

  return (
    <ShowSchema
      activeSchema={activeSchema}
      inProgressSchema={inProgressSchema}
      shapes={singleTableShapes}
      hasShapeError={hadShapeError}
      showLearnMoreLink={false}
      lineHighlighter={lineHighlighter}
      codeTransformation={codeTransformation}
    />
  );
}

export function TableSchemaContainer({
  tableName,
  highlightField,
}: {
  tableName: string;
  highlightField?: string;
}) {
  const schemas = useQuery(udfs.getSchemas.default, {
    componentId: useNents().selectedNent?.id ?? null,
  });
  const { activeSchema, inProgressSchema } = useMemo(() => {
    if (!schemas) return {};

    return {
      activeSchema: parseAndFilterToSingleTable(tableName, schemas.active),
      inProgressSchema: parseAndFilterToSingleTable(
        tableName,
        schemas.inProgress,
      ),
    };
  }, [tableName, schemas]);
  const { tables, hadError: hadShapeError } = useTableShapes();
  return (
    <LoadingTransition>
      {tables && schemas && (
        <TableSchema
          tables={tables}
          tableName={tableName}
          activeSchema={activeSchema}
          inProgressSchema={inProgressSchema}
          hadShapeError={hadShapeError}
          highlightField={highlightField}
        />
      )}
    </LoadingTransition>
  );
}

export function topLevelFieldsForValidator(validator: ValidatorJSON): {
  fields: Array<string>;
  areFieldsComplete: boolean;
} {
  if (validator.type === "object") {
    return {
      fields: ["_id", "_creationTime", ...Object.keys(validator.value)],
      areFieldsComplete: true,
    };
  }
  if (validator.type === "union") {
    const fields = new Set<string>();
    let areFieldsComplete = true;
    validator.value.forEach((v) => {
      const result = topLevelFieldsForValidator(v);
      result.fields.forEach((f) => fields.add(f));
      areFieldsComplete = areFieldsComplete && result.areFieldsComplete;
    });
    return { fields: Array.from(fields), areFieldsComplete };
  }
  return { fields: [], areFieldsComplete: false };
}
