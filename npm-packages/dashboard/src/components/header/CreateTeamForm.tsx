import { Button } from "@ui/Button";
import { TextInput } from "@ui/TextInput";
import { useCreateTeam } from "api/teams";
import { useFormik } from "formik";
import { useRouter } from "next/router";
import * as Yup from "yup";

const CreateTeamSchema = Yup.object().shape({
  name: Yup.string()
    .min(3, "Team name must be at least 3 characters long.")
    .max(128, "Team name must be at most 128 characters long.")
    .required("Team name is required."),
});

export function CreateTeamForm({ onClose }: { onClose(): void }) {
  const createTeam = useCreateTeam();
  const router = useRouter();
  const formState = useFormik({
    initialValues: {
      name: "",
    },
    validationSchema: CreateTeamSchema,
    onSubmit: async (values: { name: string }) => {
      const team = await createTeam(values);
      onClose();
      await router.push(`/t/${team.slug}/settings`);
    },
  });
  return (
    <form
      onSubmit={formState.handleSubmit}
      aria-label="Create team"
      className="flex gap-2"
    >
      <TextInput
        outerClassname="w-full"
        placeholder="Team name"
        autoFocus
        onChange={formState.handleChange}
        value={formState.values.name}
        id="name"
        error={formState.errors.name}
        labelHidden
        aria-label="Team name"
      />

      <Button
        className="mt-px h-fit"
        size="sm"
        disabled={
          !formState.dirty || formState.isSubmitting || !formState.isValid
        }
        type="submit"
        aria-label="submit"
      >
        Create
      </Button>
    </form>
  );
}
