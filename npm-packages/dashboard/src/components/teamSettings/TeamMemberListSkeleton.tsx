import { Loading } from "@ui/Loading";

export function TeamMemberListSkeleton() {
  return (
    <>
      {[...Array(3).keys()].map((i) => (
        <Loading key={i} className="mx-auto w-full p-4">
          <div className="flex items-center space-x-4">
            <div className="flex-1 space-y-2 py-1">
              <div className="h-2 w-32 rounded-sm bg-neutral-8/30 dark:bg-neutral-3/20" />
              <div className="h-2 w-24 rounded-sm bg-neutral-8/30 dark:bg-neutral-3/20" />
            </div>
            <div className="ml-auto h-2 w-10 rounded-sm bg-neutral-8/30 dark:bg-neutral-3/20" />
          </div>
        </Loading>
      ))}
    </>
  );
}
