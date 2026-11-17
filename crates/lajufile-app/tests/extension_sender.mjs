import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const EXT_ID = "lajufile-test-extension-id";
const EXT_ORIGIN = `chrome-extension://${EXT_ID}/`;
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const BG_PATH = path.resolve(__dirname, "../../../extension/background.js");

function loadBackgroundHarness() {
  const listeners = [];
  const fetchCalls = [];
  const storageData = {
    port: 53841,
    token: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    autoCapture: true,
    excludedDomains: "",
    fileTypes: "",
  };

  const sandbox = {
    URL,
    AbortController,
    setTimeout,
    clearTimeout,
    Date,
    Math,
    Set,
    Map,
    Promise,
    String,
    Number,
    Boolean,
    Array,
    JSON,
    navigator: { userAgent: "TestBrowser/1.0" },
    fetch: async (url, init) => {
      fetchCalls.push({ url: String(url), init });
      if (String(url).endsWith("/health")) {
        return { ok: true, status: 200, json: async () => ({ version: "1.0.4" }) };
      }
      if (String(url).endsWith("/rules")) {
        return {
          ok: true,
          status: 200,
          json: async () => ({ excluded_domains: "", captured_extensions: "" }),
        };
      }
      if (String(url).endsWith("/video/formats")) {
        return { ok: true, status: 200, json: async () => ({ title: "Demo", formats: [] }) };
      }
      return { ok: true, status: 201, json: async () => ({ id: 1, filename: "file.zip" }) };
    },
    chrome: {
      storage: {
        local: {
          get: async (defaults) => ({ ...defaults, ...storageData }),
          set: async (patch) => Object.assign(storageData, patch),
        },
      },
      cookies: { getAll: async () => [] },
      notifications: { create: () => {} },
      downloads: {
        onDeterminingFilename: { addListener: () => {} },
        onCreated: { addListener: () => {} },
        cancel: (_id, cb) => cb?.(),
        removeFile: (_id, cb) => cb?.(),
        erase: (_q, cb) => cb?.(),
        pause: async () => {},
        resume: async () => {},
        search: async () => [],
      },
      contextMenus: {
        removeAll: (cb) => cb?.(),
        create: () => {},
        onClicked: { addListener: () => {} },
      },
      runtime: {
        id: EXT_ID,
        getURL: (p = "") => `${EXT_ORIGIN}${p}`,
        onInstalled: { addListener: () => {} },
        onMessage: {
          addListener: (fn) => listeners.push(fn),
        },
      },
    },
  };

  const code = fs.readFileSync(BG_PATH, "utf8");
  vm.runInNewContext(code, sandbox, { filename: BG_PATH });
  assert.equal(listeners.length, 1, "expected exactly one onMessage listener");
  return { onMessage: listeners[0], fetchCalls };
}

function dispatch(onMessage, message, sender) {
  return new Promise((resolve) => {
    let settled = false;
    let replyValue;
    let ret;
    ret = onMessage(message, sender, (reply) => {
      settled = true;
      replyValue = reply;
      queueMicrotask(() => resolve({ ret, reply: replyValue }));
    });
    if (ret !== true && !settled) {
      resolve({ ret, reply: undefined });
    }
  });
}

test("rejects spoofed sender.id on all runtime messages", async () => {
  const { onMessage } = loadBackgroundHarness();
  const res = await dispatch(
    onMessage,
    { type: "bypass-url", url: "https://example.com/a.zip" },
    { id: "evil-extension-id", url: "chrome-extension://evil-extension-id/popup.html" },
  );
  assert.equal(res.ret, false);
  assert.equal(res.reply, undefined);
});

