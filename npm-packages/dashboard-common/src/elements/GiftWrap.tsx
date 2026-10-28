// NOTE: This file is mostly AI-generated with a frontier model. Any changes should be heavily tested and benchmarked for
// performance, cross-browser compatability, and regressions.
import {
  ReactNode,
  RefObject,
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { Cross2Icon } from "@radix-ui/react-icons";
import { cn } from "@ui/cn";
import { Button } from "@ui/Button";
import { useMediaQuery } from "@common/lib/useMediaQuery";

// `animationend` on the lid ends the opening; this backstops when it never fires
// (backgrounded tab, reduced motion, hidden panel).
const UNWRAP_TIMELINE_MS = 1150;
// Bow settles last, so the clock ends the wrap — there's no `animationend` to prefer.
const WRAP_TIMELINE_MS = 900;
const REDUCED_MOTION_TIMELINE_MS = 150;

// `wrapping` = the parcel being tied back up.
type Phase = "wrapped" | "unwrapping" | "wrapping" | "done";

type Motion = "still" | "untie" | "tie";

const YELLOW = "var(--brand-yellow)";
const PURPLE = "var(--brand-purple)";
const WHITE = "#fff";

type Spark = readonly [
  /** Launch point across the bar, as a percentage of its width. */
  left: number,
  /** Horizontal travel in px, signed. */
  x: number,
  /** Apex height in px, negative because the burst rises. */
  rise: number,
  /** Resting height in px, below the launch point. */
  fall: number,
  size: number,
  delay: number,
  duration: number,
  color: string,
];

// Hand-placed for art direction: generated positions miss the visual clustering
// toward the centre where the bow comes apart.
// prettier-ignore
const SPARKS: Spark[] = [
  // left     x  rise  fall  sz  dly  dur  colour
  [ 50,   -4,  -46,  14, 9,   0, 760, YELLOW],
  [ 50,   10,  -52,   8, 7,  20, 800, WHITE],
  [ 47,  -26,  -40,  20, 8,  10, 720, YELLOW],
  [ 53,   30,  -44,  16, 8,  30, 780, YELLOW],
  [ 44,  -48,  -34,  24, 6,  60, 700, WHITE],
  [ 56,   52,  -36,  22, 6,  50, 740, PURPLE],
  [ 41,  -70,  -28,  26, 7,  90, 680, YELLOW],
  [ 59,   74,  -30,  24, 7,  80, 720, WHITE],
  [ 36,  -96,  -24,  28, 5, 120, 660, PURPLE],
  [ 64,  100,  -26,  26, 5, 110, 700, YELLOW],
  [ 30,  -60,  -32,  22, 6, 150, 640, WHITE],
  [ 70,   64,  -34,  20, 6, 140, 680, YELLOW],
  [ 24,  -40,  -30,  24, 7, 180, 620, YELLOW],
  [ 76,   44,  -28,  26, 7, 170, 660, PURPLE],
  [ 18,  -24,  -26,  22, 5, 210, 600, WHITE],
  [ 82,   28,  -24,  24, 5, 200, 640, YELLOW],
  [ 12,  -14,  -22,  20, 6, 240, 580, YELLOW],
  [ 88,   16,  -20,  22, 6, 230, 620, WHITE],
  [  7,   -8,  -18,  18, 4, 270, 560, PURPLE],
  [ 93,   10,  -18,  20, 4, 260, 600, YELLOW],
  [ 46,  -16,  -60,   4, 5, 100, 820, WHITE],
  [ 54,   20,  -58,   6, 5, 130, 800, YELLOW],
  [ 49,  -34,  -50,  12, 4, 190, 740, YELLOW],
  [ 51,   38,  -48,  14, 4, 220, 760, WHITE],
];

/**
 * Wraps children as a gift parcel and unwraps them when clicked.
 *
 * `opened` is undefined while loading; children render bare during that window.
 * Once resolved the decision is frozen — only a true→false transition (rewrap)
 * is followed after mount.
 */
export function GiftWrap({
  children,
  className,
  explanation,
  opened,
  onOpen,
}: {
  children: ReactNode;
  className?: string;
  /** Shown in a bubble below the parcel, pointing up at it. */
  explanation?: string;
  /** Whether the gift has been opened. Undefined while loading. */
  opened?: boolean;
  /** Called when the parcel is clicked open. */
  onOpen: () => void;
}) {
  const reducedMotion = useMediaQuery("(prefers-reduced-motion: reduce)");

  // null = still loading; frozen once resolved, except when a rewrap fires.
  const [shouldWrap, setShouldWrap] = useState<boolean | null>(() =>
    opened === undefined ? null : !opened,
  );
  const [phase, setPhase] = useState<Phase>("wrapped");
  const [barRect, setBarRect] = useState<DOMRect | null>(null);
  // True after a rewrap so the explanation bubble is suppressed.
  const [replay, setReplay] = useState(false);
  // Announced on click, not at animation end, so screen readers hear it immediately.
  const [tieAnnouncement, setTieAnnouncement] = useState("");

  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  // Focus target for clicks that remove the element that was focused.
  const pendingFocus = useRef<"controls" | "parcel" | null>(null);
  const wrapperRef = useRef<HTMLDivElement>(null);
  // Focus target for a rewrap.
  const parcelRef = useRef<HTMLElement>(null);

  // `opened` changing is the only thing that decides — or undoes — the wrap.
  // Derived during render rather than in an effect so the answer lands on the
  // commit the prop arrives on: an effect would paint one frame of bare
  // controls first, which on a wrapped bar is a flash of what the paper is
  // there to cover.
  const [prevOpened, setPrevOpened] = useState(opened);
  if (prevOpened !== opened) {
    setPrevOpened(opened);
    if (prevOpened === undefined) {
      // The first resolution decides, and the answer is frozen from here: a
      // second tab opening the gift can't pull the paper away mid-animation.
      setShouldWrap(!opened);
    } else if (opened === false) {
      // true→false is the rewrap request. The control that triggered it is
      // gone, so focus has nowhere to return.
      pendingFocus.current = "parcel";
      setTieAnnouncement("Filter bar wrapped up again.");
      setReplay(true);
      setShouldWrap(true);
      setPhase(reducedMotion ? "wrapped" : "wrapping");
    }
  }

  const timelineMs = reducedMotion
    ? REDUCED_MOTION_TIMELINE_MS
    : phase === "wrapping"
      ? WRAP_TIMELINE_MS
      : UNWRAP_TIMELINE_MS;

  const showExplanation = !!explanation && !replay && phase === "wrapped";
  const rect = useRect(wrapperRef, shouldWrap === true && phase !== "done");

  const finish = useCallback(() => {
    setPhase((p) => {
      if (p === "unwrapping") return "done";
      if (p === "wrapping") return "wrapped";
      return p;
    });
  }, []);

  const unwrap = useCallback(() => {
    if (phaseRef.current !== "wrapped") return;
    pendingFocus.current = "controls";
    setBarRect(wrapperRef.current?.getBoundingClientRect() ?? null);
    setTieAnnouncement("");
    setReplay(false);
    onOpen();
    setPhase("unwrapping");
  }, [onOpen]);

  // A moving phase gets the clock that ends it — which the wrap needs outright
  // and the opening only falls back to when `animationend` never fires. A
  // settled phase gets the focus instead: both directions remove the control
  // that was clicked, and focus would otherwise fall to the body.
  useLayoutEffect(() => {
    if (phase === "unwrapping" || phase === "wrapping") {
      const timer = window.setTimeout(
        finish,
        // The wrap lands on its own timeline; the opening leaves `animationend`
        // room to get there first.
        phase === "wrapping" ? timelineMs : timelineMs + 250,
      );
      return () => window.clearTimeout(timer);
    }
    const target = pendingFocus.current;
    if (!target) return undefined;
    if (target === "parcel") {
      // A rewrap can land before `rect` arrives (reduced motion skips the
      // "wrapping" phase), so retry once it does.
      if (!parcelRef.current) return undefined;
      pendingFocus.current = null;
      if (document.activeElement === document.body) parcelRef.current.focus();
      return undefined;
    }
    pendingFocus.current = null;
    // Only rescue when focus fell to body — a valid focus position is fine.
    if (document.activeElement === document.body) {
      wrapperRef.current
        ?.querySelector<HTMLElement>(
          'button:not([disabled]), [tabindex]:not([tabindex="-1"])',
        )
        ?.focus();
    }
    return undefined;
  }, [phase, finish, timelineMs, rect]);

  if (shouldWrap === null || !shouldWrap) return <>{children}</>;

  return (
    // Outer div grows with the full row; inner w-fit limits the parcel width.
    <div className={className}>
      <div ref={wrapperRef} className="relative w-fit max-w-full">
        <div
          inert={phase === "wrapped" || phase === "wrapping"}
          className={cn(
            phase === "wrapped" && "opacity-0",
            // Transition survives the prefers-reduced-motion rule that kills the keyframes.
            phase === "unwrapping" &&
              (reducedMotion
                ? "opacity-100 transition-opacity duration-150"
                : "animate-giftReveal"),
            // Fade out before the paper meets in the middle so nothing shows through the seam.
            phase === "wrapping" && "animate-giftConceal",
          )}
        >
          {children}
        </div>

        {phase !== "done" && rect && (
          // Fixed so scroll panels can't clip the parcel; an ancestor with
          // transform, filter, contain or perspective clips it anyway.
          <div
            className="fixed z-30"
            style={{
              left: rect.left,
              top: rect.top,
              width: rect.width,
              height: rect.height,
            }}
          >
            <Paper
              buttonRef={parcelRef}
              phase={phase}
              reducedMotion={reducedMotion}
              onActivate={unwrap}
              onOpened={finish}
            />

            {(phase === "unwrapping" || phase === "wrapping") &&
              !reducedMotion && (
                <div
                  className={cn(
                    "pointer-events-none absolute -inset-1 rounded-md border border-util-brand-purple",
                    phase === "unwrapping"
                      ? "animate-giftGlowRing"
                      : "animate-giftCinchRing",
                  )}
                  aria-hidden
                />
              )}

            {showExplanation && (
              <Explanation onDismiss={unwrap}>{explanation}</Explanation>
            )}
          </div>
        )}

        {phase === "unwrapping" && !reducedMotion && barRect && (
          <SparkBurst rect={barRect} />
        )}

        {/* Mounted empty from the first render: a live region inserted already
            populated is frequently not announced by screen readers. */}
        <span role="status" aria-live="polite" className="sr-only">
          {phase === "done" ? "Filter bar unwrapped." : tieAnnouncement}
        </span>
      </div>
    </div>
  );
}

function useRect(ref: RefObject<HTMLElement | null>, active: boolean) {
  const [rect, setRect] = useState<DOMRect | null>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!active || !element) return undefined;
    const measure = () => setRect(element.getBoundingClientRect());
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    window.addEventListener("resize", measure);
    // Captured (true) because scroll inside a panel doesn't bubble to window.
    window.addEventListener("scroll", measure, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, true);
    };
  }, [ref, active]);
  return rect;
}

