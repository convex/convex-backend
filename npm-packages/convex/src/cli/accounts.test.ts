/**
 * End-to-end tests for logging in to several Convex accounts, run against the
 * built CLI (`dist/cli.bundle.cjs`) and a fake Convex API on localhost.
 *
 * Each test gets its own HOME and project directory. The CLI is pointed at
 * the fake API with CONVEX_PROVISION_HOST, which also moves its config to
 * `~/.convex-test-<port>/config.json`. `--override-access-token` stands in for
 * the browser login. Commands run without a terminal, so prompts either take
 * their non-interactive path or fail, as they would for an agent or in CI.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  test,
} from "vitest";
import { execFile, execFileSync } from "child_process";
import fs from "fs";
import http from "http";
import os from "os";
import path from "path";

const PACKAGE_DIR = path.resolve(__dirname, "../..");
// Set CONVEX_CLI_UNDER_TEST to a published CLI's `bin/main.js` to check that
// the "existing single-account behavior" tests describe released behavior.
const CLI =
  process.env.CONVEX_CLI_UNDER_TEST ?? path.join(PACKAGE_DIR, "bin/main.js");

type Team = { id: number; slug: string; name: string };
type Member = { id: number; email: string; name: string; teams: Team[] };

/** A small stand-in for the Convex API with just what login needs. */
class FakeConvex {
  members = new Map<number, Member>();
  // Browser-login tokens, as returned by the device flow.
  loginTokens = new Map<string, number>();
  // Personal access tokens: secret -> id and owner.
  pats = new Map<string, { id: number; member: number }>();
  // Deployment name -> team slug and project slug.
  deployments = new Map<string, { team: string; project: string }>();
  // Deploy key secret (after the `|`) -> deployment name.
  deployKeys = new Map<string, string>();
  revoked: string[] = [];
  created: string[] = [];
  private nextPatId = 1;
  private server: http.Server | null = null;
  port = 0;

  addMember(member: Member, loginToken: string) {
    this.members.set(member.id, member);
    this.loginTokens.set(loginToken, member.id);
  }

  /** A personal access token saved on disk before the test starts. */
  issuePat(memberId: number, secret: string) {
    this.pats.set(secret, { id: this.nextPatId++, member: memberId });
    return secret;
  }

  reset() {
    this.members.clear();
    this.loginTokens.clear();
    this.pats.clear();
    this.deployments.clear();
    this.deployKeys.clear();
    this.revoked = [];
    this.created = [];
    this.nextPatId = 1;
  }

  private memberFor(header: string | undefined): number | null {
    const token = header?.replace(/^Bearer /, "") ?? "";
    return this.loginTokens.get(token) ?? this.pats.get(token)?.member ?? null;
  }

  async start() {
    this.server = http.createServer((req, res) => {
      let body = "";
      req.on("data", (chunk) => (body += chunk));
      req.on("end", () => this.handle(req, res, body));
    });
    await new Promise<void>((resolve) =>
      this.server!.listen(0, "127.0.0.1", () => resolve()),
    );
    this.port = (this.server!.address() as { port: number }).port;
  }

  async stop() {
    await new Promise<void>((resolve) => this.server!.close(() => resolve()));
  }

