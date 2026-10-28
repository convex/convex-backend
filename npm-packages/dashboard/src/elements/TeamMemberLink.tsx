import { Link } from "@ui/Link";
import { useCurrentTeam } from "../api/teams";

export function TeamMemberLink({
  memberId,
  name,
  isMember = true,
}: {
  memberId?: number | null;
  name: string;
  isMember?: boolean;
}) {
  const team = useCurrentTeam();
  if (!memberId) {
    return <span>Convex</span>;
  }
  if (!isMember) {
    return <span className="text-content-secondary">Unknown team member</span>;
  }
  return (
    <Link
      target="_blank"
      className="rounded-sm hover:underline focus-visible:outline-2 focus-visible:outline-border-selected"
      href={`/t/${team?.slug}/settings/members#${memberId}`}
    >
      {name}
    </Link>
  );
}
