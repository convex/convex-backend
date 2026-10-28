import { ValidatorJSON } from "convex/values";
import { SchemaJson } from "@common/lib/format";

export function documentValidatorForTable(
  activeSchema: SchemaJson,
  tableName: string,
) {
  const documentValidator = activeSchema.tables.find(
    (t) => t.tableName === tableName,
  )?.documentType;
  return documentValidator;
}

export function validatorForColumn(
  documentValidator: SchemaJson["tables"][0]["documentType"],
  columnName: string,
): ValidatorJSON | undefined {
  const validator =
    documentValidator?.type === "object"
      ? documentValidator.value[columnName]?.fieldType
      : undefined;
  return validator;
}

// The system fields have no entry in the document validator: `_id` is an id
// of the table it was read from, and `_creationTime` is a number.
export function validatorForFilterField(
  documentValidator: SchemaJson["tables"][0]["documentType"],
  tableName: string,
  fieldName?: string,
): ValidatorJSON | undefined {
  if (!documentValidator || fieldName === undefined) {
    return undefined;
  }

  switch (fieldName) {
    case "_id":
      return { type: "id", tableName };
    case "_creationTime":
      return { type: "number" };
    default:
      return validatorForColumn(documentValidator, fieldName);
  }
}
