import {
  QuestionMarkCircledIcon,
  CrossCircledIcon,
} from "@radix-ui/react-icons";

export function UsageNoDataError() {
  return (
    <TeamUsageError
      title="No usage"
      description="No usage for this metric could be found for the period shown."
    />
  );
}

export function UsageDataError({ entity = "usage" }: { entity?: string }) {
  const title = `Error fetching ${entity} data`;
  const description = `An error occurred while fetching ${entity.toLowerCase()} data. Please try again later.`;

  return (
    <div className="flex h-56 animate-fadeInFromLoading flex-col justify-center p-2 text-center">
      <CrossCircledIcon className="mx-auto size-6 text-content-error" />
      <h4 className="mt-2">{title}</h4>
      <p className="mt-1 text-sm text-content-secondary">{description}</p>
    </div>
  );
}

export function UsageChartUnavailable() {
  return (
    <TeamUsageError
      title="Chart not available"
      description="This chart isn't available when filtering on a particular project."
    />
  );
}

export function TeamUsageError({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <div className="flex h-56 animate-fadeInFromLoading flex-col justify-center p-2 text-center">
      <QuestionMarkCircledIcon className="mx-auto size-6 text-content-tertiary" />
      <h4 className="mt-2">{title}</h4>
      <p className="mt-1 text-sm text-content-secondary">{description}</p>
    </div>
  );
}
