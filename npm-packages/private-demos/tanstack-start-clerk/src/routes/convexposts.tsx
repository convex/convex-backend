import { convexQuery } from '@convex-dev/react-query'
import { useQuery, useSuspenseQuery } from '@tanstack/react-query'
import { Link, Outlet, createFileRoute } from '@tanstack/react-router'
import { api } from 'convex/_generated/api'

export const Route = createFileRoute('/convexposts')({
  loader: async ({ context }) => {
    await context.queryClient.ensureQueryData({
      ...convexQuery(api.posts.list, {}),
      gcTime: 10000,
    })
  },
  component: PostsComponent,
})

function PostsComponent() {
  const {
    data: posts,
    isPending,
    error,
  } = useQuery({
    ...convexQuery(api.posts.list, {}),
  })

  // Not server-rendered
  const { data: count } = useQuery(convexQuery(api.posts.count, {}))
  // Not server-rendered and null until authed
  const { data: profile } = useQuery(convexQuery(api.posts.profile, {}))
  // Server-rendered
  const { data: email } = useSuspenseQuery(convexQuery(api.posts.email, {}))

  if (isPending) return <>loading..</>
  if (error) return <>error..</>

  return (
    <div className="p-2 flex gap-2 flex-col">
      <div>server-rendered email: {email}</div>
      <div>client-rendered but no auth required (pops in): {count}</div>
      <div>
        {
          'client-rendered and requires auth (could be momentarily null if not protected by <Authenticated>): '
        }
        {profile?.email}
      </div>
      <ul className="list-disc pl-4">
        {[...posts, { id: 'i-do-not-exist', title: 'Non-existent Post' }].map(
          (post) => {
            return (
              <li key={post.id} className="whitespace-nowrap">
                <Link
                  to="/posts/$postId"
                  params={{
                    postId: post.id,
                  }}
                  className="block py-1 text-blue-800 hover:text-blue-600"
                  activeProps={{ className: 'text-black font-bold' }}
                >
                  <div>{post.title.substring(0, 20)}</div>
                </Link>
              </li>
            )
          },
        )}
      </ul>
      <hr />
      <Outlet />
    </div>
  )
}