  private handle(
    req: http.IncomingMessage,
    res: http.ServerResponse,
    body: string,
  ) {
    const url = new URL(req.url!.replace(/\/{2,}/g, "/"), "http://x");
    const route = `${req.method} ${url.pathname}`;
    const auth = req.headers.authorization;
    const member = this.memberFor(auth);
    const json = (status: number, data: unknown) => {
      res.writeHead(status, { "Content-Type": "application/json" });
      res.end(JSON.stringify(data));
    };
    const token = auth?.replace(/^Bearer /, "") ?? "";

    if (route === "HEAD /api/authorize") {
      const ok = this.pats.has(token) || this.deployKeys.has(token);
      res.writeHead(ok ? 200 : 401);
      return res.end();
    }
    if (route === "GET /api/dashboard/profile") {
      // Like Convex: only the browser-login token can read the profile.
      const id = this.loginTokens.get(token);
      if (id === undefined) return json(401, { code: "Unauthorized" });
      const m = this.members.get(id)!;
      return json(200, { id: m.id, email: m.email, name: m.name });
    }
    if (route === "POST /api/check_opt_ins") {
      return json(200, { optInsToAccept: [] });
    }
    if (route === "GET /api/teams") {
      if (member === null) return json(401, { code: "Unauthorized" });
      return json(200, this.members.get(member)!.teams);
    }
    if (route === "POST /v1/create_personal_access_token") {
      const id = this.loginTokens.get(token);
      if (id === undefined) return json(401, { code: "Unauthorized" });
      const secret = `pat-${id}-${this.nextPatId}`;
      this.issuePat(id, secret);
      this.created.push(secret);
      return json(200, { accessToken: secret });
    }
    if (route === "GET /v1/list_personal_access_tokens") {
      if (member === null) return json(401, { code: "Unauthorized" });
      const items = [...this.pats.values()]
        .filter((pat) => pat.member === member)
        .map((pat) => ({ id: pat.id, name: "device", creationTime: 0 }));
      return json(200, { items, pagination: { hasMore: false } });
    }
    if (route === "POST /v1/delete_personal_access_token") {
      const { id } = JSON.parse(body);
      const pat = this.pats.get(id);
      if (member === null || pat === undefined || pat.member !== member) {
        return json(404, { code: "NotFound" });
      }
      this.pats.delete(id);
      this.revoked.push(id);
      return json(200, {});
    }
    const deployment = url.pathname.match(/^\/v1\/deployments\/([^/]+)$/);
    if (req.method === "GET" && deployment) {
      return this.canAccess(member, deployment[1])
        ? json(200, { name: deployment[1] })
        : json(404, { code: "DeploymentNotFound" });
    }
    const teamAndProject = url.pathname.match(
      /^\/api\/deployment\/([^/]+)\/team_and_project$/,
    );
    if (req.method === "GET" && teamAndProject) {
      const d = this.deployments.get(teamAndProject[1]);
      return d && this.canAccess(member, teamAndProject[1])
        ? json(200, { team: d.team, project: d.project })
        : json(404, {
            code: "DeploymentNotFound",
            message: "Deployment not found",
          });
    }
    return json(404, { code: "NotFound", message: `No fake for ${route}` });
  }

  private canAccess(member: number | null, deploymentName: string) {
    const d = this.deployments.get(deploymentName);
    if (member === null || d === undefined) return false;
    return this.members.get(member)!.teams.some((t) => t.slug === d.team);
  }
}

const fake = new FakeConvex();
let home: string;
let project: string;

const IMARA: Member = {
  id: 230405,
  email: "udo@imara.ae",
  name: "Udo",
  teams: [{ id: 1, slug: "imara", name: "Imara" }],
};
const CWC: Member = {
  id: 482122,
  email: "udo.oji@cushwake.ae",
  name: "Udo",
  teams: [{ id: 2, slug: "cwc", name: "CWC" }],
};

function configPath() {
  return path.join(home, `.convex-test-${fake.port}`, "config.json");
}
function writeConfig(config: unknown) {
  fs.mkdirSync(path.dirname(configPath()), { recursive: true });
  fs.writeFileSync(configPath(), JSON.stringify(config, null, 2));
}
function readConfig(): any {
  return fs.existsSync(configPath())
    ? JSON.parse(fs.readFileSync(configPath(), "utf8"))
    : null;
}
function writeEnvLocal(contents: string, dir = project) {
  fs.writeFileSync(path.join(dir, ".env.local"), contents);
}

type Result = { code: number; output: string };
function convex(args: string[], cwd = project): Promise<Result> {
  return new Promise((resolve) => {
    execFile(
      process.execPath,
      [CLI, ...args],
      {
        cwd,
        env: {
          PATH: process.env.PATH,
          HOME: home,
          CONVEX_PROVISION_HOST: `http://127.0.0.1:${fake.port}`,
          FORCE_COLOR: "0",
          NO_COLOR: "1",
          CONVEX_AGENT_MODE: undefined,
        },
        timeout: 30_000,
      },
      (error, stdout, stderr) => {
        resolve({
          code: error ? ((error as any).code ?? 1) : 0,
          output: `${stdout}${stderr}`,
        });
      },
    );
  });
}

function login(...args: string[]) {
  return convex(["login", "--device-name", "test", ...args]);
}

