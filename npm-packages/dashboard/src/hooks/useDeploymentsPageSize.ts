import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";

export function useDeploymentsPageSize() {
  const [deploymentsPageSize, setDeploymentsPageSize] = useGlobalLocalStorage(
    "deploymentsPageSize",
    25,
  );
  return { pageSize: deploymentsPageSize, setPageSize: setDeploymentsPageSize };
}

export const DEPLOYMENT_PAGE_SIZES = [
  { label: "10", value: 10 },
  { label: "25", value: 25 },
  { label: "50", value: 50 },
  { label: "100", value: 100 },
];
