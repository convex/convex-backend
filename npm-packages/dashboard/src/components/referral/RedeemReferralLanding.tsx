import { GitHubLogoIcon } from "@radix-ui/react-icons";
import { logEvent } from "convex-analytics";
import { Sheet } from "@ui/Sheet";
import { Spinner } from "@ui/Spinner";
import { cn } from "@ui/cn";
import { useState } from "react";

export function RedeemReferralLanding({
  title,
  code,
  isChef,
}: {
  title: string;
  code: string;
  isChef: boolean;
}) {
  return (
    <div className="relative mt-10 max-w-lg">
      <Sheet>
        <DisplayH1>{title}</DisplayH1>

        <DisplayP>
          {isChef
            ? "Chef is an app builder powered by Convex, the open-source reactive database for app developers."
            : "Convex is the open-source reactive database for app developers."}
        </DisplayP>

        <DisplayP>
          Accept this referral to double your free account quota.
        </DisplayP>

        <LogInButton code={code} isChef={isChef} />
      </Sheet>
    </div>
  );
}

function DisplayH1({ children }: React.PropsWithChildren) {
  return (
    <h1
      // eslint-disable-next-line no-restricted-syntax
      className="mb-6 font-display text-3xl leading-[1.1] font-black tracking-tight text-content-primary sm:text-5xl"
    >
      {children}
    </h1>
  );
}

function DisplayP({ children }: React.PropsWithChildren) {
  return (
    <p
      // eslint-disable-next-line no-restricted-syntax
      className="mb-4 text-xl/snug font-medium text-content-primary"
    >
      {children}
    </p>
  );
}

function LogInButton({ code, isChef }: { code: string; isChef: boolean }) {
  const [clicked, setClicked] = useState(false);

  return (
    // Using <a> instead of <Button>/<Link> to fix an issue where auth would refuse to redirect
    // to GitHub when following the link.
    <a
      className={cn(
        "group z-10 my-2 inline-flex rounded-full bg-linear-to-br from-[#8d2676_33%] via-[#ee342f] via-90% to-[#f3b01c] to-100% p-0.5 font-display shadow-[0_2px_14px_rgba(111,0,255,0.25)] transition-shadow",
        !clicked && "hover:shadow-[rgba(111,0,255,0.5)]",
        clicked && "cursor-progress opacity-80",
      )}
      href={`/api/auth/login?returnTo=${encodeURIComponent(isChef ? `/try-chef/${code}/apply` : `/referral/${code}/apply`)}`}
      onClick={() => {
        logEvent(
          `clicked “Sign up with GitHub” through ${isChef ? "Chef " : ""}referral landing`,
        );
        setClicked(true);
      }}
      aria-disabled={clicked}
    >
      <span
        // eslint-disable-next-line no-restricted-syntax
        className="flex w-full items-center gap-2 rounded-full bg-[#292929] px-8 py-2 text-center text-lg font-medium text-white transition-colors group-hover:bg-[#141414] lg:text-xl"
      >
        <GitHubLogoIcon className="size-6" />
        Sign up with GitHub
        {clicked && <Spinner className="size-4 text-white" />}
      </span>
    </a>
  );
}