beforeAll(async () => {
  // Build the CLI bundle the tests run.
  if (process.env.CONVEX_CLI_UNDER_TEST === undefined)
    execFileSync(
      process.execPath,
      ["scripts/build.cjs", "standalone-cli", "tempDir=dist"],
      { cwd: PACKAGE_DIR, stdio: "ignore" },
    );
  await fake.start();
}, 120_000);

afterAll(async () => {
  await fake.stop();
});

afterEach(() => {
  fs.rmSync(home, { recursive: true, force: true });
  fs.rmSync(project, { recursive: true, force: true });
});

beforeEach(() => {
  fake.reset();
  fake.addMember(IMARA, "login-imara");
  fake.addMember(CWC, "login-cwc");
  fake.deployments.set("befitting-hippopotamus-227", {
    team: "imara",
    project: "convex-rose-battery",
  });
  fake.deployments.set("robust-ferret-84", {
    team: "cwc",
    project: "cwc-website",
  });
  home = fs.mkdtempSync(path.join(os.tmpdir(), "convex-home-"));
  project = fs.mkdtempSync(path.join(os.tmpdir(), "convex-project-"));
  fs.writeFileSync(path.join(project, "package.json"), "{}");
});

describe("existing single-account behavior", () => {
  test("login saves one top-level token, as before", async () => {
    const result = await login("--override-access-token", "login-imara");
    expect(result.code).toBe(0);
    expect(result.output).toContain("Saved credentials to");
    expect(readConfig()).toEqual({ accessToken: fake.created[0] });
  });

  test("login with a valid token doesn't log in again", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-old") });
    const result = await login("--override-access-token", "login-imara");
    expect(result.output).toContain("previously been authorized");
    expect(fake.created).toEqual([]);
  });

  test("login with a revoked token logs in and replaces it", async () => {
    writeConfig({ accessToken: "pat-revoked" });
    const result = await login("--override-access-token", "login-imara");
    expect(result.code).toBe(0);
    expect(readConfig()).toEqual({ accessToken: fake.created[0] });
  });

  test("single-token login never revokes anything", async () => {
    writeConfig({ accessToken: "pat-revoked" });
    await login("--force", "--override-access-token", "login-imara");
    expect(fake.revoked).toEqual([]);
  });

  test("login status reports the account and its teams", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-imara") });
    const result = await convex(["login", "status"]);
    expect(result.output).toContain("Status: Logged in");
    expect(result.output).toContain("Imara (imara)");
  });

  test("logout deletes the config", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-imara") });
    const result = await convex(["logout"]);
    expect(result.code).toBe(0);
    expect(readConfig()).toBeNull();
  });

  test("properties the CLI doesn't know survive a login", async () => {
    writeConfig({ accessToken: "pat-revoked", someFutureField: 1 });
    await login("--override-access-token", "login-imara");
    expect(readConfig().someFutureField).toBe(1);
  });
});

describe("deploy keys", () => {
  beforeEach(() => {
    fake.deployKeys.set("secret", "befitting-hippopotamus-227");
  });

  test("a deploy key doesn't make login loop (the original bug)", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-imara") });
    writeEnvLocal("CONVEX_DEPLOY_KEY=dev:befitting-hippopotamus-227|secret\n");
    const result = await login("--override-access-token", "login-imara");
    expect(result.output).toContain("previously been authorized");
    expect(fake.created).toEqual([]);
  });

  test("login shows the override banner for a dev key", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-imara") });
    writeEnvLocal("CONVEX_DEPLOY_KEY=dev:befitting-hippopotamus-227|secret\n");
    const result = await login();
    expect(result.output).toContain(
      "WARNING: CONVEX_DEPLOY_KEY OVERRIDES YOUR CONVEX LOGIN IN THIS DIRECTORY",
    );
    expect(result.output).toContain(
      'dev deployment "befitting-hippopotamus-227" (valid)',
    );
  });

  test("the banner calls out a production key", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-imara") });
    writeEnvLocal("CONVEX_DEPLOY_KEY=prod:oh-dear-123|nope\n");
    const result = await login();
    expect(result.output).toContain('PRODUCTION deployment "oh-dear-123"');
    expect(result.output).toContain("(invalid or expired)");
    expect(result.output).toContain("change production");
  });

  test("other commands print a one-line warning when logged in", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-imara") });
    writeEnvLocal("CONVEX_DEPLOY_KEY=prod:oh-dear-123|nope\n");
    const result = await convex(["env", "list"]);
    expect(result.output).toContain(
      'WARNING: CONVEX_DEPLOY_KEY from .env.local overrides your Convex login: this command uses the PRODUCTION deployment "oh-dear-123"',
    );
  });

  test("a machine with only a deploy key (CI) gets no warning", async () => {
    writeEnvLocal("CONVEX_DEPLOY_KEY=prod:oh-dear-123|nope\n");
    const result = await convex(["env", "list"]);
    expect(result.output).not.toContain("WARNING");
  });
});

