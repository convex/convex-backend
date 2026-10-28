import { useCallback, useState } from "react";
import type { PalettePage } from "./pages";

export type DrillStack = {
  pages: PalettePage[];
  search: string;
  setSearch: (search: string) => void;
  pushPage: (page: PalettePage) => void;
  popPage: () => void;
  // Jump back to a given depth: 0 returns to the root, n keeps the first n
  // pages.
  goToDepth: (depth: number) => void;
  resetTo: (pages: PalettePage[]) => void;
};

export function useDrillStack({
  initialPages = [],
  afterNavigate,
}: {
  initialPages?: PalettePage[];
  afterNavigate?: () => void;
} = {}): DrillStack {
  const [pages, setPages] = useState<PalettePage[]>(initialPages);
  const [search, setSearch] = useState("");
  const [searchAtDepth, setSearchAtDepth] = useState<string[]>([]);

  const pushPage = useCallback(
    (page: PalettePage) => {
      setPages((current) => [...current, page]);
      setSearchAtDepth((current) => [...current, search]);
      setSearch("");
      afterNavigate?.();
    },
    [search, afterNavigate],
  );

  const goToDepth = useCallback(
    (depth: number) => {
      setPages((current) => current.slice(0, depth));
      setSearchAtDepth((current) => current.slice(0, depth));
      setSearch(searchAtDepth[depth] ?? "");
      afterNavigate?.();
    },
    [searchAtDepth, afterNavigate],
  );

  const popPage = useCallback(
    () => goToDepth(pages.length - 1),
    [goToDepth, pages.length],
  );

  const resetTo = useCallback((newPages: PalettePage[]) => {
    setPages(newPages);
    setSearchAtDepth([]);
    setSearch("");
  }, []);

  return { pages, search, setSearch, pushPage, popPage, goToDepth, resetTo };
}
