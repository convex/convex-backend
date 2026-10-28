import { Outlet, createFileRoute, redirect } from '@tanstack/react-router';
import { getAuth } from '@workos/authkit-tanstack-react-start';

export const Route = createFileRoute('/_authenticated')({
  loader: async ({ location }) => {
    const { user } = await getAuth();
    if (!user) {
      throw redirect({
        href: `/sign-in?returnPathname=${encodeURIComponent(location.pathname)}`,
      });
    }
  },
  component: () => <Outlet />,
});
