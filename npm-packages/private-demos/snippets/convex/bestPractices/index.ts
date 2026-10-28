import { GenericId, v } from "convex/values";
import { env } from "../_generated/server";
import {
  ActionBuilder,
  anyApi,
  Crons,
  DataModelFromSchemaDefinition,
  defineSchema,
  defineTable,
  DocumentByName,
  GenericActionCtx,
  GenericMutationCtx,
  GenericQueryCtx,
  MutationBuilder,
  PaginationOptions,
  QueryBuilder,
  SystemTableNames,
  TableNamesInDataModel,
} from "convex/server";

/**
 * The snippets in our best practices guide are a little less
 * rigorous than most of our snippets. They're more about illustrating
 * "right" and "wrong" patterns side by side than providing complete
 * code that can be copy-pasted and immediately run.
 *
 * We're more comfortable omitting import statements or glossing over
 * portions of functions in these snippets than elsewhere.
 *
 * However we still want to write these in TypeScript so we write syntactically
 * correct code (it's very easy to make mistakes in markdown).
 *
 * When changing things here, check that the "Best practices" page in
 * docs still looks correct.
 *
 * A few tricks to write syntactically valid code while glossing over details:
 * - Use `declare const` to declare variables we're using without actually needing
 * to give them a value
 * - Use blocks + `// @skipNextLine` to allow using the same `const` name
 * twice for side by side examples within the same snippet
 * - Use `foo_OMIT_1` + `foo_OMIT_2` with the `replacements` option on the
 * snippet to use the same function name twice (especially for exported functions)
 * - Use `/* do X *\/ OMIT_ME` + the `replacements` option on the snippet to
 * avoid writing out details.
 */

const schema = defineSchema({
  messages: defineTable({
    author: v.string(),
    body: v.string(),
  }).index("by_author", ["author"]),
  movies: defineTable({
    title: v.string(),
    description: v.optional(v.string()),
    votes: v.number(),
    director: v.string(),
  }).index("by_director", ["director"]),
  watchedMovies: defineTable({
    user: v.string(),
  }).index("by_user", ["user"]),
  watchedMoviesCount: defineTable({
    user: v.string(),
  }).index("by_user", ["user"]),
  teamMembers: defineTable({
    team: v.string(),
    user: v.string(),
  })
    // eslint-disable-next-line @convex-dev/no-duplicate-indexes
    .index("by_team", ["team"])
    .index("by_team_and_user", ["team", "user"]),
  teams: defineTable({
    name: v.string(),
    owner: v.string(),
  }),
  failures: defineTable({
    kind: v.string(),
    body: v.string(),
    author: v.string(),
    error: v.string(),
  }),
  posts: defineTable({
    releasedAt: v.number(),
    isReleased: v.boolean(),
  })
    .index("by_released_at", ["releasedAt"])
    .index("by_is_released", ["isReleased"]),
});
type DataModel = DataModelFromSchemaDefinition<typeof schema>;

type QueryCtx = GenericQueryCtx<DataModel>;
type MutationCtx = GenericMutationCtx<DataModel>;
type ActionCtx = GenericActionCtx<DataModel>;

declare const ctx: MutationCtx;

declare const internalMutation: MutationBuilder<DataModel, "internal">;
declare const internalQuery: QueryBuilder<DataModel, "internal">;
declare const action: ActionBuilder<DataModel, "public">;
declare const mutation: MutationBuilder<DataModel, "public">;

declare const crons: Crons;

const internal = anyApi;
const api = anyApi;

export type TableNames = TableNamesInDataModel<DataModel>;
export type Doc<TableName extends TableNames> = DocumentByName<
  DataModel,
  TableName
>;
export type Id<TableName extends TableNames | SystemTableNames> =
  GenericId<TableName>;

declare const OMIT_ME: any;

// @snippet start filter
// @skipNextLine
{
  // ❌
  const tomsMessages = ctx.db
    .query("messages")
    // @skipNextLine
    // eslint-disable-next-line @convex-dev/no-filter-in-query -- snippet intentionally demonstrates an inefficient pattern
    .filter((q) => q.eq(q.field("author"), "Tom"))
    .collect();
  // @skipNextLine
}

// @skipNextLine
{
  // ✅
  // Option 1: Use an index
  const tomsMessages = await ctx.db
    .query("messages")
    .withIndex("by_author", (q) => q.eq("author", "Tom"))
    .collect();
  // @skipNextLine
}