describe("login --account", () => {
  test("saves a new login under its member id with email and teams", async () => {
    const result = await login(
      "--account",
      "--yes",
      "--override-access-token",
      "login-cwc",
    );
    expect(result.code).toBe(0);
    expect(result.output).toContain("Logged in as udo.oji@cushwake.ae");
    const config = readConfig();
    expect(config.accounts["482122"]).toEqual({
      accessToken: fake.created[0],
      email: "udo.oji@cushwake.ae",
      description: "cwc",
      isDefault: true,
    });
    // Older CLIs read the top-level token: it mirrors the default account.
    expect(config.accessToken).toBe(fake.created[0]);
  });

  test("rejects a value that isn't a member id", async () => {
    const result = await login("--account", "list");
    expect(result.code).not.toBe(0);
    expect(result.output).toContain("--account takes a Convex member id");
    expect(fake.created).toEqual([]);
  });

  test("without a terminal or --yes, lists accounts and asks for an id", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-imara") });
    const result = await login("--account");
    expect(result.code).not.toBe(0);
    expect(result.output).toContain("Pass --account <member-id>");
    expect(fake.created).toEqual([]);
  });

  test("with an id, prints only what concerns that id", async () => {
    writeConfig({
      accessToken: "pat-a",
      accounts: {
        "230405": { accessToken: "pat-a", isDefault: true },
        "482122": { accessToken: "pat-revoked", email: CWC.email },
      },
    });
    fake.issuePat(IMARA.id, "pat-a");
    const result = await login(
      "--account",
      "482122",
      "--yes",
      "--override-access-token",
      "login-cwc",
    );
    expect(result.output).toContain(
      "Member 482122 <udo.oji@cushwake.ae>: token revoked or expired, logging in again.",
    );
    expect(result.output).not.toContain("Default account:");
    expect(result.output).not.toContain("Convex accounts on this device");
  });

  test("an account whose token works isn't logged in again", async () => {
    writeConfig({
      accessToken: "pat-a",
      accounts: { "230405": { accessToken: "pat-a", isDefault: true } },
    });
    fake.issuePat(IMARA.id, "pat-a");
    const result = await login("--account", "230405");
    expect(result.output).toContain("Already logged in as member 230405");
    expect(fake.created).toEqual([]);
  });

  test("logging in again as a saved member refreshes it, no duplicate", async () => {
    writeConfig({
      accessToken: "pat-a",
      accounts: { "230405": { accessToken: "pat-a", isDefault: true } },
    });
    fake.issuePat(IMARA.id, "pat-a");
    const result = await login(
      "--account",
      "--yes",
      "--override-access-token",
      "login-imara",
    );
    expect(result.output).toContain("Member 230405 is already saved");
    const config = readConfig();
    expect(Object.keys(config.accounts)).toEqual(["230405"]);
    expect(config.accounts["230405"].accessToken).toBe(fake.created[0]);
  });

  test("a login as a different member than --account asked for is saved separately", async () => {
    writeConfig({
      accessToken: "pat-a",
      accounts: { "230405": { accessToken: "pat-revoked", isDefault: true } },
    });
    const result = await login(
      "--account",
      "230405",
      "--yes",
      "--override-access-token",
      "login-cwc",
    );
    expect(result.output).toContain(
      "This login is member 482122, not member 230405",
    );
    const config = readConfig();
    expect(config.accounts["230405"].accessToken).toBe("pat-revoked");
    expect(config.accounts["482122"].accessToken).toBe(fake.created[0]);
  });

  test("a plain login never overwrites another member's token", async () => {
    writeConfig({
      accessToken: "pat-a",
      accounts: { "230405": { accessToken: "pat-revoked", isDefault: true } },
      instances: { [project]: { account: "230405" } },
    });
    await login("--yes", "--override-access-token", "login-cwc");
    const config = readConfig();
    expect(config.accounts["230405"].accessToken).toBe("pat-revoked");
    expect(config.accounts["482122"].accessToken).toBe(fake.created[0]);
  });
});

