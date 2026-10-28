import { EnterIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Link } from "@ui/Link";
import { LoginLayout } from "layouts/LoginLayout";
import { useRouter } from "next/router";
import { safeReturnTo } from "lib/returnTo";

export default function LoginError() {
  const { query } = useRouter();
  const returnTo = safeReturnTo(query.returnTo, "/");

  return (
    <div className="h-screen">
      <LoginLayout>
        <div className="flex max-w-prose flex-col items-center gap-4 text-center text-content-primary">
          <h2>We had trouble signing you in</h2>
          <p className="max-w-prose text-pretty text-content-secondary">
            Something went wrong while signing you in to Convex. Please try
            again in a few moments.
          </p>
          <Button
            href={`/api/auth/login?returnTo=${encodeURIComponent(returnTo)}`}
            icon={<EnterIcon />}
          >
            Try again
          </Button>
          <p className="mt-4 text-sm text-content-secondary">
            If the problem persists, check the{" "}
            <Link href="https://status.convex.dev">Convex status page</Link> or
            contact us at{" "}
            <Link href="mailto:support@convex.dev">support@convex.dev</Link>.
          </p>
        </div>
      </LoginLayout>
    </div>
  );
}
