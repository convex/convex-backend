import { cn } from "@ui/cn";

export function Loading({
  className,
  fullHeight = true,
  children,
  shimmer = true,
}: {
  className?: string;
  fullHeight?: boolean;
  children?: React.ReactNode;
  shimmer?: boolean;
}) {
  return (
    <div
      aria-busy="true"
      aria-live="polite"
      className={cn(
        "animate-fadeInFromLoading",
        "relative isolate overflow-hidden rounded-sm transition-opacity",
        "before:absolute before:inset-0",
        "before:-translate-x-full",
        shimmer && "before:animate-loading",
        "before:bg-linear-to-r before:from-transparent",
        "before:via-neutral-3/30",
        "before:to-transparent",
        fullHeight && "h-full",
        className,
      )}
    >
      {children}
    </div>
  );
}

export function LoadingTransition({
  loadingState,
  loadingProps,
  children,
}: {
  loadingState?: React.ReactNode;
  loadingProps?: Parameters<typeof Loading>[0];
  children?: React.ReactNode;
}) {
  return children ? (
    <div className="scrollbar flex h-full max-h-full grow animate-fadeInFromLoading overflow-y-auto">
      {children}
    </div>
  ) : (
    <Loading {...loadingProps}>{loadingState}</Loading>
  );
}

export function LoadingLogo() {
  return (
    <div className="size-20 animate-fadeIn">
      <svg
        width="100%"
        height="100%"
        viewBox="0 0 367 370"
        version="1.1"
        xmlns="http://www.w3.org/2000/svg"
        xmlSpace="preserve"
        className="animate-rotate"
        style={{
          fillRule: "evenodd",
          clipRule: "evenodd",
          strokeLinejoin: "round",
          strokeMiterlimit: 2,
        }}
      >
        <g transform="matrix(1,0,0,1,-129.225,-127.948)">
          <g id="Layer-1" transform="matrix(4.16667,0,0,4.16667,0,0)">
            <g transform="matrix(1,0,0,1,86.6099,107.074)">
              <path
                d="M0,-6.544C13.098,-7.973 25.449,-14.834 32.255,-26.287C29.037,2.033 -2.48,19.936 -28.196,8.94C-30.569,7.925 -32.605,6.254 -34.008,4.088C-39.789,-4.83 -41.69,-16.18 -38.963,-26.48C-31.158,-13.247 -15.3,-5.131 0,-6.544"
                className="animate-blinkFill"
                style={{
                  fill: `rgb(245,176,26)`,
                  fillRule: "nonzero",
                }}
              />
            </g>
            <g transform="matrix(1,0,0,1,47.1708,74.7779)">
              <path
                d="M0,-2.489C-5.312,9.568 -5.545,23.695 0.971,35.316C-21.946,18.37 -21.692,-17.876 0.689,-34.65C2.754,-36.197 5.219,-37.124 7.797,-37.257C18.41,-37.805 29.19,-33.775 36.747,-26.264C21.384,-26.121 6.427,-16.446 0,-2.489"
                className="animate-blinkFill"
                style={{
                  fill: `rgb(141,37,118)`,
                  fillRule: "nonzero",
                }}
              />
            </g>
            <g transform="matrix(1,0,0,1,91.325,66.4152)">
              <path
                d="M0,-14.199C-7.749,-24.821 -19.884,-32.044 -33.173,-32.264C-7.482,-43.726 24.112,-25.143 27.557,2.322C27.877,4.876 27.458,7.469 26.305,9.769C21.503,19.345 12.602,26.776 2.203,29.527C9.838,15.64 8.889,-1.328 0,-14.199"
                className="animate-blinkFill"
                style={{
                  fill: `rgb(238,52,47)`,
                  fillRule: "nonzero",
                }}
              />
            </g>
          </g>
        </g>
      </svg>
    </div>
  );
}
