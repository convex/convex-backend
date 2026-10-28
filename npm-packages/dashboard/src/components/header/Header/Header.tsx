import classNames from "classnames";
import Link from "next/link";
import { CommandPaletteTrigger } from "elements/CommandPalette";
import { DeploymentDisplay } from "elements/DeploymentDisplay";
import { useCurrentProject } from "api/projects";
import { User } from "@workos-inc/node";
import { ConvexStatusBadge } from "lib/ConvexStatusBadge";
import { useConvexStatus } from "hooks/useConvexStatus";
import { UserMenu } from "../UserMenu/UserMenu";

type HeaderProps = {
  children?: React.ReactNode;
  logoLink?: string;
  user: User | null;
};

function ConvexStatus() {
  const { status } = useConvexStatus();

  // Only show if there are issues (not operational) or still loading
  if (!status || status.indicator === "none") {
    return null;
  }

  return (
    <div className="flex items-center px-2.5">
      <ConvexStatusBadge status={status} />
    </div>
  );
}

export function Header({ children, logoLink = "/", user }: HeaderProps) {
  const project = useCurrentProject();

  return (
    <header
      className={classNames(
        "flex justify-between min-h-[56px] overflow-x-auto scrollbar-none bg-background-secondary border-b",
      )}
    >
      <div className="flex items-center bg-background-secondary pr-2">
        <Link
          href={logoLink}
          passHref
          className="group flex h-full items-center pl-2"
          data-testid="home-link"
        >
          <div className="rounded-full p-2 transition-colors group-hover:bg-background-tertiary">
            <ConvexLogo />
          </div>
        </Link>
        <div className="h-full">{children}</div>
      </div>
      {project && <DeploymentDisplay project={project} />}
      <div className="flex items-center bg-background-secondary px-2">
        <CommandPaletteTrigger />
        <div className="flex items-center">
          <ConvexStatus />
        </div>
        {user && <UserMenu />}
      </div>
    </header>
  );
}

function ConvexLogo() {
  return (
    <svg
      width={28}
      height={28}
      viewBox="0 0 367 370"
      style={{
        fillRule: "evenodd",
        clipRule: "evenodd",
        strokeLinejoin: "round",
        strokeMiterlimit: 2,
      }}
      aria-label="Convex logo"
    >
      <g transform="matrix(1,0,0,1,-129.225,-127.948)">
        <g transform="matrix(4.16667,0,0,4.16667,0,0)">
          <g transform="matrix(1,0,0,1,86.6099,107.074)">
            <path
              d="M0,-6.544C13.098,-7.973 25.449,-14.834 32.255,-26.287C29.037,2.033 -2.48,19.936 -28.196,8.94C-30.569,7.925 -32.605,6.254 -34.008,4.088C-39.789,-4.83 -41.69,-16.18 -38.963,-26.48C-31.158,-13.247 -15.3,-5.131 0,-6.544"
              style={{
                fill: "rgb(245,176,26)",
                fillRule: "nonzero",
              }}
            />
          </g>
          <g transform="matrix(1,0,0,1,47.1708,74.7779)">
            <path
              d="M0,-2.489C-5.312,9.568 -5.545,23.695 0.971,35.316C-21.946,18.37 -21.692,-17.876 0.689,-34.65C2.754,-36.197 5.219,-37.124 7.797,-37.257C18.41,-37.805 29.19,-33.775 36.747,-26.264C21.384,-26.121 6.427,-16.446 0,-2.489"
              style={{
                fill: "rgb(141,37,118)",
                fillRule: "nonzero",
              }}
            />
          </g>
          <g transform="matrix(1,0,0,1,91.325,66.4152)">
            <path
              d="M0,-14.199C-7.749,-24.821 -19.884,-32.044 -33.173,-32.264C-7.482,-43.726 24.112,-25.143 27.557,2.322C27.877,4.876 27.458,7.469 26.305,9.769C21.503,19.345 12.602,26.776 2.203,29.527C9.838,15.64 8.889,-1.328 0,-14.199"
              style={{
                fill: "rgb(238,52,47)",
                fillRule: "nonzero",
              }}
            />
          </g>
        </g>
      </g>
    </svg>
  );
}