test("config-updated allows own options tab and popup, rejects web page content scripts", async () => {
  const { onMessage, fetchCalls } = loadBackgroundHarness();

  // MV3 options page with open_in_tab: true populates sender.tab alongside chrome-extension:// URL.
  const fromOptionsTab = await dispatch(
    onMessage,
    { type: "config-updated" },
    { id: EXT_ID, tab: { id: 10, url: `${EXT_ORIGIN}options.html` }, url: `${EXT_ORIGIN}options.html`, frameId: 0 },
  );
  assert.equal(fromOptionsTab.reply?.ok, true);
  await new Promise((r) => setTimeout(r, 20));
  assert.ok(fetchCalls.some((c) => c.url.endsWith("/rules")));

  fetchCalls.length = 0;
  const fromPopup = await dispatch(
    onMessage,
    { type: "config-updated" },
    { id: EXT_ID, url: `${EXT_ORIGIN}popup.html` },
  );
  assert.equal(fromPopup.reply?.ok, true);
  await new Promise((r) => setTimeout(r, 20));

  fetchCalls.length = 0;
  const fromWebTab = await dispatch(
    onMessage,
    { type: "config-updated" },
    { id: EXT_ID, tab: { id: 11, url: "https://www.youtube.com/watch?v=abc" }, url: "https://www.youtube.com/watch?v=abc", frameId: 0 },
  );
  assert.equal(fromWebTab.ret, false);
  assert.equal(fromWebTab.reply, undefined);
  await new Promise((r) => setTimeout(r, 20));
  assert.equal(fetchCalls.length, 0);
});

test("video-formats allows top-level YouTube tab and rejects subframes and non-YouTube pages", async () => {
  const { onMessage } = loadBackgroundHarness();
  const ytUrl = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

  const subframe = await dispatch(
    onMessage,
    { type: "video-formats", url: ytUrl },
    { id: EXT_ID, tab: { id: 2, url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ" }, url: "https://www.youtube.com/embed/foo", frameId: 1 },
  );
  assert.equal(subframe.ret, false);

  const nonYt = await dispatch(
    onMessage,
    { type: "video-formats", url: ytUrl },
    { id: EXT_ID, tab: { id: 3, url: "https://evil.example/" }, url: "https://evil.example/", frameId: 0 },
  );
  assert.equal(nonYt.ret, false);

  const topYt = await dispatch(
    onMessage,
    { type: "video-formats", url: ytUrl },
    { id: EXT_ID, tab: { id: 4, url: ytUrl }, url: ytUrl, frameId: 0 },
  );
  assert.equal(topYt.ret, true);
  assert.equal(topYt.reply.status, 200);
});

test("intercept-link and bypass-url reject unsafe URL schemes and require valid senders", async () => {
  const { onMessage } = loadBackgroundHarness();

  const fileIntercept = await dispatch(
    onMessage,
    { type: "intercept-link", url: "file:///C:/Windows/win.ini", referrer: "https://example.org/" },
    { id: EXT_ID, tab: { id: 5, url: "https://example.org/" }, url: "https://example.org/", frameId: 0 },
  );
  assert.equal(fileIntercept.ret, false);

  const jsBypass = await dispatch(
    onMessage,
    { type: "bypass-url", url: "javascript:alert(1)" },
    { id: EXT_ID, tab: { id: 5, url: "https://example.org/" }, url: "https://example.org/", frameId: 0 },
  );
  assert.equal(jsBypass.ret, false);

  const validBypass = await dispatch(
    onMessage,
    { type: "bypass-url", url: "https://example.org/file.zip" },
    { id: EXT_ID, tab: { id: 5, url: "https://example.org/" }, url: "https://example.org/", frameId: 0 },
  );
  assert.equal(validBypass.reply?.ok, true);
});

test("video-download omits non-finite size instead of serializing Infinity/null", async () => {
  const { onMessage, fetchCalls } = loadBackgroundHarness();
  const ytUrl = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
  const res = await dispatch(
    onMessage,
    { type: "video-download", url: ytUrl, selector: "137", size: Infinity },
    { id: EXT_ID, tab: { id: 6, url: ytUrl }, url: ytUrl, frameId: 0 },
  );
  assert.equal(res.ret, true);
  assert.equal(res.reply.status, 201);
  const videoCall = fetchCalls.find((c) => c.url.endsWith("/video"));
  assert.ok(videoCall, "expected POST /video call");
  const body = JSON.parse(videoCall.init.body);
  assert.equal("size" in body, false);
});
