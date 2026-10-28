import * as Base64 from "base64-js";
import { ValidatorJSON, Value } from "convex/values";
import cloneDeep from "lodash/cloneDeep";
import {
  Node,
  NewExpressionNode,
  ObjectNode,
  IdentifierNode,
  ArrayNode,
  UnaryExpressionNode,
  ConvexValidationError,
  WalkResults,
  TemplateLiteralNode,
  CallExpressionNode,
  ConvexSchemaValidationError,
} from "@common/elements/ObjectEditor/ast/types";
import { isValidValue } from "@common/elements/ObjectEditor/ast/helpers";

function unsupportedSyntax(n: Node) {
  return {
    value: null,
    errors: [new ConvexValidationError(`Unsupported syntax: ${n.type}`, n.loc)],
  };
}

function identifier(n: IdentifierNode) {
  return {
    value: n.name,
    errors: [],
  };
}

export class Walker {
  private validator = this.options.validator;

  constructor(
    private options: {
      validator?: ValidatorJSON;
    },
  ) {}

  array(n: ArrayNode) {
    const value: Value[] = [];
    const errors: ConvexValidationError[] = [];

    const validNodes = n.elements.filter<Node>((e): e is Node => e !== null);

    if (validNodes.length !== n.elements.length) {
      errors.push(
        new ConvexValidationError("Arrays must not have empty elements", n.loc),
      );
      return { value: null, errors };
    }

    const originalValidator = cloneDeep(this.validator);
    validNodes.forEach((e) => {
      // this.validator needs to be set before walk is called so that recursive calls
      // can access the nested validator.
      if (originalValidator) {
        this.validator =
          originalValidator?.type === "array"
            ? originalValidator.value
            : undefined;
      }

      const { value: elementValue, errors: elementErrors } = this.walk(
        e,
        false,
      );
      value.push(elementValue);
      errors.push(...elementErrors);
    });

    if (
      originalValidator &&
      originalValidator.type !== "array" &&
      originalValidator.type !== "union" &&
      originalValidator.type !== "any"
    ) {
      errors.push(
        new ConvexSchemaValidationError(
          "IsNotArray",
          originalValidator,
          value,
          n.loc,
        ),
      );
    }

    this.validator = originalValidator;

    return { value, errors };
  }

  object(n: ObjectNode, isTopLevel: boolean) {
    const value: { [key: string]: Value } = {};
    const errors: ConvexValidationError[] = [];

    const validatedProperties =
      this.validator?.type === "object"
        ? Object.entries(this.validator.value).reduce(
            (acc, [key, v]) => {
              if (!v.optional) {
                acc[key] = false;
              }
              return acc;
            },
            {} as Record<string, boolean>,
          )
        : {};

    const originalValidator =
      this.validator?.type === "object" ||
      this.validator?.type === "record" ||
      this.validator?.type === "union" ||
      this.validator?.type === "any"
        ? cloneDeep(this.validator)
        : undefined;
    if (this.validator && !originalValidator) {
      errors.push(
        new ConvexSchemaValidationError(
          "IsNotObject",
          this.validator,
          {},
          n.loc,
        ),
      );
      // Don't validate nested values because we're walking an object that doesn't have an object validator.
      this.validator = undefined;
    }

