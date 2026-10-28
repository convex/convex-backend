import { createFileRoute } from '@tanstack/react-router';
import { handleCallbackRoute } from '@workos/authkit-tanstack-react-start';

export const Route = createFileRoute('/callback')({
  server: {
    handlers: {
      GET: handleCallbackRoute(),
    },
  },
});