function Explanation({
  children,
  onDismiss,
}: {
  children: ReactNode;
  onDismiss(): void;
}) {
  return (
    <div
      className="absolute top-full left-1/2 mt-2 w-max max-w-[min(20rem,calc(100vw-1rem))] -translate-x-1/2 animate-fadeInFromLoading"
      data-testid="giftExplanation"
    >
      <div className="relative rounded-md bg-util-accent text-xs/snug text-white shadow-sm">
        {/* Rotated square arrow — inherits the bubble colour with nothing to sync. */}
        <span
          aria-hidden
          className="absolute -top-1 left-1/2 size-2 -translate-x-1/2 rotate-45 bg-util-accent"
        />
        {/* Not a tab stop: the parcel button and dismiss already cover this action. */}
        <Button
          variant="unstyled"
          tabIndex={-1}
          onClick={onDismiss}
          className="relative block cursor-pointer px-2.5 py-1.5 text-center text-xs/snug text-white"
        >
          {children}
        </Button>
        {/* Sibling, not child — a button can't contain another button. */}
        <Button
          variant="unstyled"
          aria-label="Dismiss"
          onClick={onDismiss}
          className="absolute -top-1.5 -right-1.5 cursor-pointer rounded-full bg-util-accent p-0.5 text-white shadow-sm ring-1 ring-white/40 hover:bg-util-accent/90"
          icon={<Cross2Icon className="size-2.5" />}
        />
      </div>
    </div>
  );
}

