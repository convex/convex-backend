import { cn } from "@ui/cn";
import React, {
  createContext,
  useContext,
  Children,
  isValidElement,
  type PropsWithChildren,
} from "react";

type StepperContextValue = {
  activeStep: number;
  onSelectStep?: (index: number) => void;
  totalSteps: number;
};

const StepperContext = createContext<StepperContextValue | null>(null);

type StepContextValue = {
  index: number;
};

const StepContext = createContext<StepContextValue | null>(null);

function Step({ label, children }: PropsWithChildren<{ label: string }>) {
  const stepperCtx = useContext(StepperContext);
  const stepCtx = useContext(StepContext);
  if (!stepperCtx || !stepCtx) {
    throw new Error("Stepper.Step must be used within a Stepper");
  }

  const { activeStep, onSelectStep, totalSteps } = stepperCtx;
  const { index } = stepCtx;

  const isCompleted = index < activeStep;
  const isCurrent = index === activeStep;
  const isLast = index === totalSteps - 1;

  return (
    <div className="flex">
      {/* Left column: circle + connecting line */}
      <div className="mr-3 flex flex-col items-center">
        {/* eslint-disable-next-line react/forbid-elements -- custom timeline step circle indicator */}
        <button
          type="button"
          disabled={!isCompleted}
          onClick={() => isCompleted && onSelectStep?.(index)}
          className={cn(
            "flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-medium",
            isCompleted && "cursor-pointer bg-util-accent text-white",
            !isCompleted &&
              isCurrent &&
              "border-2 border-util-accent text-content-primary",
            !isCompleted &&
              !isCurrent &&
              "border border-border-transparent text-content-tertiary",
          )}
        >
          {index + 1}
        </button>
        {!isLast && (
          <div
            className={cn(
              "w-px grow",
              isCompleted ? "bg-util-accent" : "bg-border-transparent",
            )}
          />
        )}
      </div>

      {/* Right column: label + content */}
      <div
        className={cn("flex min-w-0 grow flex-col", !isLast ? "pb-6" : "pb-0")}
      >
        {/* eslint-disable-next-line react/forbid-elements -- custom stepper step label */}
        <button
          type="button"
          disabled={!isCompleted}
          onClick={() => isCompleted && onSelectStep?.(index)}
          className={cn(
            "flex h-7 items-center text-left font-semibold",
            isCompleted && "cursor-pointer text-content-primary",
            !isCompleted && isCurrent && "text-content-primary",
            !isCompleted && !isCurrent && "text-content-tertiary",
          )}
        >
          {label}
        </button>
        {/* CSS hidden instead of conditional rendering so children stay
            mounted across step changes (e.g. Stripe form elements). */}
        <div className={isCurrent ? "mt-3 flex flex-col gap-4" : "hidden"}>
          {children}
        </div>
      </div>
    </div>
  );
}

export function Stepper({
  activeStep,
  onSelectStep,
  children,
  className,
}: PropsWithChildren<{
  activeStep: number;
  onSelectStep?: (index: number) => void;
  className?: string;
}>) {
  const steps = Children.toArray(children).filter(isValidElement);
  const totalSteps = steps.length;

  return (
    <StepperContext.Provider value={{ activeStep, onSelectStep, totalSteps }}>
      <div className={cn("flex flex-col", className)}>
        {steps.map((child, index) => (
          <StepContext.Provider key={index} value={{ index }}>
            {child}
          </StepContext.Provider>
        ))}
      </div>
    </StepperContext.Provider>
  );
}

Stepper.Step = Step;