describe("tokens saved before member ids were recorded", () => {
  test("a login identifies an old token of the same member and files it under the member", async () => {
    // The old token and the new one differ, but both belong to IMARA.
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-old") });
    const result = await login(
      "--account",
      "--yes",
      "--override-access-token",
      "login-imara",
    );
    expect(result.output).toContain(
      'Identified the "unidentified" token: it belongs to member 230405',
    );
    const config = readConfig();
    expect(Object.keys(config.accounts)).toEqual(["230405"]);
    expect(config.accounts["230405"].isDefault).toBe(true);
  });

  test("a login as another member leaves the old token unidentified", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-old") });
    await login("--account", "--yes", "--override-access-token", "login-cwc");
    const config = readConfig();
    expect(Object.keys(config.accounts).sort()).toEqual([
      "482122",
      "unidentified",
    ]);
    expect(config.accounts.unidentified.isDefault).toBe(true);
  });

  test("account list identifies an old token without logging in", async () => {
    writeConfig({
      accessToken: "pat-old",
      accounts: {
        unidentified: { accessToken: "pat-old", isDefault: true },
        "230405": { accessToken: "pat-new", email: IMARA.email },
      },
      instances: { [project]: { account: "unidentified" } },
    });
    fake.issuePat(IMARA.id, "pat-old");
    fake.issuePat(IMARA.id, "pat-new");
    const result = await convex(["account", "list"]);
    expect(result.output).toContain(
      'Identified the "unidentified" token: it belongs to member 230405 <udo@imara.ae>',
    );
    const config = readConfig();
    expect(Object.keys(config.accounts)).toEqual(["230405"]);
    expect(config.accounts["230405"].isDefault).toBe(true);
    // The directory bound to the old token now uses the member.
    expect(config.instances[project]).toEqual({ account: "230405" });
  });

  test("a revoked old token isn't merged into whoever logs in (no terminal)", async () => {
    writeConfig({ accessToken: "pat-revoked" });
    await login(
      "--account",
      "unidentified",
      "--yes",
      "--override-access-token",
      "login-cwc",
    );
    const config = readConfig();
    expect(config.accounts.unidentified.accessToken).toBe("pat-revoked");
    expect(config.accounts["482122"]).toBeDefined();
  });

  test("login --account unidentified logs in even though the token works", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-old") });
    const result = await login(
      "--account",
      "unidentified",
      "--yes",
      "--override-access-token",
      "login-imara",
    );
    expect(result.output).toContain("its member isn't recorded");
    expect(fake.created).toHaveLength(1);
    expect(Object.keys(readConfig().accounts)).toEqual(["230405"]);
  });
});

describe("revoking replaced tokens", () => {
  const savedImara = () => ({
    accessToken: "pat-a",
    accounts: { "230405": { accessToken: "pat-a", isDefault: true } },
  });

  test("a refreshed account's old token is revoked by default", async () => {
    writeConfig(savedImara());
    fake.issuePat(IMARA.id, "pat-a");
    const result = await login(
      "--account",
      "230405",
      "--force",
      "--yes",
      "--override-access-token",
      "login-imara",
    );
    expect(fake.revoked).toEqual(["pat-a"]);
    expect(result.output).toContain("Revoked 1 replaced token on Convex");
  });

  test("nothing is revoked when the setting is off", async () => {
    writeConfig(savedImara());
    fake.issuePat(IMARA.id, "pat-a");
    await convex(["account", "settings", "revoke-replaced-tokens", "false"]);
    const result = await login(
      "--account",
      "230405",
      "--force",
      "--yes",
      "--override-access-token",
      "login-imara",
    );
    expect(fake.revoked).toEqual([]);
    expect(result.output).toContain("revoke-replaced-tokens is off");
  });

  test("an identified old token is revoked once merged", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-old") });
    await login("--account", "--yes", "--override-access-token", "login-imara");
    expect(fake.revoked).toEqual(["pat-old"]);
  });

  test("another member's token is never revoked", async () => {
    writeConfig({ accessToken: fake.issuePat(IMARA.id, "pat-old") });
    await login("--account", "--yes", "--override-access-token", "login-cwc");
    expect(fake.revoked).toEqual([]);
    expect(fake.pats.has("pat-old")).toBe(true);
  });

  test("settings shows the default and rejects bad values", async () => {
    writeConfig(savedImara());
    expect((await convex(["account", "settings"])).output).toContain(
      "revoke-replaced-tokens = true (default)",
    );
    const bad = await convex([
      "account",
      "settings",
      "revoke-replaced-tokens",
      "maybe",
    ]);
    expect(bad.code).not.toBe(0);
    const unknown = await convex(["account", "settings", "nope", "true"]);
    expect(unknown.output).toContain('Unknown setting "nope"');
  });
});

