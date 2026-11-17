import {
  ActionBuilder,
  DataModelFromSchemaDefinition,
  GenericDocument,
  GenericMutationCtx,
  GenericQueryCtx,
  MutationBuilder,
  QueryBuilder,
  anyApi,
  defineSchema,
  defineTable,
} from "convex/server";
import { GenericId, v } from "convex/values";
/**
 * See the comment at the top of ./index.ts for more details on the
 * goals of these snippets + some strategies for writing syntactically
 * correct code while glossing over some details.
 */

const schema = defineSchema({
  conversations: defineTable({
    members: v.array(v.id("users")),
    summary: v.optional(v.string()),
  }),
  users: defineTable({
    name: v.string(),
  }),
  messages: defineTable({
    conversation: v.id("conversations"),
    author: v.id("users"),
    content: v.string(),
  }),
});
type DataModel = DataModelFromSchemaDefinition<typeof schema>;

declare const OMIT_ME: any;

declare const query: QueryBuilder<DataModel, "public">;
declare const action: ActionBuilder<DataModel, "public">;
declare const internalMutation: MutationBuilder<DataModel, "internal">;
declare const internalQuery: QueryBuilder<DataModel, "internal">;

const api = anyApi;
const internal = anyApi;

type QueryCtx = GenericQueryCtx<DataModel>;
type MutationCtx = GenericMutationCtx<DataModel>;

type Doc<T extends keyof DataModel> = DataModel[T]["document"];
type Id<T extends keyof DataModel> = GenericId<T>;

// @snippet start usersWrong
export const getCurrentUser_OMIT_1 = query({
  args: {},
  handler: async (ctx) => {
    const userIdentity = await ctx.auth.getUserIdentity();
    if (userIdentity === null) {
      throw new Error("Unauthorized");
    }
    const user = /* query ctx.db to load the user */ OMIT_ME;
    const userSettings = /* load other documents related to the user */ OMIT_ME;
    return { user, settings: userSettings };
  },
});
// @snippet end usersWrong

// @snippet start conversationsWrong
export const listMessages_OMIT_1 = query({
  args: {
    conversationId: v.id("conversations"),
  },
  handler: async (ctx, { conversationId }) => {
    const user = await ctx.runQuery(api.users.getCurrentUser);
    const conversation = await ctx.db.get("conversations", conversationId);
    if (conversation === null || !conversation.members.includes(user._id)) {
      throw new Error("Unauthorized");
    }
    const messages = /* query ctx.db to load the messages */ OMIT_ME;
    return messages;
  },
});

export const summarizeConversation_OMIT_1 = action({
  args: {
    conversationId: v.id("conversations"),
  },
  handler: async (ctx, { conversationId }) => {
    const messages = await ctx.runQuery(api.conversations.listMessages, {
      conversationId,
    });
    // @skipNextLine
    /* prettier-ignore */
    const summary = /* call some external service to summarize the conversation */ OMIT_ME;
    await ctx.runMutation(api.conversations.addSummary, {
      conversationId,
      summary,
    });
  },
});
// @snippet end conversationsWrong

// @snippet start usersCorrect
export async function getCurrentUser(ctx: QueryCtx) {
  const userIdentity = await ctx.auth.getUserIdentity();
  if (userIdentity === null) {
    throw new Error("Unauthorized");
  }
  const user = /* query ctx.db to load the user */ OMIT_ME;
  const userSettings = /* load other documents related to the user */ OMIT_ME;
  return { user, settings: userSettings };
}
// @snippet end usersCorrect

declare const Users: {
  getCurrentUser: (ctx: QueryCtx) => Promise<Doc<"users">>;
};

// @snippet start conversationsModelCorrect
export async function ensureHasAccess(
  ctx: QueryCtx,
  { conversationId }: { conversationId: Id<"conversations"> },
) {
  const user = await Users.getCurrentUser(ctx);
  const conversation = await ctx.db.get("conversations", conversationId);
  if (conversation === null || !conversation.members.includes(user._id)) {
    throw new Error("Unauthorized");
  }
  return conversation;
}

export async function listMessages_OMIT_2(
  ctx: QueryCtx,
  { conversationId }: { conversationId: Id<"conversations"> },
) {
  await ensureHasAccess(ctx, { conversationId });
  const messages = /* query ctx.db to load the messages */ OMIT_ME;
  return messages;
}

export async function addSummary_OMIT_1(
  ctx: MutationCtx,
  {
    conversationId,
    summary,
  }: { conversationId: Id<"conversations">; summary: string },
) {
  await ensureHasAccess(ctx, { conversationId });
  await ctx.db.patch("conversations", conversationId, { summary });
}

export async function generateSummary(
  messages: Doc<"messages">[],
  conversationId: Id<"conversations">,
) {
  // @skipNextLine
  /* prettier-ignore */
  const summary = /* call some external service to summarize the conversation */ OMIT_ME;
  return summary;
}
// @snippet end conversationsModelCorrect

declare const Conversations: {
  addSummary: (
    ctx: MutationCtx,
    {
      conversationId,
      summary,
    }: { conversationId: Id<"conversations">; summary: string },
  ) => Promise<void>;
  listMessages: (
    ctx: QueryCtx,
    { conversationId }: { conversationId: Id<"conversations"> },
  ) => Promise<Doc<"messages">[]>;
  generateSummary: (
    messages: Doc<"messages">[],
    conversationId: Id<"conversations">,
  ) => Promise<string>;
};

// @snippet start conversationsApiCorrect
export const addSummary = internalMutation({
  args: {
    conversationId: v.id("conversations"),
    summary: v.string(),
  },
  handler: async (ctx, { conversationId, summary }) => {
    await Conversations.addSummary(ctx, { conversationId, summary });
  },
});

export const listMessages = internalQuery({
  args: {
    conversationId: v.id("conversations"),
  },
  handler: async (ctx, { conversationId }) => {
    return Conversations.listMessages(ctx, { conversationId });
  },
});

export const summarizeConversation = action({
  args: {
    conversationId: v.id("conversations"),
  },
  handler: async (ctx, { conversationId }) => {
    const messages = await ctx.runQuery(internal.conversations.listMessages, {
      conversationId,
    });
    const summary = await Conversations.generateSummary(
      messages,
      conversationId,
    );
    await ctx.runMutation(internal.conversations.addSummary, {
      conversationId,
      summary,
    });
  },
});
// @snippet end conversationsApiCorrect
