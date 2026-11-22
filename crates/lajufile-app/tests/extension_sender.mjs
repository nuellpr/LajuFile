import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const EXT_ID = "lajufile-test-extension-id";
const EXT_ORIGIN = `chrome-extension://${EXT_ID}/`;
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const COMMON_PATH = path.resolve(__dirname, "../../../extension/common.js");
const BG_PATH = path.resolve(__dirname, "../../../extension/background.js");
const INTERCEPTOR_PATH = path.resolve(__dirname, "../../../extension/interceptor.js");

function loadBackgroundHarness({
  rules = { excluded_domains: "", captured_extensions: "" },
  hasDeterminingFilename = true,
  searchItems = [],
} = {}) {
  const listeners = [];
  const onCreatedListeners = [];
  const onDeterminingListeners = [];
  const fetchCalls = [];
  const storageData = {
    port: 53841,
    token: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    autoCapture: true,
    excludedDomains: "",
    fileTypes: "",
  };

  const downloads = {
    onCreated: { addListener: (fn) => onCreatedListeners.push(fn) },
    cancel: (_id, cb) => cb?.(),
    removeFile: (_id, cb) => cb?.(),
    erase: (_q, cb) => cb?.(),
    pause: async () => {},
    resume: async () => {},
    search: async ({ id }) => searchItems.filter((item) => item.id === id),
  };
  if (hasDeterminingFilename) {
    downloads.onDeterminingFilename = {
      addListener: (fn) => onDeterminingListeners.push(fn),
    };
  }

  const sandbox = {
    URL,
    AbortController,
    setTimeout: (...args) => {
      const handle = setTimeout(...args);
      handle.unref?.();
      return handle;
    },
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
        return { ok: true, status: 200, json: async () => ({ version: "1.0.5" }) };
      }
      if (String(url).endsWith("/rules")) {
        return {
          ok: true,
          status: 200,
          json: async () => rules,
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
      downloads,
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

  const context = vm.createContext(sandbox);
  vm.runInContext(fs.readFileSync(COMMON_PATH, "utf8"), context, { filename: COMMON_PATH });
  vm.runInContext(fs.readFileSync(BG_PATH, "utf8"), context, { filename: BG_PATH });
  assert.equal(listeners.length, 1, "expected exactly one onMessage listener");
  return {
    onMessage: listeners[0],
    onCreated: onCreatedListeners[0],
    onDetermining: onDeterminingListeners[0],
    fetchCalls,
  };
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

test("auto-capture uses remoteCapturedExtensions when config.fileTypes is empty", async () => {
  const { onDetermining, fetchCalls } = loadBackgroundHarness({
    rules: { excluded_domains: "", captured_extensions: "zip,7z" },
  });

  await new Promise((resolve) => {
    onDetermining(
      { id: 1, state: "in_progress", url: "https://example.org/page.html", filename: "page.html" },
      resolve,
    );
  });
  assert.equal(fetchCalls.some((c) => c.url.endsWith("/downloads")), false);

  await new Promise((resolve) => {
    onDetermining(
      { id: 2, state: "in_progress", url: "https://example.org/bundle.zip", filename: "bundle.zip" },
      resolve,
    );
  });
  const dlCalls = fetchCalls.filter((c) => c.url.endsWith("/downloads"));
  assert.equal(dlCalls.length, 1);
  assert.equal(JSON.parse(dlCalls[0].init.body).url, "https://example.org/bundle.zip");
});

test("firefox onCreated captures immediately when onDeterminingFilename is absent", async () => {
  const item = {
    id: 42,
    state: "in_progress",
    url: "https://example.org/archive.zip",
    filename: "archive.zip",
  };
  const { onCreated, fetchCalls } = loadBackgroundHarness({
    hasDeterminingFilename: false,
    searchItems: [item],
  });

  onCreated(item);
  await new Promise((r) => setTimeout(r, 80));
  const dlCalls = fetchCalls.filter((c) => c.url.endsWith("/downloads"));
  assert.equal(dlCalls.length, 1);
  assert.equal(JSON.parse(dlCalls[0].init.body).url, "https://example.org/archive.zip");
});

test("onDeterminingFilename prefers finalUrl over initial redirect url", async () => {
  const { onDetermining, fetchCalls } = loadBackgroundHarness({
    rules: { excluded_domains: "", captured_extensions: "zip" },
  });

  await new Promise((resolve) => {
    onDetermining(
      {
        id: 99,
        state: "in_progress",
        url: "https://example.org/redirect-download",
        finalUrl: "https://cdn.example.org/files/release.zip?sig=abc",
        filename: "release.zip",
      },
      resolve,
    );
  });

  const dlCalls = fetchCalls.filter((c) => c.url.endsWith("/downloads"));
  assert.equal(dlCalls.length, 1);
  assert.equal(
    JSON.parse(dlCalls[0].init.body).url,
    "https://cdn.example.org/files/release.zip?sig=abc",
  );
});

test("common.js validates Quick-Pair payloads, ports, tokens, and host exclusions", () => {
  const sandbox = { URL, String, Number, Boolean, Array, JSON };
  const context = vm.createContext(sandbox);
  vm.runInContext(fs.readFileSync(COMMON_PATH, "utf8"), context, { filename: COMMON_PATH });
  const common = context.LajuFileCommon;

  const validToken = "a".repeat(64);
  const parsed = common.parseQuickPair(JSON.stringify({ port: 53841, token: validToken }));
  assert.equal(parsed.port, 53841);
  assert.equal(parsed.token, validToken);
  assert.throws(() => common.parseQuickPair("not-json"));
  assert.throws(() => common.parseQuickPair(JSON.stringify({ port: 80, token: validToken })));
  assert.throws(() => common.parseQuickPair(JSON.stringify({ port: 53841, token: "short" })));
  assert.equal(common.isHostExcluded("sub.internal.net", "*.internal.net, example.com"), true);
  assert.equal(common.isHostExcluded("other.org", "*.internal.net, example.com"), false);
  assert.equal(common.isSupportedVideoPage("https://vimeo.com/123456789"), true);
  assert.equal(common.isSupportedVideoPage("https://www.twitch.tv/videos/98765"), true);
  assert.equal(common.isSupportedVideoPage("https://evil.example/watch"), false);
});

test("video-formats supports multi-site platforms like Vimeo and capture-media requires ownPage", async () => {
  const { onMessage, fetchCalls } = loadBackgroundHarness();
  const vimeoUrl = "https://vimeo.com/123456789";

  const fromVimeo = await dispatch(
    onMessage,
    { type: "video-formats", url: vimeoUrl },
    { id: EXT_ID, tab: { id: 8, url: vimeoUrl }, url: vimeoUrl, frameId: 0 },
  );
  assert.equal(fromVimeo.ret, true);
  assert.equal(fromVimeo.reply.status, 200);

  // capture-media must reject web page senders and allow extension popup.
  const fromWeb = await dispatch(
    onMessage,
    { type: "capture-media", url: "https://cdn.example.org/stream.m3u8" },
    { id: EXT_ID, tab: { id: 9, url: "https://example.org/" }, url: "https://example.org/", frameId: 0 },
  );
  assert.equal(fromWeb.ret, false);

  const fromPopup = await dispatch(
    onMessage,
    { type: "capture-media", url: "https://cdn.example.org/stream.m3u8" },
    { id: EXT_ID, url: `${EXT_ORIGIN}popup.html` },
  );
  assert.equal(fromPopup.ret, true);
  assert.ok(fetchCalls.some((c) => c.url.endsWith("/video")));
});

function loadInterceptorHarness({ isTopFrame = true, mediaElements = [] } = {}) {
  const clickListeners = [];
  const messageListeners = [];
  const sentMessages = [];
  const winObj = {
    location: { assign: () => {} },
    open: () => ({}),
  };
  winObj.top = isTopFrame ? winObj : {};

  const sandbox = {
    URL,
    String,
    Number,
    Boolean,
    Array,
    Set,
    Object,
    JSON,
    decodeURIComponent,
    setTimeout: (...args) => {
      const handle = setTimeout(...args);
      handle.unref?.();
      return handle;
    },
    window: winObj,
    location: { href: "https://example.org/page", hostname: "example.org" },
    document: {
      addEventListener: (event, fn) => {
        if (event === "click") clickListeners.push(fn);
      },
      querySelectorAll: () => mediaElements,
    },
    chrome: {
      storage: {
        local: {
          get: async (defaults) => ({ ...defaults, autoCapture: true }),
        },
        onChanged: { addListener: () => {} },
      },
      runtime: {
        id: EXT_ID,
        sendMessage: async (msg) => {
          sentMessages.push(msg);
          return { captured: true, status: 201 };
        },
        onMessage: { addListener: (fn) => messageListeners.push(fn) },
      },
    },
  };

  const context = vm.createContext(sandbox);
  vm.runInContext(fs.readFileSync(COMMON_PATH, "utf8"), context, { filename: COMMON_PATH });
  vm.runInContext(fs.readFileSync(INTERCEPTOR_PATH, "utf8"), context, {
    filename: INTERCEPTOR_PATH,
  });
  return {
    onClick: clickListeners[0],
    onMessage: messageListeners[0],
    sentMessages,
  };
}

test("interceptor ignores synthetic untrusted clicks and intercepts trusted PDF clicks", async () => {
  const { onClick, sentMessages } = loadInterceptorHarness();
  const anchor = {
    href: "https://private.example.org/report.pdf",
    hasAttribute: () => false,
  };
  const makeEvent = (isTrusted) => {
    let prevented = false;
    return {
      isTrusted,
      button: 0,
      altKey: false,
      ctrlKey: false,
      metaKey: false,
      shiftKey: false,
      target: { closest: () => anchor },
      preventDefault: () => {
        prevented = true;
      },
      stopPropagation: () => {},
      get prevented() {
        return prevented;
      },
    };
  };

  const synthetic = makeEvent(false);
  onClick(synthetic);
  assert.equal(synthetic.prevented, false);
  assert.equal(sentMessages.length, 0);

  const trusted = makeEvent(true);
  onClick(trusted);
  assert.equal(trusted.prevented, true);
  assert.equal(sentMessages.length, 1);
  assert.equal(sentMessages[0].type, "intercept-link");
  assert.equal(sentMessages[0].url, "https://private.example.org/report.pdf");
  assert.equal(sentMessages[0].filename, undefined);

  anchor.href = "https://private.example.org/Annual%20Report.pdf";
  const encodedClick = makeEvent(true);
  onClick(encodedClick);
  assert.equal(encodedClick.prevented, true);
  assert.equal(sentMessages.length, 2);
  assert.equal(sentMessages[1].type, "intercept-link");
  assert.equal(sentMessages[1].url, "https://private.example.org/Annual%20Report.pdf");
  assert.equal(sentMessages[1].filename, undefined);
});

test("sniff-page-media survives malformed percent encoding and skips empty subframes", async () => {
  const emptySubframe = loadInterceptorHarness({ isTopFrame: false, mediaElements: [] });
  let subframeReplied = false;
  const subRet = emptySubframe.onMessage(
    { type: "sniff-page-media" },
    { id: EXT_ID },
    () => {
      subframeReplied = true;
    },
  );
  assert.equal(subRet, false);
  assert.equal(subframeReplied, false);

  const mediaFrame = loadInterceptorHarness({
    isTopFrame: false,
    mediaElements: [
      {
        tagName: "VIDEO",
        currentSrc: "https://cdn.example.org/media/clip%ZZname.mp4",
        querySelectorAll: () => [],
      },
    ],
  });
  let foundReply = null;
  mediaFrame.onMessage({ type: "sniff-page-media" }, { id: EXT_ID }, (reply) => {
    foundReply = reply;
  });
  assert.equal(foundReply?.items?.length, 1);
  assert.equal(foundReply.items[0].url, "https://cdn.example.org/media/clip%ZZname.mp4");
  assert.equal(foundReply.items[0].label, "clip%ZZname.mp4");
});
