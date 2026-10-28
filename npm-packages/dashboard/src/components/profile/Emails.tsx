import { Sheet } from "@ui/Sheet";
import { MemberEmailResponse } from "generatedApi";
import { EmailList } from "./EmailList";
import { PROFILE_SECTIONS } from "lib/sectionAnchors";

export function Emails({ emails }: { emails: MemberEmailResponse[] }) {
  return (
    <Sheet id={PROFILE_SECTIONS.emails.id} className="flex flex-col gap-4">
      <h3>Emails</h3>
      <p className="max-w-prose">
        The emails associated with your account are used to accept team
        invitations. Account-related communications will be sent to your primary
        email.
      </p>
      <EmailList emails={emails} />
    </Sheet>
  );
}
