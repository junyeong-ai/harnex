import { type ChildProcess, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { expect, test } from "@playwright/test";

const HARNEX = process.env.HARNEX_BIN ?? resolve(import.meta.dirname, "../../../../target/debug/harnex");

const ASKS = {
  locale: "ko",
  sources: ["decision.md"],
  asks: [
    {
      id: "d-1",
      label: "결정 1",
      version: "v1",
      answers: [
        { name: "지금", note: "none" },
        { name: "나중", note: "optional" },
      ],
    },
    {
      id: "d-2",
      label: "설계",
      version: "v2",
      answers: [
        { name: "동의", note: "none" },
        { name: "고칠 곳", note: "required" },
      ],
    },
  ],
};

/** What the page's own script records of the transport's events. */
const LISTENER = `<script>
  window.heard = [];
  document.addEventListener("ask-ready", (e) => heard.push(["ready", e.detail.asks.asks.length]));
  document.addEventListener("ask-closed", (e) => heard.push(["closed", e.detail.answered]));
</script>`;

const FIELDS = `
<fieldset data-ask="d-1" data-version="v1"><legend>결정 1</legend>
  <label><input type="radio" name="d-1" value="지금" disabled>지금</label>
  <label><input type="radio" name="d-1" value="나중" disabled>나중</label>
  <div data-ask-note="나중" hidden><input type="text" disabled></div>
</fieldset>
<fieldset data-ask="d-2" data-version="v2"><legend>설계</legend>
  <label><input type="radio" name="d-2" value="동의" disabled>동의</label>
  <label><input type="radio" name="d-2" value="고칠 곳" disabled>고칠 곳</label>
  <div data-ask-note="고칠 곳" hidden><textarea disabled></textarea></div>
</fieldset>`;

const page = (body: string) =>
  `<!doctype html><html lang="ko"><head><meta charset="utf-8">
<link rel="stylesheet" href="styles/page.css">${LISTENER}</head>
<body>${body}<div data-ask-send>여기서는 고를 수 없다.</div></body></html>`;

interface Served {
  readonly url: string;
  readonly dir: string;
  readonly child: ChildProcess;
  readonly exit: Promise<{ code: number | null; envelope: any }>;
}

/** Every command a test started, ended after it so a failing test leaves none serving. */
const running: ChildProcess[] = [];
test.afterEach(() => {
  for (const child of running.splice(0)) child.kill();
});

/** Start `harnex ask serve --no-open` over these files; resolves once it announces its address. */
async function serve(
  html: string,
  { asks = ASKS, name = "page.html" }: { asks?: object; name?: string } = {},
): Promise<Served> {
  const dir = mkdtempSync(join(tmpdir(), "ask-page-"));
  const files: Record<string, string> = {
    [name]: html,
    "styles/page.css": "body { background-color: rgb(1, 2, 3); }",
    "asks.json": JSON.stringify(asks),
    "decision.md": "# 결정\n",
  };
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, path)), { recursive: true });
    writeFileSync(join(dir, path), text);
  }
  const child = spawn(HARNEX, ["ask", "serve", name, "asks.json", "--no-open"], { cwd: dir });
  running.push(child);
  let stdout = "";
  child.stdout?.on("data", (chunk) => {
    stdout += chunk;
  });
  const exit = new Promise<{ code: number | null; envelope: any }>((settle) =>
    child.on("close", (code) => settle({ code, envelope: stdout === "" ? null : JSON.parse(stdout) })),
  );
  const url = await new Promise<string>((found, failed) => {
    let stderr = "";
    child.stderr?.on("data", (chunk) => {
      stderr += chunk;
      const line = /^serving (\S+) for \d+ minutes$/m.exec(stderr);
      if (line) found(line[1]);
    });
    child.on("close", () => failed(new Error(`harnex exited before serving: ${stderr}`)));
  });
  return { url, dir, child, exit };
}

test("a served page turns on, keeps each note with its answer, and sends", async ({ page: tab }) => {
  const served = await serve(page(FIELDS));
  await tab.goto(served.url);

  const send = tab.locator(".ask-send");
  await expect(send).toHaveText("보내기");
  await expect(tab.locator("[data-ask-send]")).not.toContainText("여기서는 고를 수 없다");
  await expect(tab.locator(".ask-progress")).toHaveText("답한 것 0 / 2");
  await expect(tab.locator(".ask-until")).toHaveText(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}까지 답을 받는다$/);
  await expect(tab.locator("body")).toHaveCSS("background-color", "rgb(1, 2, 3)");
  expect(await tab.evaluate(() => (window as any).heard)).toEqual([["ready", 2]]);

  await tab.getByLabel("고칠 곳").check();
  await expect(tab.locator("[data-ask-note='고칠 곳']")).toBeVisible();
  await expect(tab.locator("[data-ask-note='나중']")).toBeHidden();
  await expect(tab.locator(".ask-progress")).toHaveText("답한 것 1 / 2");

  await send.click();
  await expect(tab.locator(".ask-status")).toHaveText("설계: '고칠 곳'에는 적을 것이 있다.");
  await expect(send).toBeEnabled();

  await tab.locator("[data-ask-note='고칠 곳'] textarea").fill("  3번 칸  ");
  await tab.getByLabel("지금").check();
  await send.click();
  await expect(tab.locator(".ask-status")).toHaveText("보냈다. 세션이 답을 받았다. 이 창은 닫아도 된다.");
  await expect(tab.getByLabel("지금")).toBeDisabled();
  await expect(send).toBeDisabled();
  expect(await tab.evaluate(() => (window as any).heard)).toEqual([
    ["ready", 2],
    ["closed", true],
  ]);

  const { code, envelope } = await served.exit;
  expect(code).toBe(0);
  expect(envelope.data.outcome).toBe("answered");
  expect(envelope.data.answers.map((a: any) => [a.id, a.answer, a.note])).toEqual([
    ["d-1", "지금", null],
    ["d-2", "고칠 곳", "3번 칸"],
  ]);
});

