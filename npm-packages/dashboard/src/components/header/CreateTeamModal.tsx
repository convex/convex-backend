import { Modal } from "@ui/Modal";
import { LocalDevCallout } from "@common/elements/LocalDevCallout";
import { useProfile } from "api/profile";
import { useCreateTeamModalOpen } from "hooks/useCreateTeamModal";
import { CreateTeamForm } from "./CreateTeamForm";

// The single Create Team modal, rendered once near the app root and driven
// entirely by `useCreateTeamModalOpen`. Any surface (header, command palette)
// opens it by setting that state.
export function CreateTeamModal() {
  const [open, setOpen] = useCreateTeamModalOpen();
  const profile = useProfile();

  if (!open) {
    return null;
  }

  const onClose = () => setOpen(false);

  return (
    <Modal title="Create Team" onClose={onClose}>
      <p className="mb-5">
        Collaborate with your team members by creating a Convex Team.
      </p>
      <CreateTeamForm onClose={onClose} />
      <LocalDevCallout
        tipText="Tip: Run this to increase the number of teams you can create:"
        command={`just big-brain-tool-dev entitlement grant add --member-entitlement max_teams 500 --member-id ${profile?.id ?? "{MEMBER_ID}"} --reason "local" --for-real`}
      />
    </Modal>
  );
}
