import { useRouter } from "next/router";
import { useCallback, useMemo } from "react";
import { validateProjectTag } from "components/projects/projectTags";

export function useProjectTagFilter(): [string[], (tags: string[]) => void] {
  const router = useRouter();
  const tagParam = router.query.tag;
  const tags = useMemo(
    () =>
      [tagParam ?? []]
        .flat()
        .reduce<string[]>(
          (valid, tag) =>
            validateProjectTag(tag.trim(), valid) === undefined
              ? [...valid, tag.trim()]
              : valid,
          [],
        )
        .slice(0, 1),
    [tagParam],
  );
  const setTags = useCallback(
    (newTags: string[]) => {
      void router.replace(
        { pathname: router.pathname, query: { ...router.query, tag: newTags } },
        undefined,
        { shallow: true },
      );
    },
    [router],
  );
  return [tags, setTags];
}
