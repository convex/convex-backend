import { CopyIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Sheet } from "@ui/Sheet";
import { TextInput } from "@ui/TextInput";
import { useFormik } from "formik";
import { TeamResponse } from "generatedApi";
import * as Yup from "yup";
import { useCopy } from "@common/lib/useCopy";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { TEAM_SETTINGS_SECTIONS } from "lib/sectionAnchors";

export type TeamSlugFormProps = {
  team: TeamResponse;
  onUpdateTeam: (body: { slug: string }) => Promise<void>;
  canUpdate: boolean;
};

const TeamSlugSchema = Yup.object().shape({
  slug: Yup.string()
    .min(3, "Team slug must be at least 3 characters long.")
    .max(64, "Team slug must be at most 64 characters long.")
    .matches(
      /^[\w-]+$/,
      "Team slug may contain numbers, letters, underscores, and '-'.",
    )
    .required(),
});

export function TeamSlugForm({
  team,
  onUpdateTeam,
  canUpdate,
}: TeamSlugFormProps) {
  const formState = useFormik({
    initialValues: {
      slug: team.slug,
    },
    enableReinitialize: true,
    validationSchema: TeamSlugSchema,
    onSubmit: async (values) => {
      await onUpdateTeam({ slug: values.slug });
      // Completely reload the page to avoid race conditions
      // with the slug of the team being updated (it's part of the URL).
      window.location.href = `/t/${values.slug}/settings`;
    },
  });

  const copyToClipboard = useCopy("Team slug");

  return (
    <Sheet id={TEAM_SETTINGS_SECTIONS.teamSlug.id} className="text-sm">
      <h3 className="mb-1">Team Slug</h3>
      <p className="mb-4 max-w-prose text-content-secondary">
        The unique identifier for your team in dashboard URLs.
      </p>
      <form
        onSubmit={formState.handleSubmit}
        aria-label="Edit team slug"
        className="flex items-start gap-2"
      >
        <div className="max-w-[20rem] flex-1">
          <Tooltip
            className="block w-full"
            tip={
              !canUpdate
                ? permissionDeniedTip(
                    "You do not have permission to update the team slug.",
                    "team:update",
                  )
                : undefined
            }
          >
            <TextInput
              label="Team Slug"
              labelHidden
              // Cap the input at the wrapper width: the copy icon's negative
              // margin otherwise makes the input overgrow by a few px, eating
              // into the gap before the Save button.
              className="max-w-full"
              placeholder="Enter a slug for your team"
              onChange={formState.handleChange}
              value={formState.values.slug}
              // We hide the button when the tooltip is visible to avoid nesting buttons
              Icon={canUpdate ? CopyIcon : undefined}
              iconTooltip="Copy team slug"
              action={() => copyToClipboard(formState.values.slug)}
              id="slug"
              error={formState.errors.slug}
              disabled={!canUpdate}
            />
          </Tooltip>
        </div>

        <Button
          disabled={
            !formState.dirty ||
            formState.isSubmitting ||
            !formState.isValid ||
            !canUpdate
          }
          type="submit"
          aria-label="Save team slug"
        >
          Save
        </Button>
      </form>
    </Sheet>
  );
}