describe("binding directories", () => {
  const twoAccounts = () => {
    fake.issuePat(IMARA.id, "pat-a");
    fake.issuePat(CWC.id, "pat-c");
    return {
      accessToken: "pat-a",
      accounts: {
        "230405": { accessToken: "pat-a", isDefault: true, email: IMARA.email },
        "482122": { accessToken: "pat-c", email: CWC.email },
      },
    };
  };

  test("account use refuses an account that can't reach CONVEX_DEPLOYMENT", async () => {
    writeConfig(twoAccounts());
    writeEnvLocal("CONVEX_DEPLOYMENT=dev:robust-ferret-84 # team: cwc\n");
    const result = await convex(["account", "use", "230405"]);
    expect(result.output).toContain(
      `can't access the deployment "robust-ferret-84"`,
    );
    expect(readConfig().instances ?? {}).toEqual({});
  });

  test("account use binds an account that can reach it", async () => {
    writeConfig(twoAccounts());
    writeEnvLocal("CONVEX_DEPLOYMENT=dev:robust-ferret-84\n");
    const result = await convex(["account", "use", "482122"]);
    expect(result.code).toBe(0);
    expect(readConfig().instances).toEqual({
      [fs.realpathSync(project)]: { account: "482122" },
    });
  });

  test("account use --force skips the check", async () => {
    writeConfig(twoAccounts());
    writeEnvLocal("CONVEX_DEPLOYMENT=dev:robust-ferret-84\n");
    await convex(["account", "use", "230405", "--force"]);
    expect(Object.values(readConfig().instances)).toEqual([
      { account: "230405" },
    ]);
  });

  test("a revoked account can't be checked, so it isn't bound", async () => {
    const config = twoAccounts();
    config.accounts["482122"].accessToken = "pat-revoked";
    writeConfig(config);
    writeEnvLocal("CONVEX_DEPLOYMENT=dev:robust-ferret-84\n");
    const result = await convex(["account", "use", "482122"]);
    expect(result.output).toContain("its token doesn't work");
    expect(readConfig().instances ?? {}).toEqual({});
  });

  test("login --yes doesn't move a directory off its account", async () => {
    const config = { ...twoAccounts(), instances: {} as any };
    config.instances[fs.realpathSync(project)] = { account: "482122" };
    writeConfig(config);
    await login("--account", "--yes", "--override-access-token", "login-imara");
    expect(readConfig().instances[fs.realpathSync(project)]).toEqual({
      account: "482122",
    });
  });

  test("subdirectories use the account of the closest bound parent", async () => {
    const config = { ...twoAccounts(), instances: {} as any };
    config.instances[fs.realpathSync(project)] = { account: "482122" };
    writeConfig(config);
    const sub = path.join(project, "convex");
    fs.mkdirSync(sub);
    const result = await convex(["account", "list"], sub);
    expect(result.output).toContain(
      "This directory: member 482122 <udo.oji@cushwake.ae>",
    );
  });

  test("unbind falls back to the default account", async () => {
    const config = { ...twoAccounts(), instances: {} as any };
    config.instances[fs.realpathSync(project)] = { account: "482122" };
    writeConfig(config);
    const result = await convex(["account", "unbind"]);
    expect(result.output).toContain("it now uses the default account");
    expect(readConfig().instances).toEqual({});
  });

  test("binding the account already in use as default says so", async () => {
    writeConfig(twoAccounts());
    const result = await convex(["account", "use", "230405"]);
    expect(result.output).toContain(
      "It already used this account as the default",
    );
  });

  test("unknown member ids are rejected", async () => {
    writeConfig(twoAccounts());
    const result = await convex(["account", "use", "999"]);
    expect(result.code).not.toBe(0);
    expect(result.output).toContain('No logged-in account for member "999"');
  });
});

