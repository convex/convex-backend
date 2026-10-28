import classNames from "classnames";
import Image from "next/image";

type AvatarSize = "medium" | "large";

export type AvatarProps = {
  name?: string;
  hashKey?: string;
  size?: AvatarSize;
  isSystem?: boolean;
  patternIdx?: number; // Optional: force pattern index
  colorSchemeIdx?: number; // Optional: force color scheme index
  rotation?: number; // Optional: force rotation in degrees
};

const classesForSize: Record<AvatarSize, string> = {
  medium: "w-7 h-7",
  large: "w-12 h-12",
};

export const colorSchemes = [
  [
    "hsl(255, 60%, 36%)",
    "hsl(37, 35.7%, 55.5%)",
    "hsl(346, 100%, 85%)",
    "hsl(42, 97%, 54%)",
  ],
  [
    "hsl(3, 100%, 32%)",
    "hsl(42, 100%, 80%)",
    "hsl(29, 89%, 54%)",
    "hsl(0, 0%, 36%)",
  ],
  [
    "hsl(270, 13%, 27%)",
    "hsl(220, 56%, 78%)",
    "hsl(316, 59%, 77%)",
    "hsl(260, 60%, 51%)",
  ],
  [
    "hsl(220, 14%, 45%)",
    "hsl(120, 22%, 62%)",
    "hsl(6, 100%, 74%)",
    "hsl(312, 33%, 71%)",
  ],
  [
    "hsl(220, 14%, 45%)",
    "hsl(262, 87%, 74%)",
    "hsl(240, 70%, 42%)",
    "hsl(210, 66%, 84%)",
  ],
  [
    "hsl(6, 100%, 74%)",
    "hsl(40, 80%, 75%)",
    "hsl(316, 59%, 65%)",
    "hsl(42, 100%, 80%)",
  ],
];

export function Avatar({
  name = "",
  hashKey: slug = "",
  size = "medium",
  isSystem = false,
  patternIdx,
  colorSchemeIdx,
  rotation,
}: AvatarProps) {
  const initial = (
    name.split(" ").length > 1
      ? name.split(" ")[0][0] + name.split(" ")[1][0]
      : name.slice(0, 2)
  ).replace(/[^a-zA-Z0-9]/g, "");

  // Simple hash function for deterministic color selection
  function hashString(str: string) {
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
      hash = (hash << 5) - hash + str.charCodeAt(i);
      hash |= 0;
    }
    return Math.abs(hash);
  }

  // Hash logic to pick a pattern and a color scheme
  const hash = hashString(slug);
  const patternIdxFinal =
    typeof patternIdx === "number"
      ? patternIdx % 4 // 4 patterns for CSS gradients
      : hash % 4;
  const colorSchemeIdxFinal =
    typeof colorSchemeIdx === "number"
      ? colorSchemeIdx % colorSchemes.length
      : Math.floor(hash / 4) % colorSchemes.length;
  const baseColors = colorSchemes[colorSchemeIdxFinal];

  // Compute rotation: use prop if provided, else hash-based
  const rotationDeg = typeof rotation === "number" ? rotation : hash % 360;

  // Generate CSS gradient background based on patternIdxFinal
  let gradient: string;
  switch (patternIdxFinal) {
    case 0:
      // Linear gradient left to right
      gradient = `linear-gradient(${rotationDeg}deg, ${baseColors[0]}, ${baseColors[1]})`;
      break;
    case 1:
      // Linear gradient top left to bottom right
      gradient = `linear-gradient(${45 + rotationDeg}deg, ${baseColors[2]}, ${baseColors[3]})`;
      break;
    case 2:
      // Diagonal multi-stop gradient
      gradient = `linear-gradient(${135 + rotationDeg}deg, ${baseColors[2]}, ${baseColors[0]} 60%, ${baseColors[1]})`;
      break;
    case 3: {
      // Radial gradient: use rotation to move the center in a circle
      const theta = (rotationDeg / 180) * Math.PI;
      const r = 30; // radius in percent
      const x = 50 + r * Math.cos(theta); // percent
      const y = 50 + r * Math.sin(theta); // percent
      gradient = `radial-gradient(circle at ${x}% ${y}%, ${baseColors[0]} 0%, ${baseColors[1]} 80%, ${baseColors[2]} 100%)`;
      break;
    }
    default:
      gradient = `linear-gradient(${rotationDeg}deg, ${baseColors[0]}, ${baseColors[1]})`;
  }

  // Only apply pattern for non-system avatars
  const style = !isSystem
    ? {
        backgroundImage: gradient,
        backgroundSize: "cover",
      }
    : undefined;

  return (
    <span
      className={classNames(
        "inline-flex items-center select-none justify-center rounded-full relative overflow-hidden font-display shrink-0",
        isSystem ? "bg-util-accent/30 dark:bg-util-accent" : undefined,
        classesForSize[size],
      )}
      style={style}
    >
      {/* Overlay for contrast */}
      {!isSystem && (
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 z-10 size-full rounded-full bg-black/30 dark:bg-black/15"
        />
      )}
      {isSystem ? (
        <Image src="/convex-logo-only.svg" width="14" height="14" alt="" />
      ) : (
        <span
          className="relative z-20 text-sm leading-none font-medium text-white"
          style={{
            textShadow: "0 0 3px rgba(0, 0, 0, 0.5)",
          }}
        >
          {initial}
        </span>
      )}
    </span>
  );
}