// @skipNextLine
{
  // Option 2: Filter in code
  const allMessages = await ctx.db.query("messages").collect();
  const tomsMessages = allMessages.filter((m) => m.author === "Tom");
  // @skipNextLine
}
// @snippet end filter

declare const paginationOptions: PaginationOptions;

// @snippet start collectIndex
// @skipNextLine
{
  // ❌ -- potentially unbounded
  const allMovies = await ctx.db.query("movies").collect();
  const moviesByDirector = allMovies.filter(
    (m) => m.director === "Steven Spielberg",
  );
  // @skipNextLine
}

// @skipNextLine
{
  // ✅ -- small number of results, so `collect` is fine
  const moviesByDirector = await ctx.db
    .query("movies")
    .withIndex("by_director", (q) => q.eq("director", "Steven Spielberg"))
    .collect();
  // @skipNextLine
}
// @snippet end collectIndex

// @snippet start collectPaginate
// @skipNextLine
{
  // ❌ -- potentially unbounded
  const watchedMovies = await ctx.db
    .query("watchedMovies")
    .withIndex("by_user", (q) => q.eq("user", "Tom"))
    .collect();
  // @skipNextLine
}

// @skipNextLine
{
  // ✅ -- using pagination, showing recently watched movies first
  const watchedMovies = await ctx.db
    .query("watchedMovies")
    .withIndex("by_user", (q) => q.eq("user", "Tom"))
    .order("desc")
    .paginate(paginationOptions);
  // @skipNextLine
}
// @snippet end collectPaginate

// @snippet start collectCount
// @skipNextLine
{
  // ❌ -- potentially unbounded
  const watchedMovies = await ctx.db
    .query("watchedMovies")
    .withIndex("by_user", (q) => q.eq("user", "Tom"))
    .collect();
  const numberOfWatchedMovies = watchedMovies.length;
  // @skipNextLine
}

// @skipNextLine
{
  // ✅ -- Show "99+" instead of needing to load all documents
  const watchedMovies = await ctx.db
    .query("watchedMovies")
    .withIndex("by_user", (q) => q.eq("user", "Tom"))
    .take(100);
  const numberOfWatchedMovies =
    watchedMovies.length === 100 ? "99+" : watchedMovies.length.toString();
  // @skipNextLine
}

// @skipNextLine
{
  // ✅ -- Denormalize the number of watched movies in a separate table
  const watchedMoviesCount = await ctx.db
    .query("watchedMoviesCount")
    .withIndex("by_user", (q) => q.eq("user", "Tom"))
    .unique();
  // @skipNextLine
}
// @snippet end collectCount

declare const teamId: GenericId<"teams">;

// @snippet start redundantIndexes
// @skipNextLine
{
  // ❌
  const allTeamMembers = await ctx.db
    .query("teamMembers")
    .withIndex("by_team", (q) => q.eq("team", teamId))
    .collect();
  const currentUserId = /* get current user id from `ctx.auth` */ OMIT_ME;
  const currentTeamMember = await ctx.db
    .query("teamMembers")
    .withIndex("by_team_and_user", (q) =>
      q.eq("team", teamId).eq("user", currentUserId),
    )
    .unique();
  // @skipNextLine
}

// @skipNextLine
{
  // ✅
  // Just don't include a condition on `user` when querying for results on `team`
  const allTeamMembers = await ctx.db
    .query("teamMembers")
    .withIndex("by_team_and_user", (q) => q.eq("team", teamId))
    .collect();
  const currentUserId = /* get current user id from `ctx.auth` */ OMIT_ME;
  const currentTeamMember = await ctx.db
    .query("teamMembers")
    .withIndex("by_team_and_user", (q) =>
      q.eq("team", teamId).eq("user", currentUserId),
    )
    .unique();
  // @skipNextLine
}
// @snippet end redundantIndexes

// @snippet start validation
// ❌ -- `id` and `update` are not validated, so a client could pass
//       any Convex value (the type at runtime could mismatch the
//       TypeScript type). In particular, `update` could contain
//       fields other than `title` and `director`.
// @skipNextLine
// eslint-disable-next-line @convex-dev/require-args-validator
export const updateMovie_OMIT_1 = mutation({
  handler: async (
    ctx,
    {
      id,
      update,
    }: {
      id: Id<"movies">;
      update: Pick<Doc<"movies">, "title" | "director">;
    },
  ) => {
    await ctx.db.patch("movies", id, update);
  },
});

