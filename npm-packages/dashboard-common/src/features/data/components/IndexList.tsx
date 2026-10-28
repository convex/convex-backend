import groupBy from "lodash/groupBy";
import { Link } from "@ui/Link";
import {
  MagnifyingGlassIcon,
  QuestionMarkCircledIcon,
  ArrowTopRightIcon,
} from "@radix-ui/react-icons";
import { FingerPrintIcon } from "@heroicons/react/24/outline";
import { Index } from "@common/features/data/lib/api";
import { useNents } from "@common/lib/useNents";
import { Loading } from "@ui/Loading";
import { Spinner } from "@ui/Spinner";
import { useQuery } from "convex/react";
import { api } from "system-udfs/convex/_generated/api";
import { Fragment } from "react";
import { ProgressBarWithPercent } from "@ui/ProgressBar";
import { Tooltip } from "@ui/Tooltip";
import { HelpTooltip } from "@ui/HelpTooltip";
import { cn } from "@ui/cn";
import { Callout } from "@ui/Callout";

export function IndexList({ tableName }: { tableName: string }) {
  const { selectedNent } = useNents();
  const indexes =
    useQuery(api._system.frontend.indexes.default, {
      tableName,
      tableNamespace: selectedNent?.id ?? null,
    }) ?? undefined;

  return <IndexesList tableName={tableName} indexes={indexes} />;
}

export function IndexesList({
  tableName,
  indexes,
}: {
  tableName: string;
  indexes: Index[] | undefined;
}) {
  if (indexes === undefined) {
    return <Loading />;
  }

  const recommendStagedIndexes = (indexes ?? []).some(
    (index) =>
      index.backfill.state === "backfilling" &&
      !index.staged &&
      index.backfill.stats?.totalDocs &&
      index.backfill.stats.totalDocs > 10_000,
  );

  const groupedIndexes = groupBy(indexes, getIndexType);

  return (
    <div className="flex flex-col gap-6">
      {recommendStagedIndexes && (
        <Callout variant="hint">
          <p>
            <strong className="font-semibold">Hint</strong>: When adding an
            index to a large table, consider using a{" "}
            <Link
              href="https://docs.convex.dev/database/reading-data/indexes/#staged-indexes"
              target="_blank"
              rel="noreferrer"
            >
              staged index
            </Link>{" "}
            to avoid blocking deploy.
          </p>
        </Callout>
      )}
      <div className="flex flex-col gap-10">
        <IndexListSection
          title="Indexes"
          description="Indexes allow you to speed up your document queries by telling Convex how to organize your documents."
          learnMoreUrl="https://docs.convex.dev/database/reading-data/indexes/"
          indexes={groupedIndexes.database ?? []}
          icon={FingerPrintIcon}
          tableName={tableName}
        />
        <IndexListSection
          title="Search indexes"
          description="Search indexes allows you to find Convex documents that approximately match a textual search query."
          learnMoreUrl="https://docs.convex.dev/search/text-search"
          indexes={groupedIndexes.search ?? []}
          icon={MagnifyingGlassIcon}
          tableName={tableName}
        />
        <IndexListSection
          title="Vector indexes"
          description="Vector search allows you to find Convex documents similar to a provided vector."
          learnMoreUrl="https://docs.convex.dev/search/vector-search"
          indexes={groupedIndexes.vector ?? []}
          icon={ArrowTopRightIcon}
          tableName={tableName}
        />
      </div>
    </div>
  );
}