test("a page that breaks the promise says so and sends nothing", async ({ page: tab }) => {
  const broken = FIELDS.replace('data-version="v1"', 'data-version="v0"').replace(
    '<div data-ask-note="고칠 곳" hidden><textarea disabled></textarea></div>',
    "",
  );
  const served = await serve(page(broken));
  await tab.goto(served.url);

  const status = tab.locator(".ask-status");
  await expect(status).toContainText("결정 1: 페이지가 보인 판과 묻는 판이 다르다.");
  await expect(status).toContainText("설계: '고칠 곳'에 적을 칸이 없다.");
  await expect(tab.locator(".ask-send")).toBeDisabled();
  await expect(tab.getByLabel("지금")).toBeDisabled();
  expect(await tab.evaluate(() => (window as any).heard)).toEqual([]);
});

test("a page under a strict content security policy turns on and sends", async ({ page: tab }) => {
  const strict = `<!doctype html><html lang="ko"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self'; connect-src 'self'">
</head><body>${FIELDS}<div data-ask-send>여기서는 고를 수 없다.</div></body></html>`;
  const served = await serve(strict);
  await tab.goto(served.url);

  await tab.getByLabel("동의").check();
  await tab.locator(".ask-send").click();
  await expect(tab.locator(".ask-status")).toHaveText("보냈다. 세션이 답을 받았다. 이 창은 닫아도 된다.");
  expect((await served.exit).code).toBe(0);
});

test("a source changed while the page was open closes it as stale", async ({ page: tab }) => {
  const served = await serve(page(FIELDS));
  await tab.goto(served.url);
  writeFileSync(join(served.dir, "decision.md"), "# 결정 (고침)\n");

  await tab.getByLabel("동의").check();
  await tab.locator(".ask-send").click();
  await expect(tab.locator(".ask-status")).toHaveText(
    "이 페이지를 연 뒤 원천 문서가 바뀌어서 이 답은 받지 않았다. 세션에 그렇게 알렸다.",
  );
  await expect(tab.getByLabel("동의")).toBeDisabled();
  expect(await tab.evaluate(() => (window as any).heard)).toEqual([
    ["ready", 2],
    ["closed", false],
  ]);

  const { code, envelope } = await served.exit;
  expect(code).toBe(1);
  expect(envelope.data.outcome).toBe("stale");
});

test("a send after the session stopped waiting says the session was not reached", async ({ page: tab }) => {
  const served = await serve(page(FIELDS));
  await tab.goto(served.url);
  served.child.kill();
  await served.exit;

  await tab.getByLabel("동의").check();
  await tab.locator(".ask-send").click();
  await expect(tab.locator(".ask-status")).toHaveText(
    "세션에 닿지 않았다. 답을 기다리던 명령이 끝났을 수 있다.",
  );
  await expect(tab.locator(".ask-send")).toBeEnabled();
});

test.describe("in a browser set to another locale and time zone", () => {
  test.use({ locale: "ko-KR", timezoneId: "America/Los_Angeles" });

  test("a page named in any script is served, and speaks the asks file's locale", async ({ page: tab }) => {
    const served = await serve(page(FIELDS), { asks: { ...ASKS, locale: "en" }, name: "결정 페이지.html" });
    await tab.goto(served.url);

    await expect(tab.locator(".ask-send")).toHaveText("Send");
    await expect(tab.locator(".ask-progress")).toHaveText("Answered 0 / 2");
    const script = await (await tab.request.get(served.url.replace(/\/page\/[^/]*$/, "/ask.js"))).text();
    const { deadline } = JSON.parse(/^const ASK = (.*);$/m.exec(script)![1]);
    const at = Object.fromEntries(
      new Intl.DateTimeFormat("en-CA", {
        timeZone: "America/Los_Angeles",
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        hourCycle: "h23",
      })
        .formatToParts(new Date(deadline))
        .map((part) => [part.type, part.value]),
    );
    await expect(tab.locator(".ask-until")).toHaveText(
      `Taking answers until ${at.year}-${at.month}-${at.day} ${at.hour}:${at.minute}`,
    );
    await expect(tab.locator("body")).toHaveCSS("background-color", "rgb(1, 2, 3)");

    await tab.getByLabel("동의").check();
    await tab.locator(".ask-send").click();
    await expect(tab.locator(".ask-status")).toHaveText(
      "Sent. The session has the answers, and this window can be closed.",
    );
    expect((await served.exit).code).toBe(0);
  });
});