    n.properties.forEach((property) => {
      const currentValidator = this.validator;
      // If we're doing schema validation, we need to temporarily set the value to string
      // so that we don't validate property keys. We'll validate them later.
      if (currentValidator) {
        this.validator = undefined;
      }
      const { value: key, errors: keyErrors } =
        property.key.type === "Identifier"
          ? identifier(property.key)
          : this.walk(property.key, false);
      this.validator = currentValidator;

      if (keyErrors.length) {
        errors.push(...keyErrors);
        return;
      }
      if (typeof key !== "string") {
        errors.push(
          new ConvexValidationError(
            `Unsupported field name: "${key}" must be a string.`,
            property.key.loc,
          ),
        );
        return;
      }

      if (originalValidator?.type === "record") {
        const keys = originalValidator.keys as ValidatorJSON;
        if (!isValidValue(keys, key, false)) {
          errors.push(
            new ConvexSchemaValidationError(
              "RecordKeysMismatch",
              keys,
              key,
              property.key.loc,
            ),
          );
        }
      } else {
        const fieldValidationError = validateConvexFieldName(
          key,
          "field",
          isTopLevel,
        );
        if (fieldValidationError) {
          errors.push(
            new ConvexValidationError(
              `Unsupported field name: ${fieldValidationError}`,
              property.key.loc,
            ),
          );
          return;
        }
      }

      // this.validator needs to be set before walk is called so that recursive calls
      // can access the nested validator.
      if (originalValidator) {
        switch (originalValidator.type) {
          case "object": {
            const objectValidator = originalValidator.value[key];
            if (!objectValidator) {
              errors.push(
                new ConvexSchemaValidationError(
                  "ExtraProperty",
                  originalValidator,
                  key,
                  property.key.loc,
                ),
              );
            }
            this.validator = objectValidator
              ? objectValidator.fieldType
              : undefined;
            break;
          }
          case "record": {
            this.validator = originalValidator.values.fieldType;
            break;
          }
          case "any":
          case "union":
            // Do not validate any and unions at lower levels.
            this.validator = undefined;
            break;
          default: {
            originalValidator satisfies never;
          }
        }
      }

      const { value: propertyValue, errors: propertyErrors } = this.walk(
        property.value,
        false,
      );

      value[key] = propertyValue;
      errors.push(...propertyErrors);

      validatedProperties[key] = true;
    });

    this.validator = originalValidator;

    if (originalValidator?.type === "object") {
      Object.entries(validatedProperties).forEach(
        ([validatedKey, isValidated]) => {
          if (!isValidated) {
            errors.push(
              new ConvexSchemaValidationError(
                "RequiredPropertyMissing",
                originalValidator.value[validatedKey].fieldType,
                validatedKey,
                n.loc,
              ),
            );
          }
        },
      );
    }

    // Since we didn't validate unions at lower levels, let's validate them here.
    if (
      originalValidator?.type === "union" &&
      !isValidValue(originalValidator, value, false)
    ) {
      errors.push(
        new ConvexSchemaValidationError(
          "UnionMismatch",
          originalValidator,
          value,
          n.loc,
        ),
      );
    }

    return { value, errors };
  }

  unary(n: UnaryExpressionNode) {
    const errors = [];
    if (!n.prefix || n.operator !== "-") {
      errors.push(
        new ConvexValidationError(
          `Unsupported UnaryExpression: "${n.operator}"`,
          n.loc,
        ),
      );
    }

    if (n.argument.type === "Identifier" && n.argument.name === "Infinity") {
      return {
        value: -Infinity,
        errors: [],
      };
    }

    if (
      n.argument.type !== "Literal" ||
      (typeof n.argument.value !== "number" &&
        typeof n.argument.value !== "bigint")
    ) {
      errors.push(
        new ConvexValidationError(
          `"-" must be followed by a number or bigint.`,
          n.loc,
        ),
      );
    }

    const { value, errors: argErrors } = this.walk(n.argument, false);
    return {
      // I wish this conditional wasn't necessary,
      // but you can't mix numbers and bigints when performing arithmetic.
      value:
        typeof value === "bigint" ? value * BigInt(-1) : (value as number) * -1,
      errors: [...errors, ...argErrors],
    };
  }

  newExpression(n: NewExpressionNode) {
    switch (n.callee.name) {
      case "Id": {
        const error = new ConvexValidationError(
          "The `Id` class is no longer supported. Use an ID string instead.",
          n.loc,
          {
            code: {
              // @ts-expect-error -- the monaco editor types are overly strict here
              target: "https://news.convex.dev/announcing-convex-0-17-0/",
              value: "Learn more",
            },
          },
        );
        return { value: null, errors: [error] };
      }

      default:
        return {
          value: null,
          errors: [
            new ConvexValidationError(
              `Unsupported constructor: "${n.callee.name}".`,
              n.loc,
            ),
          ],
        };
    }
  }

  templateLiteral(n: TemplateLiteralNode) {
    if (n.expressions?.length) {
      return {
        value: null,
        errors: [
          new ConvexValidationError(
            `Unsupported template literal: expressions are not supported.`,
            n.loc,
          ),
        ],
      };
    }

    const value = n.quasis.map((q) => q.value.cooked).join("");
    return {
      value,
      errors:
        this.validator && !isValidValue(this.validator, value)
          ? [
              new ConvexSchemaValidationError(
                "LiteralMismatch",
                this.validator,
                value,
                n.loc,
              ),
            ]
          : [],
    };
  }

