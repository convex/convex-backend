import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Sheet } from "@ui/Sheet";
import { TextInput } from "@ui/TextInput";
import { useCopy } from "@common/lib/useCopy";
import { CopyIcon } from "@radix-ui/react-icons";
import { useFormik } from "formik";
import { useUpdateProject } from "api/projects";
import { ProjectDetails, TeamResponse } from "generatedApi";
import * as Yup from "yup";

const TeamSchema = Yup.object().shape({
  name: Yup.string()
    .min(3, "Project name must be at least 3 characters long.")
    .max(128, "Project name must be at most 128 characters long.")
    .required("Project name is required."),
  slug: Yup.string()
    .min(3, "Project slug must be at least 3 characters long.")
    .max(64, "Project slug must be at most 64 characters long.")
    .matches(
      /^[\w-]+$/,
      "Project slug may contain numbers, letters, underscores, and '-'.",
    )
    .required(),
});

export function ProjectForm({
  project,
  team,
  hasAdminPermissions,
  permissionDeniedTip,
}: {
  team: TeamResponse;
  project: ProjectDetails;
  hasAdminPermissions: boolean;
  permissionDeniedTip?: React.ReactNode;
}) {
  const updateProject = useUpdateProject(project.id);
  const formState = useFormik({
    initialValues: {
      name: project.name,
      slug: project.slug,
    },
    enableReinitialize: true,
    validationSchema: TeamSchema,
    onSubmit: async (values) => {
      await updateProject(values);
      // Completely reload the page to avoid race conditions
      // with the slug of the project being updated.
      window.location.href = `/t/${team.slug}/${values.slug}/settings`;
    },
  });

  const copyToClipboard = useCopy("Project slug");

  return (
    <Sheet className="text-sm">
      <h3 className="mb-4">Edit Project</h3>
      <form
        onSubmit={formState.handleSubmit}
        aria-label="Edit project settings"
      >
        <div className="flex flex-col gap-6">
          <div className="flex max-w-xs flex-col gap-4">
            <Tooltip
              tip={!hasAdminPermissions ? permissionDeniedTip : undefined}
            >
              <TextInput
                label="Project Name"
                outerClassname="max-w-[20rem]"
                placeholder="Enter a name for your project"
                onChange={formState.handleChange}
                value={formState.values.name}
                id="name"
                error={formState.errors.name}
                disabled={!hasAdminPermissions}
              />
            </Tooltip>

            <Tooltip
              tip={!hasAdminPermissions ? permissionDeniedTip : undefined}
            >
              <TextInput
                label="Project Slug"
                outerClassname="max-w-[20rem]"
                placeholder="Enter a slug for your project"
                description="Changing the project slug will require you to update any deploy keys currently in use."
                onChange={formState.handleChange}
                value={formState.values.slug}
                Icon={CopyIcon}
                action={() => copyToClipboard(formState.values.slug)}
                id="slug"
                error={formState.errors.slug}
                disabled={!hasAdminPermissions}
              />
            </Tooltip>
          </div>

          <div className="flex justify-end">
            <Button
              disabled={
                !formState.dirty || formState.isSubmitting || !formState.isValid
              }
              type="submit"
              aria-label="submit"
            >
              Save
            </Button>
          </div>
        </div>
      </form>
    </Sheet>
  );
}