// ✅ -- This can only be called with an ID from the movies table,
//       and an `update` object with only the `title`/`director` fields
export const updateMovie_OMIT_2 = mutation({
  args: {
    id: v.id("movies"),
    update: v.object({
      title: v.string(),
      director: v.string(),
    }),
  },
  handler: async (ctx, { id, update }) => {
    await ctx.db.patch("movies", id, update);
  },
});
// @snippet end validation

type TeamMember = {
  email: string;
};
// @snippet start accessControl
// ❌ -- no checks! anyone can update any team if they get the ID
export const updateTeam_OMIT_1 = mutation({
  args: {
    id: v.id("teams"),
    update: v.object({
      name: v.optional(v.string()),
      owner: v.optional(v.id("users")),
    }),
  },
  handler: async (ctx, { id, update }) => {
    await ctx.db.patch("teams", id, update);
  },
});

// ❌ -- checks access, but uses `email` which could be spoofed
export const updateTeam_OMIT_2 = mutation({
  args: {
    id: v.id("teams"),
    update: v.object({
      name: v.optional(v.string()),
      owner: v.optional(v.id("users")),
    }),
    email: v.string(),
  },
  handler: async (ctx, { id, update, email }) => {
    const teamMembers = /* load team members */ OMIT_ME as TeamMember[];
    if (!teamMembers.some((m) => m.email === email)) {
      throw new Error("Unauthorized");
    }
    await ctx.db.patch("teams", id, update);
  },
});

// ✅ -- checks access, and uses `ctx.auth`, which cannot be spoofed
export const updateTeam = mutation({
  args: {
    id: v.id("teams"),
    update: v.object({
      name: v.optional(v.string()),
      owner: v.optional(v.id("users")),
    }),
  },
  handler: async (ctx, { id, update }) => {
    const user = await ctx.auth.getUserIdentity();
    if (user === null) {
      throw new Error("Unauthorized");
    }
    const isTeamMember = /* check if user is a member of the team */ OMIT_ME;
    if (!isTeamMember) {
      throw new Error("Unauthorized");
    }
    await ctx.db.patch("teams", id, update);
  },
});

// ✅ -- separate functions which have different access control
export const setTeamOwner = mutation({
  args: {
    id: v.id("teams"),
    owner: v.id("users"),
  },
  handler: async (ctx, { id, owner }) => {
    const user = await ctx.auth.getUserIdentity();
    if (user === null) {
      throw new Error("Unauthorized");
    }
    const isTeamOwner = /* check if user is the owner of the team */ OMIT_ME;
    if (!isTeamOwner) {
      throw new Error("Unauthorized");
    }
    await ctx.db.patch("teams", id, { owner: owner });
  },
});

export const setTeamName = mutation({
  args: {
    id: v.id("teams"),
    name: v.string(),
  },
  handler: async (ctx, { id, name }) => {
    const user = await ctx.auth.getUserIdentity();
    if (user === null) {
      throw new Error("Unauthorized");
    }
    const isTeamMember = /* check if user is a member of the team */ OMIT_ME;
    if (!isTeamMember) {
      throw new Error("Unauthorized");
    }
    await ctx.db.patch("teams", id, { name: name });
  },
});
// @snippet end accessControl

// @snippet start internal
// ❌ -- using `api`
export const sendMessage_OMIT_1 = mutation({
  args: {
    body: v.string(),
    author: v.string(),
  },
  handler: async (ctx, { body, author }) => {
    // add message to the database
  },
});

// crons.ts
crons.daily(
  "send daily reminder",
  { hourUTC: 17, minuteUTC: 30 },
  api.messages.sendMessage,
  { author: "System", body: "Share your daily update!" },
);

// ✅ Using `internal`
// REPLACE_WITH_MUTATION_CTX_IMPORT
async function sendMessageHelper(
  ctx: MutationCtx,
  args: { body: string; author: string },
) {
  // add message to the database
}

export const sendMessage_OMIT_2 = mutation({
  args: {
    body: v.string(),
  },
  handler: async (ctx, { body }) => {
    const user = await ctx.auth.getUserIdentity();
    if (user === null) {
      throw new Error("Unauthorized");
    }
    await sendMessageHelper(ctx, { body, author: user.name ?? "Anonymous" });
  },
});

