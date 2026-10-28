import { cn } from "@ui/cn";

export function Spinner({ className }: { className?: string }) {
  return (
    <svg
      role="status"
      className={cn(
        "ml-auto size-4 animate-rotate text-content-primary/70",
        className,
      )}
      style={{ animationDuration: "0.8s, 1.5s" }}
      viewBox="0 0 100 100"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <path
        d="M 50,50 m 0,-38 a 38,38 0 1,1 0,76 a 38,38 0 1,1 0,-76"
        pathLength="100"
        stroke="currentColor"
        strokeWidth="18"
        fill="none"
        strokeDasharray="35 65"
        className="animate-dashLength"
      />
    </svg>
  );
}
