import React, { forwardRef, useContext } from "react";
import { tv } from "tailwind-variants";
import { Tooltip, TooltipSide } from "@ui/Tooltip";
import { UrlObject } from "url";
import { UIContext } from "@ui/UIContext";
import { Spinner } from "@ui/Spinner";
import classNames from "classnames";

export type ButtonProps = {
  id?: string;
  children?: React.ReactNode;
  variant?: "primary" | "danger" | "neutral" | "unstyled";
  inline?: boolean;
  size?: ButtonSize;
  focused?: boolean;
  icon?: React.ReactNode;
  className?: string;
  disabled?: boolean;
  tip?: React.ReactNode;
  tipSide?: TooltipSide;
  tipDisableHoverableContent?: boolean;
  loading?: boolean;
} & Pick<
  React.HTMLProps<HTMLElement>,
  | "tabIndex"
  | "role"
  | "aria-label"
  | "style"
  | "title"
  | "onMouseOver"
  | "onKeyDown"
> &
  (
    | {
        href?: never;
        onClick?: React.ButtonHTMLAttributes<HTMLButtonElement>["onClick"];
        onClickOfAnchorLink?: never;
        type?: React.ButtonHTMLAttributes<HTMLButtonElement>["type"];
        target?: never;
      }
    | {
        href: React.AnchorHTMLAttributes<HTMLAnchorElement>["href"] | UrlObject;
        onClick?: never;
        // In most cases you shouldn’t use this. This is only useful when you
        // need the event sent before the native link behavior is handled.
        onClickOfAnchorLink?: React.AnchorHTMLAttributes<HTMLAnchorElement>["onClick"];
        download?: boolean;
        type?: never;
        target?: React.AnchorHTMLAttributes<HTMLAnchorElement>["target"];
      }
  );

export type ButtonSize = "xs" | "sm" | "md" | "lg";

export const Button = forwardRef<HTMLElement, ButtonProps>(function Button(
  {
    id,
    children,
    inline = false,
    variant = "primary",
    className = "",
    size = "sm",
    disabled = false,
    focused = false,
    icon,
    tip,
    tipSide,
    tipDisableHoverableContent,
    loading = false,
    ...props
  },
  ref,
) {
  const Link = useContext(UIContext);
  const { href, onClick, target, type, onClickOfAnchorLink, ...htmlProps } =
    "href" in props
      ? { ...props, type: undefined }
      : { ...props, href: undefined, target: undefined };
  const buttonClassName =
    variant === "unstyled"
      ? className
      : buttonClasses({
          inline,
          icon,
          variant,
          disabled,
          focused,
          className,
          size,
          loading,
        });
  if (href !== undefined && !disabled) {
    return (
      <Tooltip
        tip={tip}
        side={tipSide}
        disableHoverableContent={tipDisableHoverableContent}
        asChild
      >
        <Link
          passHref
          href={href}
          // There is something weird here with `forwardRef`, I’d expect this to work without `any`
          ref={ref as any}
          role="link"
          rel="noopener noreferrer"
          className={buttonClassName}
          tabIndex={0}
          target={target}
          onClick={onClickOfAnchorLink}
          {...htmlProps}
        >
          {icon && <div>{icon}</div>}
          {children}
        </Link>
      </Tooltip>
    );
  }
  return (
    <Tooltip
      tip={tip}
      side={tipSide}
      disableHoverableContent={tipDisableHoverableContent}
      asChild
    >
      {/* we're allowed to use button here. It's the Button component */}
      {/* eslint-disable-next-line react/forbid-elements */}
      <button
        id={id}
        type={type ?? "button"}
        tabIndex={0}
        onClick={onClick}
        style={{ "--final-opacity": disabled ? 0.5 : 1 } as React.CSSProperties}
        className={buttonClassName}
        disabled={disabled || loading}
        // There is something weird here with `forwardRef`, I’d expect this to work without `any`
        ref={ref as any}
        {...htmlProps}
      >
        {/* This needs to be wrapped in a dom element to
          fix an issue with the google translate extension
          throwing errors when the icon switches between different icons.
          The negative margin is added when the icon doesn't exist
          to not render the flex gap.
          https://github.com/facebook/react/issues/11538#issuecomment-390386520
       */}
        {icon && <div>{icon}</div>}
        {children}
        {loading && (
          <div
            className={classNames(
              "transition-none absolute left-1/2 -translate-x-1/2",
            )}
          >
            <span className="sr-only">(Loading...)</span>
            <Spinner
              className={
                variant === "primary"
                  ? "text-white"
                  : variant === "danger"
                    ? "text-content-error"
                    : undefined
              }
            />
          </div>
        )}
      </button>
    </Tooltip>
  );
});