export const sendInternalMessage = internalMutation({
  args: {
    body: v.string(),
    // don't need to worry about `author` being spoofed since this is an internal function
    author: v.string(),
  },
  handler: async (ctx, { body, author }) => {
    await sendMessageHelper(ctx, { body, author });
  },
});

// crons.ts
crons.daily(
  "send daily reminder",
  { hourUTC: 17, minuteUTC: 30 },
  internal.messages.sendInternalMessage,
  { author: "System", body: "Share your daily update!" },
);
// @snippet end internal

// @snippet start runAction
// ❌ -- using `runAction`
export const scrapeWebsite_OMIT_1 = action({
  args: {
    siteMapUrl: v.string(),
  },
  handler: async (ctx, { siteMapUrl }) => {
    const siteMap = await fetch(siteMapUrl);
    const pages = /* parse the site map */ OMIT_ME as string[];
    await Promise.all(
      pages.map((page) =>
        ctx.runAction(internal.scrape.scrapeSinglePage, { url: page }),
      ),
    );
  },
});
// @snippet end runAction

// @snippet start scrapeModel
// ✅ -- using a plain TypeScript function
export async function scrapeSinglePage(
  ctx: ActionCtx,
  { url }: { url: string },
) {
  const page = await fetch(url);
  const text = /* parse the page */ OMIT_ME;
  await ctx.runMutation(internal.scrape.addPage, { url, text });
}
// @snippet end scrapeModel

declare const Scrape: {
  scrapeSinglePage: (ctx: ActionCtx, { url }: { url: string }) => Promise<void>;
};
// @snippet start scrapeAction
export const scrapeWebsite_OMIT_2 = action({
  args: {
    siteMapUrl: v.string(),
  },
  handler: async (ctx, { siteMapUrl }) => {
    const siteMap = await fetch(siteMapUrl);
    const pages = /* parse the site map */ OMIT_ME as string[];
    await Promise.all(
      pages.map((page) => Scrape.scrapeSinglePage(ctx, { url: page })),
    );
  },
});
// @snippet end scrapeAction

declare const assert: (condition: boolean) => void;

// @snippet start runQueryWrong
// ❌ -- this assertion could fail if the team changed between running the two queries
const team = await ctx.runQuery(internal.teams.getTeam, { teamId });
const teamOwner = await ctx.runQuery(internal.teams.getTeamOwner, { teamId });
assert(team.owner === teamOwner._id);
// @snippet end runQueryWrong

declare const Teams: {
  load: (
    ctx: QueryCtx,
    { teamId }: { teamId: GenericId<"teams"> },
  ) => Promise<{ owner: GenericId<"users"> }>;
};
declare const Users: {
  load: (
    ctx: QueryCtx,
    { userId }: { userId: GenericId<"users"> },
  ) => Promise<{ _id: GenericId<"users"> }>;
  insert: (
    ctx: MutationCtx,
    { name, email }: { name: string; email: string },
  ) => Promise<void>;
};

// @snippet start runQueryCorrect
export const sendBillingReminder = action({
  args: {
    teamId: v.id("teams"),
  },
  handler: async (ctx, { teamId }) => {
    // ✅ -- this will always pass
    const teamAndOwner = await ctx.runQuery(internal.teams.getTeamAndOwner, {
      teamId,
    });
    assert(teamAndOwner.team.owner === teamAndOwner.owner._id);
    // send a billing reminder email to the owner
  },
});

export const getTeamAndOwner = internalQuery({
  args: {
    teamId: v.id("teams"),
  },
  handler: async (ctx, { teamId }) => {
    const team = await Teams.load(ctx, { teamId });
    const owner = await Users.load(ctx, { userId: team.owner });
    return { team, owner };
  },
});
// @snippet end runQueryCorrect

// Gets members on the team
async function fetchTeamMemberData(teamId: string) {
  return [{ name: "Alice", email: "alice@gmail.com" }];
}
// @snippet start runMutationWrong
export const importTeams_OMIT_1 = action({
  args: {
    teamId: v.id("teams"),
  },
  handler: async (ctx, { teamId }) => {
    // Fetch team members from an external API
    const teamMembers = await fetchTeamMemberData(teamId);

    // ❌ This will run a separate mutation for inserting each user,
    // which means you lose transaction guarantees like atomicity.
    for (const member of teamMembers) {
      await ctx.runMutation(internal.teams.insertUser, member);
    }
  },
});
export const insertUser = internalMutation({
  args: { name: v.string(), email: v.string() },
  handler: async (ctx, { name, email }) => {
    await Users.insert(ctx, { name, email });
  },
});
// @snippet end runMutationWrong

