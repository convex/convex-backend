import { MemberResponse } from "generatedApi";

const MAX_REFERENCE_LENGTH = 100;

export function defaultDevDeploymentReference(
  member: Pick<MemberResponse, "name" | "email">,
) {
  const name = member.name?.trim() ? member.name : member.email.split("@")[0];
  return `dev/${slugify(name)}`.slice(0, MAX_REFERENCE_LENGTH);
}

function slugify(s: string) {
  return s
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}
