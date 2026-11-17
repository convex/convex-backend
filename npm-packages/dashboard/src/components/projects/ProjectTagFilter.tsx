import { Combobox } from "@ui/Combobox";
import { toast } from "@common/lib/utils";
import { useTeamProjectTags } from "api/projects";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { validateProjectTag } from "./projectTags";

const ALL_TAGS = { label: "All tags", value: "all" };

function TagOption({ label, inButton }: { label: string; inButton: boolean }) {
  if (inButton && label !== ALL_TAGS.label) {
    return (
      <span>
        <span className="font-semibold">Tag:</span> {label}
      </span>
    );
  }
  return <span>{label}</span>;
}

export function ProjectTagFilter({
  teamId,
  canListTags,
  selectedTag,
  onChangeTag,
}: {
  teamId: number;
  canListTags: boolean | undefined;
  selectedTag: string | undefined;
  onChangeTag: (tag: string | undefined) => void;
}) {
  // Lists the first page of the team's tags; any other tag can be typed in.
  const tags = useTeamProjectTags(canListTags ? teamId : undefined);
  const disabledTip =
    canListTags === false
      ? permissionDeniedTip(
          "You do not have permission to view this team's project tags.",
          "team:projectTags:view",
        )
      : tags?.length === 0
        ? "None of this team's projects have tags yet."
        : undefined;

  // A tag typed in, or taken from the URL, may not be on the listed page.
  const listed =
    selectedTag === undefined || tags?.includes(selectedTag)
      ? (tags ?? [])
      : [selectedTag, ...(tags ?? [])];

  return (
    <Combobox
      label="Filter by tag"
      labelHidden
      options={[ALL_TAGS, ...listed.map((tag) => ({ label: tag, value: tag }))]}
      Option={TagOption}
      selectedOption={selectedTag ?? ALL_TAGS.value}
      setSelectedOption={(value) => {
        const tag = value?.trim();
        if (!tag || tag === ALL_TAGS.value) {
          onChangeTag(undefined);
          return;
        }
        const error = validateProjectTag(tag, []);
        if (error) {
          toast("error", error, "project-tag-filter-invalid");
        } else {
          onChangeTag(tag);
        }
      }}
      allowCustomValue
      customValueLabel={(tag) => `"${tag}"`}
      searchPlaceholder="Search tags..."
      disabled={disabledTip !== undefined}
      buttonProps={{ tip: disabledTip }}
      buttonClasses="w-fit"
      innerButtonClasses={
        selectedTag !== undefined
          ? "bg-yellow-100/50 dark:bg-yellow-600/20 hover:bg-yellow-100 dark:hover:bg-yellow-600/50"
          : ""
      }
      optionsWidth="fit"
    />
  );
}