// @snippet start runMutationCorrect
export const importTeams_OMIT_2 = action({
  args: {
    teamId: v.id("teams"),
  },
  handler: async (ctx, { teamId }) => {
    // Fetch team members from an external API
    const teamMembers = await fetchTeamMemberData(teamId);

    // ✅ This action runs a single mutation that inserts all users in the same transaction.
    await ctx.runMutation(internal.teams.insertUsers, teamMembers);
  },
});
export const insertUsers = internalMutation({
  args: { users: v.array(v.object({ name: v.string(), email: v.string() })) },
  handler: async (ctx, { users }) => {
    for (const { name, email } of users) {
      await Users.insert(ctx, { name, email });
    }
  },
});
// @snippet end runMutationCorrect

// @snippet start partialRollback
export const trySendMessage = mutation({
  args: {
    body: v.string(),
    author: v.string(),
  },
  handler: async (ctx, { body, author }) => {
    try {
      await ctx.runMutation(internal.messages.sendMessage, { body, author });
    } catch (e) {
      // Record the failure, but rollback any writes from `sendMessage`
      await ctx.db.insert("failures", {
        kind: "MessageFailed",
        body,
        author,
        error: `Error: ${e}`,
      });
    }
  },
});
// @snippet end partialRollback

// @snippet start explicitTableIds
// @skipNextLine
declare const movieId: GenericId<"movies">;
// @skipNextLine
{
  // ❌
  // @skipNextLine
  // eslint-disable-next-line @convex-dev/explicit-table-ids
  await ctx.db.get(movieId);
  // @skipNextLine
  // eslint-disable-next-line @convex-dev/explicit-table-ids
  await ctx.db.patch(movieId, { title: "Whiplash" });
  // @skipNextLine
  // eslint-disable-next-line @convex-dev/explicit-table-ids
  await ctx.db.replace(movieId, {
    title: "Whiplash",
    director: "Damien Chazelle",
    votes: 0,
  });
  // @skipNextLine
  // eslint-disable-next-line @convex-dev/explicit-table-ids
  await ctx.db.delete(movieId);

  // ✅            vvvvvvvv
  await ctx.db.get("movies", movieId);
  await ctx.db.patch("movies", movieId, { title: "Whiplash" });
  await ctx.db.replace("movies", movieId, {
    title: "Whiplash",
    director: "Damien Chazelle",
    votes: 0,
  });
  await ctx.db.delete("movies", movieId);
  // @skipNextLine
}
// @snippet end explicitTableIds

// @snippet start dateInQueries
// @skipNextLine
{
  // ❌
  const releasedPosts = await ctx.db
    .query("posts")
    .withIndex("by_released_at", (q) => q.lte("releasedAt", Date.now()))
    .take(100);
  // @skipNextLine
}

// @skipNextLine
{
  // ✅
  const releasedPosts = await ctx.db
    .query("posts")
    // `isReleased` is set to `true` by a scheduled function after `releasedAt` is reached
    .withIndex("by_is_released", (q) => q.eq("isReleased", true))
    .take(100);
  // @skipNextLine
}
// @snippet end dateInQueries

// @snippet start typedEnv
// @skipNextLine
declare const body: string;
// ❌ The typo compiles, and reads as "not configured": notifications silently
// stop being sent
// @skipNextLine
// eslint-disable-next-line @convex-dev/no-process-env
const webhookUrl_OMIT_1 = process.env.SLACK_WEBOOK_URL;
if (webhookUrl_OMIT_1 !== undefined) {
  await fetch(webhookUrl_OMIT_1, { method: "POST", body });
}

// ✅ The typo would be a build error, and the type says the variable is optional
const webhookUrl_OMIT_2 = env.SLACK_WEBHOOK_URL;
if (webhookUrl_OMIT_2 !== undefined) {
  await fetch(webhookUrl_OMIT_2, { method: "POST", body });
}
// @snippet end typedEnv
