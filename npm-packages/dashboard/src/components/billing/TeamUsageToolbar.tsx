import { Period, UsagePeriodSelector } from "elements/UsagePeriodSelector";
import { Combobox, MAX_DISPLAYED_OPTIONS } from "@ui/Combobox";
import { Tooltip } from "@ui/Tooltip";
import { TextInput } from "@ui/TextInput";
import { useRouter } from "next/router";
import { PuzzlePieceIcon } from "@common/elements/icons";
import { usePaginatedProjects } from "api/projects";
import { ProjectDetails } from "generatedApi";
import { useState, useMemo } from "react";
import { useDebounce } from "react-use";

export function TeamUsageToolbar({
  shownBillingPeriod,
  setSelectedBillingPeriod,
  currentBillingPeriod,
  teamId,
  projectId,
  selectedProject,
}: {
  shownBillingPeriod: Period;
  teamId: number;
  projectId: number | null;
  selectedProject: ProjectDetails | null;
  setSelectedBillingPeriod: (period: Period) => void;
  currentBillingPeriod: { start: string; end: string };
}) {
  const { query, replace } = useRouter();

  const [filter, setFilter] = useState("");
  const [debouncedFilter, setDebouncedFilter] = useState("");

  // Debounce search query (300ms delay)
  useDebounce(
    () => {
      setDebouncedFilter(filter);
    },
    300,
    [filter],
  );

  const paginatedProjects = usePaginatedProjects(teamId, {
    q: debouncedFilter,
    limitOverride: MAX_DISPLAYED_OPTIONS,
  });

  const projects = paginatedProjects ? paginatedProjects.items : undefined;

  // Detect duplicate project names and prepare options
  const { projectOptions, slugByProjectId } = useMemo(() => {
    const nameCountMap = new Map<string, number>();
    projects?.forEach((p) => {
      nameCountMap.set(p.name, (nameCountMap.get(p.name) || 0) + 1);
    });

    const slugMap = new Map<number, string>();
    const options = [
      { label: "All Projects", value: null as number | null },
      ...(projects?.map((p) => {
        const isDuplicate = (nameCountMap.get(p.name) || 0) > 1;
        if (isDuplicate && p.slug) {
          slugMap.set(p.id, p.slug);
        }
        const label = isDuplicate && p.slug ? `${p.name} (${p.slug})` : p.name;
        return { label, value: p.id as number | null };
      }) ?? []),
    ];

    // Ensure the selected project is always an option, even when it's filtered
    // out of the paginated list (or not in the first page). Otherwise the
    // button label falls back to "Select a project" and blurring the combobox
    // deselects it.
    if (
      selectedProject &&
      !options.some((o) => o.value === selectedProject.id)
    ) {
      options.push({
        label: selectedProject.name,
        value: selectedProject.id,
      });
    }

    return { projectOptions: options, slugByProjectId: slugMap };
  }, [projects, selectedProject]);

  return (
    <div className="sticky top-0 z-20 mb-2 flex h-(--team-usage-toolbar-height) flex-wrap content-center items-center gap-2 border-b bg-background-primary">
      <UsagePeriodSelector
        period={shownBillingPeriod}
        onChange={setSelectedBillingPeriod}
        currentBillingPeriod={currentBillingPeriod}
      />
      <Combobox
        label="Projects"
        options={projectOptions}
        selectedOption={projectId}
        onFilterChange={setFilter}
        isLoadingOptions={
          !!paginatedProjects?.isLoading && debouncedFilter === filter
        }
        innerButtonClasses={
          projectId
            ? "bg-yellow-100/50 dark:bg-yellow-600/20 hover:bg-yellow-100 dark:hover:bg-yellow-600/50"
            : ""
        }
        setSelectedOption={(o) => {
          const newProject =
            projects?.find((p) => p.id === o) ??
            (selectedProject?.id === o ? selectedProject : undefined);
          query.projectSlug = newProject?.slug ?? o?.toString();
          void replace({ query }, undefined, { shallow: true });
        }}
        unknownLabel={() => "projects"}
        Option={({ label, value }) => {
          const slug = value !== null ? slugByProjectId.get(value) : undefined;
          if (slug) {
            const name = label.replace(` (${slug})`, "");
            return (
              <span>
                {name} <span className="text-content-secondary">({slug})</span>
              </span>
            );
          }
          return <span>{label}</span>;
        }}
      />

      <Tooltip
        className="w-48 animate-fadeInFromLoading"
        tip={
          <div className="flex flex-col gap-1">
            <p>
              Filter usage to only include components whose paths start with
              this string.
            </p>
            <p>Enter "app" to only see results for the root app.</p>
          </div>
        }
      >
        <TextInput
          label="Component Prefix"
          labelHidden
          id="componentPrefix"
          type="search"
          SearchIcon={PuzzlePieceIcon}
          placeholder="Component Prefix"
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.currentTarget.blur();
            }
          }}
          onBlur={(e) => {
            const { value } = e.target;
            query.componentPrefix = value;
            void replace({ query }, undefined, { shallow: true });
          }}
        />
      </Tooltip>

      <span className="text-sm text-content-secondary lg:ml-auto">
        All dates are in UTC
      </span>
    </div>
  );
}