function IndexListSection({
  title,
  description,
  learnMoreUrl,
  indexes,
  icon: Icon,
  tableName,
}: {
  title: string;
  description: string;
  learnMoreUrl: string;
  indexes: Index[];
  icon: React.FC<{ className?: string }>;
  tableName: string;
}) {
  const indexesByName = groupBy(indexes, "name");

  return (
    <section aria-label={title} className="flex flex-col gap-3">
      <header className="flex items-center gap-1.5 text-content-primary">
        <Icon className="size-5 text-content-secondary" />
        <h5 className="text-base font-medium">{title}</h5>
        <HelpTooltip>
          <p>
            {description}{" "}
            <Link href={learnMoreUrl} target="_blank" rel="noopener noreferrer">
              Learn more
            </Link>
          </p>
        </HelpTooltip>
      </header>
      {indexes.length === 0 ? (
        <div className="text-sm text-content-tertiary">
          <code>{tableName}</code> has no {title.toLowerCase()}.
        </div>
      ) : (
        <div className="flex flex-col gap-5">
          {indexes.map((index) => (
            <IndexListRow
              key={`${index.name} ${indexesByName[index.name].indexOf(index)}`}
              index={index}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function IndexListRow({ index }: { index: Index }) {
  const { fields } = index;
  const isStaged = index.staged === true;

  return (
    <article className="flex flex-col gap-2 text-sm text-content-secondary">
      <header className="flex items-center gap-1">
        <div className="flex items-baseline gap-1">
          <h6 className="truncate font-mono text-sm font-medium text-content-primary">
            {index.name}
          </h6>

          {isStaged && (
            <Tooltip
              className="ml-1 flex gap-1"
              tip={
                <div className="text-sm">
                  <p className="mb-2">
                    Staged indexes are not queryable. To enable this index,
                    remove{" "}
                    <code className="whitespace-nowrap">
                      {"{ staged: true }"}
                    </code>{" "}
                    in your <code>schema.ts</code> file.
                  </p>
                </div>
              }
            >
              <span className="text-xs text-content-tertiary underline decoration-dotted">
                Staged
              </span>
              <QuestionMarkCircledIcon className="text-content-tertiary" />
            </Tooltip>
          )}
        </div>
        <div
          className={cn("ml-1 grow border-b", isStaged && "border-dashed")}
        />
      </header>

      <div className="flex flex-col gap-1 pl-2">
        {Array.isArray(fields) && (
          <IndexAttribute title="Fields">
            <FieldList fields={fields} />
          </IndexAttribute>
        )}

        {"searchField" in fields && (
          <IndexAttribute title="Search field">
            <code>{fields.searchField}</code>
          </IndexAttribute>
        )}

        {"vectorField" in fields && (
          <IndexAttribute title="Vector field">
            <code>{fields.vectorField}</code>
          </IndexAttribute>
        )}

        {"filterFields" in fields && fields.filterFields.length > 0 && (
          <IndexAttribute title="Filter fields">
            <FieldList fields={fields.filterFields} />
          </IndexAttribute>
        )}

        {"dimensions" in fields && (
          <IndexAttribute title="Dimensions">
            {fields.dimensions}
          </IndexAttribute>
        )}
      </div>

      {index.backfill.state === "backfilling" && (
        <div className="flex flex-col gap-1 pl-2">
          <div className="flex items-center gap-2">
            {!(
              index.backfill.stats && index.backfill.stats.totalDocs !== null
            ) && (
              <div>
                <Spinner />
              </div>
            )}
            Backfill in progress
          </div>
          {index.backfill.stats && index.backfill.stats.totalDocs !== null && (
            <ProgressBarWithPercent
              fraction={Math.min(
                // numDocsIndexed is an estimate and can grow larger than totalDocs
                // (in particular because if new documents are added during the backfill,
                // totalDocs is not updated), so we cap at 99% to not confuse the user
                0.99,
                index.backfill.stats.numDocsIndexed /
                  index.backfill.stats.totalDocs,
              )}
              variant="stripes"
              ariaLabel="Index backfill progress"
            />
          )}
        </div>
      )}
      {index.backfill.state === "backfilled" && (
        <div className="flex flex-col gap-1 pl-2">
          Backfill completed
          <ProgressBarWithPercent
            fraction={1}
            variant="solid"
            ariaLabel="Index backfill progress"
          />
        </div>
      )}
    </article>
  );
}

function IndexAttribute({
  title,
  children,
}: React.PropsWithChildren<{ title: string }>) {
  return (
    <div className="flex gap-1">
      <span>
        <strong className="font-medium text-content-primary">{title}</strong>:
      </span>
      <div>{children}</div>
    </div>
  );
}

function FieldList({ fields }: { fields: string[] }) {
  return (
    <>
      {fields.map((field) => (
        <Fragment key={field}>
          <code>{field}</code>
          {fields.length > 1 && fields.indexOf(field) < fields.length - 1 && (
            <span>, </span>
          )}
        </Fragment>
      ))}
    </>
  );
}

function getIndexType(
  index: Index,
): "database" | "search" | "vector" | "unknown" {
  if (Array.isArray(index.fields)) {
    return "database";
  }

  if ("searchField" in index.fields) {
    return "search";
  }

  if ("vectorField" in index.fields) {
    return "vector";
  }

  index.fields satisfies never;
  return "unknown";
}
