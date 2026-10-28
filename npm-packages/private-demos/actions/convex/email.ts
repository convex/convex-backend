"use node";
import { action, env } from "./_generated/server";

// Require:
const postmark = require("postmark");

export default action(async (_, { email }: { email: string }) => {
  // test client configured by Tom, don't use in automated tests (we'll get cut off)
  const client = new postmark.ServerClient(env.POSTMARK_SERVER_TOKEN);

  const result = await client.sendEmail({
    From: "tom@convex.dev",
    To: email,
    Subject: "Hello from Postmark",
    HtmlBody: "<strong>Hello</strong> dear Convex user!",
    TextBody: `Hello from Tom!`,
    MessageStream: "outbound",
  });
  console.log("email sent...");
  console.log(result);
});
