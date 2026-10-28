import { useUser } from "@clerk/react";

export default function Badge() {
  const { user } = useUser();

  return (
    <p className="badge">
      <span>Logged in{user!.fullName ? ` as ${user!.fullName}` : ""}</span>
    </p>
  );
}
