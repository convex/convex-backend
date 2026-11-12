import { cn } from "@ui/cn";

export function ProjectTagList({
  tags,
  chipClassName,
}: {
  tags: string[];
  chipClassName?: string;
}) {
  if (tags.length === 0) {
    return null;
  }

  return (
    <ul
      aria-label="Tags"
      className="relative z-10 -mx-4 scrollbar-none flex w-fit max-w-[calc(100%+2rem)] min-w-0 cursor-pointer gap-1 overflow-x-auto mask-[linear-gradient(to_right,transparent,black_1rem,black_calc(100%-1rem),transparent)] px-4 select-none focus-visible:mask-none"
      // The tags hold no controls, so the row itself takes focus to be
      // scrollable from the keyboard; the fade lifts so the focus ring shows.
      // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- https://dequeuniversity.com/rules/axe/4.11/scrollable-region-focusable?application=axeAPI
      tabIndex={0}
    >
      {tags.map((tag) => (
        <li
          key={tag}
          className={cn(
            "h-5 shrink-0 rounded-sm bg-background-tertiary px-1.5 text-xs/5 whitespace-nowrap text-content-primary",
            chipClassName,
          )}
        >
          {tag}
        </li>
      ))}
    </ul>
  );
}