  callExpression(n: CallExpressionNode): WalkResults {
    if (n.callee.name !== "Bytes") {
      return {
        value: null,
        errors: [
          new ConvexValidationError(
            `Unsupported call expression: "${n.callee.name}".`,
            n.loc,
          ),
        ],
      };
    }

    const errors = [];
    if (this.validator && this.validator.type !== "bytes") {
      errors.push(
        new ConvexSchemaValidationError(
          "IsNotBytes",
          this.validator,
          undefined,
          n.loc,
        ),
      );
    }

    if (n.arguments.length !== 1) {
      return {
        value: null,
        errors: [
          ...errors,
          new ConvexValidationError(
            `The Bytes constructor requires exactly one argument.`,
            n.loc,
          ),
        ],
      };
    }

    // We need to temporarily unset the validator so that we don't validate the argument
    // to the call expression as a { type: "bytes" }
    const originalValidator = this.validator;
    this.validator = undefined;
    const { value, errors: walkErrors } = this.walk(n.arguments[0], false);
    this.validator = originalValidator;

    if (walkErrors.length) {
      return { value: null, errors: [...errors, ...walkErrors] };
    }
    if (typeof value !== "string") {
      return {
        value: null,
        errors: [
          ...errors,
          new ConvexValidationError(
            `The Bytes constructor requires a string argument.`,
            n.loc,
          ),
        ],
      };
    }

    try {
      const bytes = Base64.toByteArray(value);
      return {
        value: bytes.buffer as ArrayBuffer,
        errors,
      };
    } catch (e: any) {
      return {
        value: null,
        errors: [
          new ConvexValidationError(
            `The Bytes constructor requires a valid base64 encoded string: ${e.message}`,
            n.loc,
          ),
        ],
      };
    }
  }

  identifier(n: IdentifierNode) {
    if (n.name === "Infinity") {
      return {
        value: Infinity,
        errors: [],
      };
    }
    if (n.name === "NaN") {
      return {
        value: NaN,
        errors: [],
      };
    }

    return {
      value: null,
      errors: [
        new ConvexValidationError(
          `\`${n.name}\` is not a valid Convex value`,
          n.loc,
        ),
      ],
    };
  }

  walk(n: Node, isTopLevel: boolean): WalkResults {
    switch (n.type) {
      case "ArrayExpression": {
        return this.array(n);
      }

      case "ObjectExpression": {
        return this.object(n, isTopLevel);
      }

      case "UnaryExpression": {
        return this.unary(n);
      }

      case "NewExpression": {
        return this.newExpression(n);
      }

      case "TemplateLiteral": {
        return this.templateLiteral(n);
      }

      // Base case
      case "Literal": {
        if (n.regex) {
          return {
            value: null,
            errors: [
              new ConvexValidationError(`Unsupported syntax: RegExp`, n.loc),
            ],
          };
        }

        if (this.validator && !isValidValue(this.validator, n.value)) {
          return {
            value: n.value,
            errors: [
              new ConvexSchemaValidationError(
                "LiteralMismatch",
                this.validator,
                n.value,
                n.loc,
              ),
            ],
          };
        }

        return {
          value: n.value,
          errors: [],
        };
      }

      case "Identifier": {
        return this.identifier(n);
      }

      case "CallExpression": {
        return this.callExpression(n);
      }

      default: {
        return unsupportedSyntax(n);
      }
    }
  }
}

export const validateConvexFieldName = (
  fieldName: string,
  name: string,
  isTopLevel: boolean,
) => {
  if (fieldName.startsWith("$")) {
    return `${name} cannot start with a '$'`;
  }

  if (isTopLevel && fieldName.startsWith("_")) {
    return `${name} is top-level and cannot start with an underscore.`;
  }

  for (let i = 0; i < fieldName.length; i += 1) {
    const charCode = fieldName.charCodeAt(i);
    // Non-control ASCII characters
    if (charCode < 32 || charCode >= 127) {
      return `${name} must only contain non-control ASCII characters.`;
    }
  }
  return undefined;
};
