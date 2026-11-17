import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Sheet } from "@ui/Sheet";
import { TextInput } from "@ui/TextInput";
import { useFormik } from "formik";
import { TeamResponse } from "generatedApi";
import * as Yup from "yup";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { TEAM_SETTINGS_SECTIONS } from "lib/sectionAnchors";

export type TeamNameFormProps = {
  team: TeamResponse;
  onUpdateTeam: (body: { name: string }) => Promise<void>;
  canUpdate: boolean;
};

const TeamNameSchema = Yup.object().shape({
  name: Yup.string()
    .min(3, "Team name must be at least 3 characters long.")
    .max(128, "Team name must be at most 128 characters long.")
    .required("Team name is required."),
});

export function TeamNameForm({
  team,
  onUpdateTeam,
  canUpdate,
}: TeamNameFormProps) {
  const formState = useFormik({
    initialValues: {
      name: team.name,
    },
    enableReinitialize: true,
    validationSchema: TeamNameSchema,
    onSubmit: async (values) => {
      await onUpdateTeam({ name: values.name });
    },
  });

  return (
    <Sheet id={TEAM_SETTINGS_SECTIONS.teamName.id} className="text-sm">
      <h3 className="mb-4">Team Name</h3>
      <form
        onSubmit={formState.handleSubmit}
        aria-label="Edit team name"
        className="flex items-start gap-2"
      >
        <div className="max-w-[20rem] flex-1">
          <Tooltip
            className="block w-full"
            tip={
              !canUpdate
                ? permissionDeniedTip(
                    "You do not have permission to update the team name.",
                    "team:update",
                  )
                : undefined
            }
          >
            <TextInput
              label="Team Name"
              labelHidden
              placeholder="Enter a name for your team"
              onChange={formState.handleChange}
              value={formState.values.name}
              id="name"
              error={formState.errors.name}
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
          aria-label="Save team name"
        >
          Save
        </Button>
      </form>
    </Sheet>
  );
}
