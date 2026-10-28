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

export const useCreateProjectModalRequest = createGlobalState<{
  team?: TeamResponse;
} | null>(null);

export function CreateProjectModal() {
  const [request, setRequest] = useCreateProjectModalRequest();
  const currentTeam = useCurrentTeam();

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
  onSuccess,
}: {
  onClose(): void;
  team: TeamResponse;
  showLabel?: boolean;
  onSuccess: (project: PlatformCreateProjectResponse) => void;
}) {
  const createProject = useCreateProject(team.id);
  const { capture } = usePostHog();
  const formState = useFormik({
    initialValues: {
      projectName: "",
    },
    validateOnChange: false,
    validateOnBlur: true,
    validationSchema: CreateProjectSchema,
    onSubmit: async (values: { projectName: string }) => {
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
      className="flex gap-2"
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

      <Button
        disabled={!formState.dirty || !formState.isValid}
        className={cn("h-fit", showLabel ? "mt-6" : "")}
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