/** Static gift icon (paper, band, bow) with no animation. */
export function GiftParcel({ className }: { className?: string }) {
  return (
    <span
      aria-hidden
      className={cn(
        "gift-paper relative block size-5 overflow-hidden rounded-sm border border-util-brand-purple",
        className,
      )}
    >
      <RibbonHalf half="top" motion="still" width="w-1" />
      <RibbonHalf half="bottom" motion="still" width="w-1" />
      {/* Bow drawn for ~58px; at 1.25rem scale 0.3 brings it to roughly a third. */}
      <Bow motion="still" scale={0.3} />
    </span>
  );
}

function Paper({
  buttonRef,
  phase,
  reducedMotion,
  onActivate,
  onOpened,
}: {
  /** Focus target for a rewrap. */
  buttonRef: RefObject<HTMLElement | null>;
  phase: Phase;
  reducedMotion: boolean;
  onActivate(): void;
  onOpened(): void;
}) {
  const unwrapping = phase === "unwrapping";
  const wrapping = phase === "wrapping";
  // Paper is in pieces while animating; only at rest does it get the full sheet and shake.
  const still = !unwrapping && !wrapping;
  const motion: Motion = unwrapping ? "untie" : wrapping ? "tie" : "still";
  return (
    <Button
      variant="unstyled"
      ref={buttonRef}
      aria-label="Unwrap the filter bar"
      onClick={onActivate}
      className={cn(
        // 1px overhang so the parcel sits on top of the controls rather than shrink-wrapping them.
        "group absolute -inset-1 overflow-hidden rounded-md",
        "cursor-pointer touch-manipulation select-none",
        // Visible under forced colors, which strips the paper gradients.
        "border border-util-brand-purple",
        // Transitioned so the shadow fades in with the tie rather than popping in.
        "transition-shadow duration-200",
        still && "gift-paper shadow-md",
        unwrapping && "animate-giftPull will-change-transform",
        wrapping && "animate-giftCinch will-change-transform",
        // origin-bottom so it rocks like a box nudged from inside, not pivoting at its centre.
        still && !reducedMotion && "animate-giftShake origin-bottom",
      )}
    >
      {/* Top half fires onOpened (last element to settle); wrap is ended by the clock instead. */}
      {!still && (
        <>
          <LidStrip
            half="top"
            motion={motion}
            onAnimationEnd={unwrapping ? onOpened : undefined}
          />
          <LidStrip half="bottom" motion={motion} />
        </>
      )}

      <RibbonHalf half="top" motion={motion} />
      <RibbonHalf half="bottom" motion={motion} />

      <Bow motion={motion} />

      {/* Hover highlight as an overlay — the parcel's transform belongs to the shake
          and a second transform on the same element would replace it. */}
      {still && (
        <span
          aria-hidden
          className="pointer-events-none absolute inset-0 bg-white opacity-0 transition-opacity duration-200 group-hover:opacity-10"
        />
      )}
    </Button>
  );
}

