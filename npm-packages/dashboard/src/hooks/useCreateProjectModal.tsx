import { Modal } from "@ui/Modal";
import { TextInput } from "@ui/TextInput";
import { Button } from "@ui/Button";
import { Loading } from "@ui/Loading";

import { useFormik } from "formik";
import * as Yup from "yup";
import { createGlobalState } from "react-use";
import { TeamResponse } from "generatedApi";
import type { PlatformCreateProjectResponse } from "@convex-dev/platform/managementApi";
import { useCurrentTeam } from "api/teams";
import { useCreateProject } from "api/projects";
import { cn } from "@ui/cn";
import { usePostHog } from "hooks/usePostHog";
import { ProjectTagsInput } from "components/projects/ProjectTagsInput";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { useProjectTagSuggestions } from "hooks/useProjectTagSuggestions";

export const useCreateProjectModalRequest = createGlobalState<{
  team?: TeamResponse;
} | null>(null);

export function CreateProjectModal() {
  const [request, setRequest] = useCreateProjectModalRequest();
  const currentTeam = useCurrentTeam();
  const { tagEditUi } = useLaunchDarkly();

  if (!request) {
    return null;
  }

  const close = () => setRequest(null);
  const selectedTeam = request.team || currentTeam;

  return (
    <Modal title="Create Project" onClose={close}>
      <>
        {selectedTeam && (
          <p className="mb-5">
            Create a project in{" "}
            <span className="font-semibold">{selectedTeam?.name}</span>.
          </p>
        )}
        {selectedTeam ? (
          <CreateProjectForm
            onClose={close}
            team={selectedTeam}
            showLabel
            showTags={tagEditUi}
            onSuccess={(project) => {
              const projectUrl = `/t/${selectedTeam.slug}/${project.slug}/development`;
              window.location.href = projectUrl;
            }}
          />
        ) : (
          <Loading />
        )}
      </>
    </Modal>
  );
}

export const ProjectNameSchema = Yup.string()
  .min(3, "Project name must be at least 3 characters long.")
  .max(128, "Project name must be at most 128 characters long.")
  .required("Project name is required.");
const CreateProjectSchema = Yup.object().shape({
  projectName: ProjectNameSchema,
});

export function CreateProjectForm({
  onClose,
  team,
  showLabel = false,
  showTags = false,
  onSuccess,
}: {
  onClose(): void;
  team: TeamResponse;
  showLabel?: boolean;
  showTags?: boolean;
  onSuccess: (project: PlatformCreateProjectResponse) => void;
}) {
  const createProject = useCreateProject(team.id);
  const tagSuggestions = useProjectTagSuggestions(
    showTags ? team.id : undefined,
  );
  const { capture } = usePostHog();
  const formState = useFormik({
    initialValues: {
      projectName: "",
      tags: [] as string[],
    },
    validateOnChange: false,
    validateOnBlur: true,
    validationSchema: CreateProjectSchema,
    onSubmit: async (values) => {
      const project = await createProject({
        ...values,
        deploymentType: null,
      });
      capture("created_project");
      onSuccess(project);
      onClose();
    },
  });
  return (
    <form
      onSubmit={formState.handleSubmit}
      aria-label="Create project"
      className={showTags ? "flex flex-col gap-4" : "flex gap-2"}
    >
      <TextInput
        labelHidden={!showLabel}
        label="Project Name"
        outerClassname="w-full"
        placeholder="Project name"
        onChange={(e) => {
          // Reset the errors so the user can blur the form
          formState.setErrors({});
          formState.handleChange(e);
        }}
        onBlur={formState.handleBlur}
        value={formState.values.projectName}
        autoFocus
        id="projectName"
        error={formState.touched ? formState.errors.projectName : undefined}
      />

      {showTags && (
        <ProjectTagsInput
          tags={formState.values.tags}
          onChange={(tags) => void formState.setFieldValue("tags", tags)}
          suggestions={tagSuggestions}
          description="Optionally, add tags to organize this project."
        />
      )}

      <Button
        disabled={!formState.dirty || !formState.isValid}
        className={cn("h-fit", showTags ? "self-end" : showLabel && "mt-6")}
        size="sm"
        type="submit"
        aria-label="submit"
        loading={formState.isSubmitting}
      >
        {formState.isSubmitting ? "Creating" : "Create"}
      </Button>
    </form>
  );
}
