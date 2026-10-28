import { Command, Option } from "@commander-js/extra-typings";
import { invoke } from "./executor";
import { v4 as uuidv4 } from "uuid";
import { log, setDebugLogging } from "./log";
import os from "node:os";
import http from "node:http";
import express, { Request, Response } from "express";

const DEFAULT_PORT = 3002;

async function startServer(
  listenTarget: number | { path: string },
  debug: boolean,
  tempdir: string,
) {
  setDebugLogging(debug);
  const app = express();
  app.use(express.json({ limit: "6MB" })); // 5 MiB for args (https://docs.convex.dev/production/state/limits#functions) + extra space

  // Override os.tmpdir to use the provided tempdir
  os.tmpdir = () => tempdir;
  log(`Node executor using tempdir: ${tempdir}`);

  // Add health check endpoint
  app.get("/health", (_req: Request, res: Response) => {
    res.json({ status: "ok" });
  });

  app.post("/invoke", async (req: Request, res: Response) => {
    try {
      const request = req.body;
      request.requestId = uuidv4();

      // Set up streaming response
      res.setHeader("Content-Type", "application/x-ndjson");
      res.setHeader("Transfer-Encoding", "chunked");

      await invoke(request, res);
    } catch (err: any) {
      // If we haven't written anything yet, send an error response
      if (!res.headersSent) {
        res.status(500).json({
          type: "error",
          message: err.message || "Internal server error",
        });
      } else {
        // If we've already started streaming, try to write an error line
        res.write(
          JSON.stringify({
            type: "error",
            message: err.message || "Internal server error",
          }) + "\n",
        );
      }
    } finally {
      res.end();
    }
  });

  const server = http.createServer(app);
  server.listen(listenTarget, () => {
    const addr = server.address();
    const addrStr =
      typeof addr === "object" && addr
        ? `port ${addr.port}`
        : typeof listenTarget === "object" && "path" in listenTarget
          ? `path ${listenTarget.path}`
          : String(listenTarget);
    log(`Node executor server listening on ${addrStr}`);
  });
}

const program = new Command();
program
  .name("node-executor")
  .description(
    "node-executor runs an HTTP server for executing actions locally",
  )
  .usage("command [options]")
  .option("--debug", "print debug output", false)
  .option("--port <number>", "port to listen on", DEFAULT_PORT.toString())
  .addOption(
    new Option(
      "--ipc-path <path>",
      "listen on a Unix domain socket or Windows named pipe path",
    ).conflicts(["port"]),
  )
  .option(
    "--tempdir <path>",
    "temporary directory to use for downloading code and dependencies",
    "",
  )
  .action(async (options) => {
    const listenTarget =
      options.ipcPath !== undefined
        ? { path: options.ipcPath }
        : parseInt(options.port, 10);
    await startServer(listenTarget, options.debug, options.tempdir);
  });

program.parseAsync(process.argv);