function LidStrip({
  half,
  motion,
  onAnimationEnd,
}: {
  half: "top" | "bottom";
  motion: Motion;
  onAnimationEnd?: () => void;
}) {
  const top = half === "top";
  return (
    // Outer = clipping window for this half; inner = full-height paper layer so
    // diagonal stripes align across the seam.
    <span
      aria-hidden
      onAnimationEnd={onAnimationEnd}
      style={{ "--gift-lid-dir": top ? -1 : 1 } as React.CSSProperties}
      className={cn(
        "absolute inset-x-0 h-1/2 overflow-hidden will-change-transform",
        motion === "tie" ? "animate-giftLidClose" : "animate-giftLidOpen",
        top ? "top-0 origin-top" : "bottom-0 origin-bottom",
      )}
    >
      <span
        className={cn(
          "gift-paper absolute inset-x-0 h-[200%]",
          top ? "top-0" : "bottom-0",
        )}
      />
    </span>
  );
}

function RibbonHalf({
  half,
  motion,
  width = "w-2.5",
}: {
  half: "top" | "bottom";
  motion: Motion;
  /** Width of the girth band; small parcels need it narrower. */
  width?: string;
}) {
  // Centring on the outer span — the snap keyframe owns `transform` on the inner
  // span and would drop a translate declared there.
  return (
    <span
      aria-hidden
      className={cn(
        "absolute left-1/2 h-1/2 -translate-x-1/2",
        width,
        half === "top" ? "top-0" : "bottom-0",
      )}
    >
      <span
        style={{
          // Darker edges make the band read as a curved fold.
          background:
            "linear-gradient(90deg, var(--yellow-700), var(--brand-yellow) 40%, var(--brand-yellow) 60%, var(--yellow-700))",
        }}
        className={cn(
          "block size-full",
          half === "top" ? "origin-top" : "origin-bottom",
          motion !== "still" && "will-change-transform",
          motion === "untie" && "animate-giftRibbonSnap",
          motion === "tie" && "animate-giftRibbonTie",
        )}
      />
    </span>
  );
}

