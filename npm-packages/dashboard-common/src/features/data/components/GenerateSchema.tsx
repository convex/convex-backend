import { Link } from "@ui/Link";
import React, { useMemo } from "react";
import { Shape } from "shapes";
import { ConvexSchemaFilePath } from "@common/features/data/components/ConvexSchemaFilePath";
import { HighlightLines, ReadonlyCode } from "@common/elements/ReadonlyCode";
import { displaySchemaFromShapes } from "@common/lib/format";
import { Callout } from "@ui/Callout";
import { CopyButton } from "@common/elements/CopyButton";

export type LineHighlighter = (code: string) => HighlightLines;
export type CodeTransformation = (code: string) => string;

export function GenerateSchema({
  shapes,
  hadError,
  lineHighlighter = undefined,
  codeTransformation = (code) => code,
  showLearnMoreLink = true,
  showUsageInstructions = true,
}: {
  shapes: Map<string, Shape>;
  hadError: boolean;
  // Determines which lines to highlight in the UI. A number highlights a
  // single line. HightLines higlights a range.  The code provided to this
  // function will have already been mutated by codeTransformation if one is
  // provided.
  lineHighlighter?: LineHighlighter;
  // Optionally transforms the code block this function produces from the given
  // shapes.
  codeTransformation?: CodeTransformation;
  showLearnMoreLink?: boolean;
  showUsageInstructions?: boolean;
}) {
  const displayedSchema = useMemo(() => {
    const schema = displaySchemaFromShapes(shapes);
    return schema ? codeTransformation(schema) : "";
  }, [codeTransformation, shapes]);

  const widthString = displayedSchema ? "max-w-full" : "w-[32rem]";
  const learnMoreLink = (
    <Link
      href="https://docs.convex.dev/database/schemas"
      target="_blank"
      externalIcon
    >
      Schema docs
    </Link>
  );
  return (
    <div className={widthString}>
      {hadError && (
        <Callout className="mb-2" variant="error">
          Encountered an error generating the table schema.
        </Callout>
      )}
      <div className="mb-2">
        {displayedSchema ? (
          <div className="flex flex-col gap-2">
            <div>
              We've generated an approximate schema based on the data available
              in your table
              {shapes.size > 1 ? "s" : ""}. {showLearnMoreLink && learnMoreLink}
            </div>
            {showUsageInstructions && (
              <>
                <div>
                  Paste this schema into the{" "}
                  <ConvexSchemaFilePath className="text-xs" /> file in your
                  codebase.{" "}
                </div>
                <div>
                  Modify the field types if the generated schema doesn’t fit
                  your data model.
                </div>
              </>
            )}
          </div>
        ) : (
          <div>
            Your project doesn't have any data yet. Once your tables have data,
            Convex will be able to suggest a schema for your project.
            {showLearnMoreLink && learnMoreLink}
          </div>
        )}
      </div>
      {displayedSchema && (
        <div
          className="relative block rounded-sm border p-4 text-sm wrap-break-word whitespace-pre-wrap"
          aria-hidden="true"
        >
          <ReadonlyCode
            path="generateSchema"
            highlightLines={
              lineHighlighter ? lineHighlighter(displayedSchema) : undefined
            }
            code={displayedSchema}
            language="javascript"
            disableLineNumbers
            height={{ type: "content", maxHeightRem: 52 }}
          />
          <div className="absolute top-0 right-0 h-10">
            <CopyButton text={displayedSchema} />
          </div>
        </div>
      )}
    </div>
  );
}
