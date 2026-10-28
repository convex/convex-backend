import { Fragment, useMemo } from "react";
import { Shape } from "shapes";
import {
  TabList as HeadlessTabList,
  TabPanel as HeadlessTabPanel,
  TabPanels as HeadlessTabPanels,
  TabGroup as HeadlessTabGroup,
} from "@headlessui/react";
import { Tab } from "@ui/Tab";
import { ConvexSchemaFilePath } from "@common/features/data/components/ConvexSchemaFilePath";
import {
  GenerateSchema,
  CodeTransformation,
  LineHighlighter,
} from "@common/features/data/components/GenerateSchema";
import { SchemaJson, displaySchema } from "@common/lib/format";
import { ReadonlyCode } from "@common/elements/ReadonlyCode";
import { Spinner } from "@ui/Spinner";
import { ProgressBarWithPercent } from "@ui/ProgressBar";

export function ShowSchema({
  activeSchema,
  inProgressSchema,
  shapes,
  hasShapeError = false,
  lineHighlighter = undefined,
  codeTransformation = (code) => code,
  showLearnMoreLink = true,
  schemaValidationProgress = undefined,
}: {
  activeSchema: SchemaJson | null | undefined;
  inProgressSchema: SchemaJson | null | undefined;
  shapes: Map<string, Shape>;
  hasShapeError?: boolean;
  lineHighlighter?: LineHighlighter;
  codeTransformation?: CodeTransformation;
  showLearnMoreLink?: boolean;
  schemaValidationProgress?: {
    numDocsValidated: number;
    totalDocs: number | null;
  } | null;
}) {
  const displayedSchema = useMemo(() => {
    if (!activeSchema) {
      return "";
    }
    const schema = displaySchema(activeSchema);
    return schema ? codeTransformation(schema) : "";
  }, [activeSchema, codeTransformation]);

  const noSavedSchema = !activeSchema && !inProgressSchema;
  return (
    <div className="max-w-full">
      <HeadlessTabGroup as={Fragment}>
        <HeadlessTabList className="flex gap-1">
          <Tab
            disabled={noSavedSchema}
            tip={
              noSavedSchema
                ? "Your project doesn’t have a saved schema yet. Edit convex/schema.ts to add one."
                : undefined
            }
          >
            Saved
          </Tab>
          <Tab>Generated</Tab>
        </HeadlessTabList>
        <HeadlessTabPanels className="p-3">
          <HeadlessTabPanel>
            {activeSchema && (
              <>
                <p className="mb-2">
                  This is a representation of the schema that is{" "}
                  {activeSchema?.schemaValidation
                    ? "currently being enforced"
                    : "saved"}
                  . It is equivalent to your <ConvexSchemaFilePath />.
                </p>
                <div className="block rounded-sm border p-4 text-sm wrap-break-word whitespace-pre-wrap">
                  <ReadonlyCode
                    disableLineNumbers
                    path="generateSchema"
                    highlightLines={
                      lineHighlighter
                        ? lineHighlighter(displayedSchema)
                        : undefined
                    }
                    code={displayedSchema}
                    language="javascript"
                    height={{ type: "content", maxHeightRem: 52 }}
                  />
                </div>
              </>
            )}

            {inProgressSchema && (
              <div className="mt-4 flex items-center gap-1 text-sm text-content-secondary">
                {!schemaValidationProgress && (
                  <div>
                    <Spinner />
                  </div>
                )}
                {schemaValidationProgress
                  ? "Schema validation in progress..."
                  : "Code push in progress..."}
                {schemaValidationProgress &&
                  schemaValidationProgress.totalDocs !== null && (
                    <div className="grow sm:px-6">
                      <ProgressBarWithPercent
                        // totalDocs is not necessarily taken from the same snapshot as the snapshot being iterated over to increment numDocsValidated
                        // so we cap the progress at 99%
                        fraction={Math.min(
                          0.99,
                          schemaValidationProgress.numDocsValidated /
                            schemaValidationProgress.totalDocs,
                        )}
                        variant="stripes"
                        ariaLabel="Schema validation progress"
                      />
                    </div>
                  )}
              </div>
            )}
          </HeadlessTabPanel>
          <HeadlessTabPanel>
            <GenerateSchema
              shapes={shapes}
              hadError={hasShapeError}
              showUsageInstructions={noSavedSchema}
              lineHighlighter={lineHighlighter}
              codeTransformation={codeTransformation}
              showLearnMoreLink={showLearnMoreLink}
            />
          </HeadlessTabPanel>
        </HeadlessTabPanels>
      </HeadlessTabGroup>
    </div>
  );
}