function Bow({
  motion,
  scale,
}: {
  motion: Motion;
  /** Shrinks the bow for smaller parcels. */
  scale?: number;
}) {
  return (
    <svg
      viewBox="0 0 76 32"
      width={58}
      height={24}
      aria-hidden
      // Composes with `-translate-1/2`: Tailwind writes that as the standalone
      // `translate` property, not into `transform`, so they don't conflict.
      style={scale ? { transform: `scale(${scale})` } : undefined}
      // overflow-visible: loops fly outside the SVG viewport when released.
      className="pointer-events-none absolute top-1/2 left-1/2 -translate-1/2 overflow-visible"
    >
      {/* Solid fills only — dashed strokes are pathological in WebKit. */}
      <BowLoop mirrored={false} motion={motion} />
      <BowLoop mirrored motion={motion} />
      <g
        style={{ transformBox: "fill-box", transformOrigin: "center" }}
        className={cn(
          motion === "untie" && "animate-giftKnotRelease",
          motion === "tie" && "animate-giftKnotTie",
        )}
      >
        <ellipse cx="38" cy="16" rx="6" ry="5" fill="var(--brand-yellow)" />
        <ellipse
          cx="38"
          cy="14.2"
          rx="3"
          ry="1.8"
          fill="rgb(255 255 255 / 0.5)"
        />
      </g>
    </svg>
  );
}

function BowLoop({ mirrored, motion }: { mirrored: boolean; motion: Motion }) {
  // Outer group holds the mirror so the animation's `transform` on the inner group
  // doesn't override it.
  return (
    <g transform={mirrored ? "scale(-1 1) translate(-76 0)" : undefined}>
      <g
        style={
          {
            "--gift-bow-dir": -1,
            transformBox: "fill-box",
            transformOrigin: "center",
          } as React.CSSProperties
        }
        className={cn(
          motion === "untie" && "animate-giftBowLoop",
          motion === "tie" && "animate-giftBowLoopTie",
        )}
      >
        <path
          d="M38 16C33 9 22 1 13 3C3 5 2 15 8 20C15 25 32 21 38 16Z"
          fill="var(--brand-yellow)"
        />
        <path
          d="M38 16C33 13.5 26 12.8 19.5 15C25.5 18.4 33 18.6 38 16Z"
          fill="var(--yellow-700)"
        />
      </g>
    </g>
  );
}

// Fixed: the container's overflow-y:hidden computes overflow-x to auto,
// clipping both axes and leaving sparks nowhere to go.
function SparkBurst({ rect }: { rect: DOMRect }) {
  return (
    <div
      aria-hidden
      className="pointer-events-none fixed z-30"
      style={{
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: rect.height,
      }}
    >
      {SPARKS.map(([left, x, rise, fall, size, delay, duration, color], i) => (
        <span
          key={i}
          className="animate-giftSparkArc absolute top-1/2 will-change-transform"
          style={
            {
              left: `${left}%`,
              "--gift-spark-x": `${x}px`,
              "--gift-spark-rise": `${rise}px`,
              "--gift-spark-fall": `${fall}px`,
              animationDelay: `${delay}ms`,
              animationDuration: `${duration}ms`,
            } as React.CSSProperties
          }
        >
          <span
            className="gift-spark animate-giftSparkPop block will-change-transform"
            style={{
              width: size,
              height: size,
              // Radial gradient instead of drop-shadow: avoids a paint pass per spark per frame.
              background: `radial-gradient(circle, ${WHITE} 0%, ${color} 55%, ${color} 100%)`,
              animationDelay: `${delay}ms`,
              animationDuration: `${duration}ms`,
            }}
          />
        </span>
      ))}
    </div>
  );
}