describe("account list", () => {
  test("names the default account and checks every token", async () => {
    fake.issuePat(IMARA.id, "pat-a");
    writeConfig({
      accessToken: "pat-a",
      accounts: {
        "230405": { accessToken: "pat-a", isDefault: true, email: IMARA.email },
        "482122": { accessToken: "pat-revoked", email: CWC.email },
      },
    });
    const result = await convex(["account", "list"]);
    expect(result.output).toContain(
      "Default account: member 230405 <udo@imara.ae>",
    );
    expect(result.output).toContain(
      "member 230405 <udo@imara.ae> (default) - logged in",
    );
    expect(result.output).toContain(
      "member 482122 <udo.oji@cushwake.ae> - token revoked or expired",
    );
  });

  test("warns when this directory's account can't reach its deployment", async () => {
    fake.issuePat(IMARA.id, "pat-a");
    writeConfig({
      accessToken: "pat-a",
      accounts: { "230405": { accessToken: "pat-a", isDefault: true } },
    });
    writeEnvLocal("CONVEX_DEPLOYMENT=dev:robust-ferret-84\n");
    const result = await convex(["account", "list"]);
    expect(result.output).toContain(
      `can't access the deployment "robust-ferret-84"`,
    );
  });

  test("reads configs that marked the default with isPrimary", async () => {
    fake.issuePat(IMARA.id, "pat-a");
    writeConfig({
      accessToken: "pat-a",
      accounts: { "230405": { accessToken: "pat-a", isPrimary: true } },
    });
    const result = await convex(["account", "list"]);
    expect(result.output).toContain("Default account: member 230405");
  });
});

describe("logout", () => {
  const twoAccounts = {
    accessToken: "pat-a",
    accounts: {
      "230405": { accessToken: "pat-a", isDefault: true },
      "482122": { accessToken: "pat-c" },
    },
  };

  test("--account removes one account and promotes a new default", async () => {
    writeConfig(twoAccounts);
    const result = await convex(["logout", "--account", "230405"]);
    expect(result.output).toContain("Member 482122 is now the default account");
    const config = readConfig();
    expect(Object.keys(config.accounts)).toEqual(["482122"]);
    expect(config.accessToken).toBe("pat-c");
  });

  test("without a terminal, asks for --account or --all", async () => {
    writeConfig(twoAccounts);
    const result = await convex(["logout"]);
    expect(result.code).not.toBe(0);
    expect(result.output).toContain("Pass --account <member-id> or --all");
    expect(readConfig()).not.toBeNull();
  });

  test("--all deletes the config", async () => {
    writeConfig(twoAccounts);
    await convex(["logout", "--all"]);
    expect(readConfig()).toBeNull();
  });

  test("logging out of the last account deletes the config", async () => {
    writeConfig({
      accessToken: "pat-a",
      accounts: { "230405": { accessToken: "pat-a", isDefault: true } },
    });
    await convex(["logout", "--account", "230405"]);
    expect(readConfig()).toBeNull();
  });
});

describe("dev with a project another account owns", () => {
  test("without a terminal, explains and suggests the saved account that can reach it", async () => {
    fake.issuePat(IMARA.id, "pat-a");
    fake.issuePat(CWC.id, "pat-c");
    writeConfig({
      accessToken: "pat-a",
      accounts: {
        "230405": { accessToken: "pat-a", isDefault: true, email: IMARA.email },
        "482122": { accessToken: "pat-c", email: CWC.email },
      },
    });
    writeEnvLocal(
      "CONVEX_DEPLOYMENT=dev:robust-ferret-84 # team: cwc, project: cwc-website\n",
    );
    fs.writeFileSync(
      path.join(project, "package.json"),
      JSON.stringify({ dependencies: { convex: "*" } }),
    );
    const result = await convex(["dev", "--once"]);
    expect(result.code).not.toBe(0);
    expect(result.output).toContain(
      'This directory is set up for the deployment "robust-ferret-84" (team: cwc, project: cwc-website)',
    );
    expect(result.output).toContain(
      "member 230405 <udo@imara.ae> can't access it",
    );
    expect(result.output).toContain("npx convex account use 482122");
    // .env.local is left alone.
    expect(fs.readFileSync(path.join(project, ".env.local"), "utf8")).toContain(
      "robust-ferret-84",
    );
  });
});
