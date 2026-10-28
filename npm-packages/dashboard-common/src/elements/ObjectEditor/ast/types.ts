import { Node as AcornNode, Position, SourceLocation } from "acorn";
import { ValidatorJSON, Value } from "convex/values";
import type { editor } from "monaco-editor";
import { displayValidatorPretty } from "@common/lib/format";
import { typeForValue } from "@common/elements/ObjectEditor/ast/helpers";

export type Node =
  | ProgramNode
  | ExpressionNode
  | NewExpressionNode
  | ObjectNode
  | ObjectPropertyNode
  | ArrayNode
  | UnaryExpressionNode
  | IdentifierNode
  | CallExpressionNode
  | LiteralNode
  | TemplateLiteralNode;

// Acorn doesn't fully type nested nodes so we do it ourselves.
export type ProgramNode = {
  type: "Program";
  body: [Node];
} & AcornNode;

export type ExpressionNode = {
  type: "ExpressionStatement";
  expression: Node;
} & AcornNode;

export type NewExpressionNode = {
  type: "NewExpression";
  callee: IdentifierNode;
  arguments: LiteralNode[];
} & AcornNode;

export type ObjectNode = {
  type: "ObjectExpression";
  properties: ObjectPropertyNode[];
} & AcornNode;

export type ObjectPropertyNode = {
  type: "Property";
  key: IdentifierNode | LiteralNode;
  value: Node;
} & AcornNode;

export type ArrayNode = {
  type: "ArrayExpression";
  elements: (Node | null)[];
} & AcornNode;

export type UnaryExpressionNode = {
  type: "UnaryExpression";
  operator: string;
  prefix: boolean;
  argument: Node;
} & AcornNode;

export type IdentifierNode = { type: "Identifier"; name: string } & AcornNode;
export type CallExpressionNode = {
  type: "CallExpression";
  callee: { name?: string };
  arguments: Node[];
} & AcornNode;

export type LiteralNode = {
  type: "Literal";
  value: any;
  regex?: any;
} & AcornNode;

export type TemplateElementNode = {
  type: "TemplateElement";
  value: { cooked: string; raw: string };
  tail: boolean;
} & AcornNode;

export type TemplateLiteralNode = {
  type: "TemplateLiteral";
  quasis: TemplateElementNode[];
  expressions: Node[];
} & AcornNode;

export class ConvexValidationError extends Error {
  public readonly markerData: Partial<editor.IMarkerData>;

  public readonly loc?: SourceLocation;

  constructor(
    message: string,
    loc?: SourceLocation,
    markerData?: Partial<editor.IMarkerData>,
  ) {
    super(message);
    this.name = "ConvexValidationError";
    this.markerData = markerData ?? {};
    // Because we're adding an extra character at the start of the first line, the
    // left open paren, ConvexValidationError has to shift any location on the first
    // line back by one.
    if (loc) {
      this.loc = {
        ...loc,
        start: fixPosition(loc.start),
        end: fixPosition(loc.end),
      };
    }
  }
}

export type SchemaValidationErrorCode =
  | "IsNotArray"
  | "IsNotObject"
  | "IsNotBytes"
  | "RecordKeysMismatch"
  | "ObjectMismatch"
  | "UnionMismatch"
  | "LiteralMismatch"
  | "ExtraProperty"
  | "RequiredPropertyMissing";
export class ConvexSchemaValidationError extends ConvexValidationError {
  public readonly code: SchemaValidationErrorCode;

  constructor(
    code: SchemaValidationErrorCode,
    validator: ValidatorJSON,
    value?: Value,
    loc?: SourceLocation,
    markerData?: Partial<editor.IMarkerData>,
  ) {
    const message =
      code === "RequiredPropertyMissing"
        ? `${value ? `Property '${value}'` : "Value"} is missing but required in schema:\n${displayValidatorPretty(validator)}`
        : code === "ExtraProperty"
          ? `Property '${value}' does not exist in schema:\n${displayValidatorPretty(validator)}`
          : code === "UnionMismatch"
            ? `Object does not match any of the types in union:\n${displayValidatorPretty(validator)}`
            : `Type '${value !== undefined ? typeForValue(value) : "Value"}' is not assignable to:\n${displayValidatorPretty(validator)}`;
    super(message, loc, markerData);
    this.name = "ConvexSchemaValidationError";
    this.code = code;
  }
}

function fixPosition(position: Position) {
  return position.line === 1
    ? { ...position, column: position.column - 1 }
    : position;
}

export type WalkResults = {
  value: Value;
  errors: ConvexValidationError[];
};

export const isUndefined = (node: Node) =>
  node.type === "Identifier" && node.name === "undefined";
