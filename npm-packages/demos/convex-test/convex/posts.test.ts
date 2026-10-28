import { convexTest } from "convex-test";
import { describe, it, expect } from "vitest";
import { api, internal } from "./_generated/api";
import schema from "./schema";

describe("posts.list", () => {
  it("returns empty array when no posts exist", async () => {
    const t = convexTest(schema, modules);

    // Initially, there are no posts, so `list` returns an empty array
    const posts = await t.query(api.posts.list);
    expect(posts).toEqual([]);
  });

  it("returns all posts ordered by creation time when there are posts", async () => {
    const t = convexTest(schema, modules);

    // Create some posts
    await t.mutation(internal.posts.add, {
      title: "First Post",
      content: "This is the first post",
      author: "Alice",
    });
    await t.mutation(internal.posts.add, {
      title: "Second Post",
      content: "This is the second post",
      author: "Bob",
    });

    // `list` returns all posts ordered by creation time
    const posts = await t.query(api.posts.list);
    expect(posts).toHaveLength(2);
    expect(posts[0].title).toBe("Second Post");
    expect(posts[1].title).toBe("First Post");
  });
});

const modules = import.meta.glob("./**/*.ts");
