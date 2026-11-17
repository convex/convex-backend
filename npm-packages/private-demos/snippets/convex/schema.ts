import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export const LinkDoc = {
  normalizedId: v.string(),
  short: v.string(),
  long: v.string(),
  created: v.number(),
  lastEdit: v.number(),
  owner: v.string(),
};

export const LinkTable = defineTable(LinkDoc).index("by_normalizedId", [
  "normalizedId",
]);

export default defineSchema({
  tasks: defineTable({
    text: v.optional(v.string()),
    tag: v.optional(v.string()),
    status: v.optional(v.object({ archived: v.boolean() })),
    invalid: v.optional(v.boolean()),
    taskListId: v.optional(v.id("taskLists")),
    created: v.optional(v.string()),
    duration: v.optional(v.number()),
    authorId: v.optional(v.id("users")),
    result: v.optional(
      v.union(
        v.object({
          kind: v.literal("success"),
          result: v.any(),
        }),
        v.object({
          kind: v.literal("error"),
          error: v.optional(v.string()),
        }),
      ),
    ),
  })
    .index("by_task_list_id", ["taskListId"])
    .index("by_status", ["status"]),
  taskLists: defineTable({}),
  messages: defineTable({
    body: v.string(),
    author: v.optional(v.string()),
    channel: v.optional(v.id("channels")),
  }).index("by_author", ["author"]),
  changes: defineTable({
    type: v.string(),
  }),
  channels: defineTable({}),
  grades: defineTable({
    studentId: v.string(),
    subject: v.string(),
    grade: v.number(),
  }).index("by_studentId", ["studentId"]),
  events: defineTable({}),
  attendees: defineTable({
    eventId: v.id("events"),
    userId: v.id("users"),
  }).index("by_eventId", ["eventId"]),
  users: defineTable({
    name: v.optional(v.string()),
    preferencesId: v.id("preferences"),
  }),
  preferences: defineTable({
    userId: v.optional(v.id("users")),
  }),
  images: defineTable({
    storageId: v.string(),
    prompt: v.string(),
  }),
  exports: defineTable({
    storageId: v.id("_storage"),
  }),
  plans: defineTable({
    planType: v.string(),
  }),
  likes: defineTable({
    liker: v.string(),
    messageId: v.id("messages"),
  }).index("byMessageId", ["messageId"]),
  foods: defineTable({
    description: v.string(),
    cuisine: v.string(),
    embedding: v.array(v.float64()),
    mainIngredient: v.string(),
  }).vectorIndex("by_embedding", {
    vectorField: "embedding",
    dimensions: 1536,
    filterFields: ["cuisine", "mainIngredient"],
  }),
  movieEmbeddings: defineTable({
    embedding: v.array(v.float64()),
    genre: v.string(),
  }).vectorIndex("by_embedding", {
    vectorField: "embedding",
    dimensions: 1536,
    filterFields: ["genre"],
  }),
  movies: defineTable({
    title: v.string(),
    genre: v.string(),
    description: v.string(),
    votes: v.number(),
    embeddingId: v.optional(v.id("movieEmbeddings")),
  }).index("by_embedding", ["embeddingId"]),
  teams: defineTable(v.any()),
  links: LinkTable,
  stats: defineTable({
    link: v.id("links"),
    clicks: v.number(),
  }).index("byLink", ["link"]),
  numbers: defineTable({
    value: v.number(),
  }),
});
