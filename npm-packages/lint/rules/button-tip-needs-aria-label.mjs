// `Button` uses a string `tip` as the accessible name of an icon-only button,
// but it can't derive a name from a JSX `tip`. Those buttons need an explicit
// `aria-label`.

function containsJsx(node) {
  if (!node || typeof node !== "object") return false;
  if (node.type === "JSXElement" || node.type === "JSXFragment") return true;
  for (const key of Object.keys(node)) {
    if (key === "parent") continue;
    const value = node[key];
    if (Array.isArray(value) ? value.some(containsJsx) : containsJsx(value)) {
      return true;
    }
  }
  return false;
}

// Mirrors `Button`, which treats `false`, `null` and `""` children as empty.
function isEmptyExpression(expression) {
  return (
    expression.type === "JSXEmptyExpression" ||
    (expression.type === "Literal" &&
      (expression.value === false ||
        expression.value === null ||
        expression.value === "")) ||
    (expression.type === "Identifier" && expression.name === "undefined")
  );
}

function isEmptyChild(child) {
  if (child.type === "JSXText") return child.value.trim() === "";
  if (child.type === "JSXExpressionContainer") {
    return isEmptyExpression(child.expression);
  }
  return false;
}

function isEmptyAttributeValue(value) {
  if (!value) return false;
  if (value.type === "Literal") return value.value === "";
  if (value.type === "JSXExpressionContainer") {
    return isEmptyExpression(value.expression);
  }
  return false;
}

/** @type {import("eslint").Rule.RuleModule} */
export default {
  meta: {
    type: "problem",
    docs: {
      description:
        "Require an aria-label on icon-only Buttons whose tip is not a string",
    },
    messages: {
      missingLabel:
        "This icon-only Button has a non-string `tip`, which can't be used as its accessible name. Add an `aria-label`.",
    },
    schema: [],
  },
  create(context) {
    return {
      JSXElement(element) {
        const opening = element.openingElement;
        if (opening.name.type !== "JSXIdentifier") return;
        if (opening.name.name !== "Button") return;

        let tip;
        for (const attr of opening.attributes) {
          // A spread may provide the label; don't guess.
          if (attr.type === "JSXSpreadAttribute") return;
          const name = attr.name.name;
          if (name === "aria-label") return;
          if (name === "children" && !isEmptyAttributeValue(attr.value)) {
            return;
          }
          if (name === "tip") tip = attr;
        }

        if (!tip?.value || tip.value.type !== "JSXExpressionContainer") return;
        if (!containsJsx(tip.value.expression)) return;
        if (!element.children.every(isEmptyChild)) return;

        context.report({ node: tip, messageId: "missingLabel" });
      },
    };
  },
};
