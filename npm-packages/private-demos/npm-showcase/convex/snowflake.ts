"use node";
import { v } from "convex/values";
import { action, env } from "./_generated/server";
import snowflake from "snowflake-sdk";

export const doSqlQuery = action({
  args: {
    stmt: v.string(),
  },
  handler: async (_, { stmt }) => {
    const { SNOWFLAKE_ACCOUNT, SNOWFLAKE_USERNAME, SNOWFLAKE_PASSWORD } = env;
    if (!SNOWFLAKE_ACCOUNT || !SNOWFLAKE_USERNAME || !SNOWFLAKE_PASSWORD) {
      throw new Error(
        "Set SNOWFLAKE_ACCOUNT, SNOWFLAKE_USERNAME and SNOWFLAKE_PASSWORD to run this demo",
      );
    }
    const connection = snowflake.createConnection({
      account: SNOWFLAKE_ACCOUNT,
      username: SNOWFLAKE_USERNAME,
      password: SNOWFLAKE_PASSWORD,
      authenticator: "SNOWFLAKE",
    });

    const connPromise = new Promise((resolve, reject) => {
      connection.connect(async function (err, conn) {
        if (err) {
          throw err;
        } else {
          console.log("Successfully connected as id: " + conn.getId());
          if (!(await conn.isValidAsync())) {
            reject("connection invalid");
          }

          conn.execute({
            sqlText: stmt,
            complete: function (err, _, rows) {
              if (err) {
                throw err;
              } else {
                resolve(rows || []);
              }
            },
          });
        }
      });
    });

    return JSON.stringify(await connPromise);
  },
});