const button = tv({
  base: "animate-fadeInToVar relative inline-flex items-center rounded-md text-sm font-medium whitespace-nowrap transition-colors select-none focus-visible:border focus-visible:border-border-selected focus-visible:outline-hidden",
  variants: {
    variant: {
      primary: "border-white/30 bg-util-accent text-white",
      neutral: "text-content-primary",
      danger: "text-content-error",
    },
    icon: {
      true: "gap-1.5",
      false: "gap-2.5",
    },
    accent: {
      none: "border",
      inline: "border border-transparent",
    },
    disabled: {
      true: "cursor-not-allowed opacity-50 disabled:cursor-not-allowed disabled:opacity-50",
      false: "cursor-pointer",
    },
    loading: {
      true: "cursor-not-allowed text-transparent",
      false: "",
    },
    focused: {
      true: "",
      false: "",
    },
    size: {
      xs: "p-1",
      sm: "p-1.5",
      md: "px-4 py-3",
      lg: "rounded-lg px-6 py-4.5",
    },
  },
  compoundVariants: [
    {
      variant: "primary",
      accent: "inline",
      class: "bg-transparent text-content-accent hover:bg-background-tertiary",
      loading: false,
    },
    {
      variant: "primary",
      accent: "inline",
      class: "hover:bg-transparent",
      disabled: true,
    },
    {
      variant: "primary",
      focused: true,
      accent: "none",
      class: "bg-util-accent/80 text-white",
    },
    {
      variant: "primary",
      focused: true,
      accent: "inline",
      class: "bg-background-tertiary",
    },
    {
      variant: "neutral",
      accent: "none",
      class: "bg-background-tertiary",
    },
    {
      variant: "neutral",
      accent: "none",
      class: "border bg-background-secondary",
    },
    {
      variant: "neutral",
      class: "hover:bg-background-tertiary",
      disabled: false,
      loading: false,
    },
    {
      variant: "neutral",
      focused: true,
      accent: ["none", "inline"],
      class: "bg-background-primary",
    },
    {
      variant: "neutral",
      focused: true,
      accent: "none",
      class: "border border-border-selected bg-background-secondary",
    },
    {
      variant: "danger",
      accent: "none",
      class: "border-content-error/30 bg-background-error",
    },
    {
      variant: "danger",
      focused: true,
      class: "bg-background-errorSecondary text-content-error",
    },
    {
      variant: "primary",
      disabled: false,
      accent: "none",
      class: "hover:bg-util-accent/80",
      loading: false,
    },
    {
      variant: "danger",
      disabled: false,
      class: "hover:bg-background-errorSecondary",
      loading: false,
    },
    {
      variant: "neutral",
      disabled: false,
      class: "hover:bg-background-primary",
      loading: false,
    },
    {
      variant: "danger",
      disabled: false,
      class: "hover:bg-background-errorSecondary",
      loading: false,
    },
  ],
  defaultVariants: {
    size: "sm",
    variant: "primary",
    focused: false,
    disabled: false,
    accent: "none",
    icon: false,
  },
});

export function buttonClasses({
  variant,
  disabled,
  inline,
  icon,
  focused,
  className,
  size,
  loading,
}: Pick<
  ButtonProps,
  | "variant"
  | "disabled"
  | "focused"
  | "className"
  | "size"
  | "inline"
  | "icon"
  | "loading"
>) {
  return variant === "unstyled"
    ? className
    : button({
        accent: inline ? "inline" : "none",
        icon: !!icon,
        variant,
        disabled,
        focused,
        className,
        size,
        loading,
      });
}
