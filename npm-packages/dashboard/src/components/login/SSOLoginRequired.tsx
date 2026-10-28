import { useRouter } from "next/router";
import { Sheet } from "@ui/Sheet";
import { Button } from "@ui/Button";
import { ExitIcon, LockClosedIcon } from "@radix-ui/react-icons";
import { useCurrentTeam } from "api/teams";
import { logout } from "lib/logout";
import { ssoLoginPath } from "lib/ssoLogin";

export function SSOLoginRequired() {
  const router = useRouter();
  const ssoLoginId = useCurrentTeam()?.ssoLoginId;

  return (
    <SSOLoginRequiredSheet
      ssoLoginHref={
        ssoLoginId ? ssoLoginPath(ssoLoginId, router.asPath) : undefined
      }
    />
  );
}

export function SSOLoginRequiredSheet({
  ssoLoginHref,
}: {
  ssoLoginHref?: string;
}) {
  return (
    <div className="flex size-full items-center justify-center">
      <Sheet className="flex max-w-prose flex-col gap-4">
        <div className="flex items-center gap-2">
          <LockClosedIcon className="size-8" />
          <h3>Single Sign-On Login Required</h3>
        </div>
        <span className="flex flex-col gap-2">
          <p>
            This team requires you to log in with Single Sign-On to access it.
          </p>
          <p>
            Log in through this team's Single Sign-On provider, or switch teams
            by using the selector on the top of this page.
          </p>
        </span>
        <div className="ml-auto flex gap-2">
          <Button
            variant={ssoLoginHref ? "neutral" : "primary"}
            onClick={() => logout()}
            icon={<ExitIcon />}
          >
            Log Out
          </Button>
          {ssoLoginHref && (
            <Button
              icon={<LockClosedIcon />}
              onClick={() => {
                // A full navigation, because `/api/auth/login` is an API
                // route that redirects to AuthKit.
                window.location.href = ssoLoginHref;
              }}
            >
              Log in with SSO
            </Button>
          )}
        </div>
      </Sheet>
    </div>
  );
}
