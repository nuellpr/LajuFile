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

function loadInterceptorHarness({
  isTopFrame = true,
  mediaElements = [],
  imgElements = [],
  anchorElements = [],
} = {}) {
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
      querySelectorAll: (selector) => {
        if (selector === "img") return imgElements;
        if (selector === "a[href]") return anchorElements;
        return mediaElements;
      },
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
    onAssetsMessage: messageListeners[1],
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

test("auto-capture catches png, jpg, md, and extensionless files when rules are blank or legacy default", async () => {
  const legacyDefault =
    "zip rar 7z tar gz bz2 xz tgz exe msi msix apk dmg deb rpm iso img bin mp4 mkv webm avi mov flv wmv m4v mp3 m4a wav flac ogg aac opus pdf doc docx xls xlsx ppt pptx epub csv torrent";
  for (const capturedExtensions of ["", legacyDefault]) {
    const { onDetermining, fetchCalls } = loadBackgroundHarness({
      rules: { excluded_domains: "", captured_extensions: capturedExtensions },
    });
    const cases = [
      { id: 101, url: "https://github.com/nuellpr/LajuFile/raw/main/logo.png", filename: "logo.png" },
      { id: 102, url: "https://cdn.example.org/photo_2K.jpg", filename: "photo_2K.jpg" },
      { id: 103, url: "https://github.com/nuellpr/LajuFile/raw/main/qris.md", filename: "qris.md" },
      { id: 104, url: "https://example.org/download?id=99", filename: "artifact" },
    ];
    for (const item of cases) {
      await new Promise((resolve) => {
        onDetermining({ ...item, state: "in_progress" }, resolve);
      });
    }
    const dlCalls = fetchCalls.filter((c) => c.url.endsWith("/downloads"));
    assert.equal(dlCalls.length, 4);
    assert.deepEqual(
      dlCalls.map((c) => JSON.parse(c.init.body).url),
      cases.map((c) => c.url),
    );
  }
});

test("universal video-formats and video-download support TikTok, generic sites, streams, and embeds while keeping SSRF/origin guards", async () => {
  const { onMessage, fetchCalls } = loadBackgroundHarness();

  // TikTok feed item on top-level TikTok tab forwards referrer.
  const tiktokPage = "https://www.tiktok.com/foryou";
  const tiktokVideo = "https://www.tiktok.com/@creator/video/7400000111222";
  const ttFormats = await dispatch(
    onMessage,
    { type: "video-formats", url: tiktokVideo, referrer: tiktokPage },
    { id: EXT_ID, tab: { id: 10, url: tiktokPage }, url: tiktokPage, frameId: 0 },
  );
  assert.equal(ttFormats.ret, true);
  assert.equal(ttFormats.reply.status, 200);
  const ttFmtCall = fetchCalls.find((c) => c.url.endsWith("/video/formats"));
  assert.equal(JSON.parse(ttFmtCall.init.body).referrer, tiktokPage);

  // Generic website video page + video-download forwards referrer.
  const genericUrl = "https://videos.example.org/watch/clip-42";
  const genericDl = await dispatch(
    onMessage,
    { type: "video-download", url: genericUrl, selector: "bv*+ba/b", referrer: genericUrl },
    { id: EXT_ID, tab: { id: 11, url: genericUrl }, url: genericUrl, frameId: 0 },
  );
  assert.equal(genericDl.ret, true);
  assert.equal(genericDl.reply.status, 201);
  const genericCall = fetchCalls.find((c) => c.url.endsWith("/video"));
  assert.equal(JSON.parse(genericCall.init.body).referrer, genericUrl);

  // Embedded iframe HLS stream (.m3u8) is allowed and forwards iframe referrer.
  const embedFrameUrl = "https://player.embedhost.example/embed/99";
  const hlsUrl = "https://cdn-hls.othercdn.net/hls/master.m3u8?token=abc";
  const hlsRes = await dispatch(
    onMessage,
    { type: "video-formats", url: hlsUrl, referrer: embedFrameUrl },
    { id: EXT_ID, tab: { id: 12, url: "https://hostsite.example/post" }, url: embedFrameUrl, frameId: 2 },
  );
  assert.equal(hlsRes.ret, true);
  assert.equal(hlsRes.reply.status, 200);

  // Matching YouTube embed iframe can request its own video ID, mismatched one is rejected.
  const matchingEmbed = await dispatch(
    onMessage,
    { type: "video-formats", url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ" },
    { id: EXT_ID, tab: { id: 13, url: "https://blog.example/" }, url: "https://www.youtube.com/embed/dQw4w9WgXcQ", frameId: 1 },
  );
  assert.equal(matchingEmbed.ret, true);

  // Reject loopback targets and non-HTTP schemes.
  const loopback = await dispatch(
    onMessage,
    { type: "video-formats", url: "http://127.0.0.1:8080/video.mp4" },
    { id: EXT_ID, tab: { id: 14, url: "http://127.0.0.1:8080/" }, url: "http://127.0.0.1:8080/", frameId: 0 },
  );
  assert.equal(loopback.ret, false);

  const fileScheme = await dispatch(
    onMessage,
    { type: "video-download", url: "file:///C:/video.mp4", selector: "bv*+ba/b" },
    { id: EXT_ID, tab: { id: 15, url: "https://example.org/" }, url: "https://example.org/", frameId: 0 },
  );
  assert.equal(fileScheme.ret, false);
});

test("direct-video-download captures direct MP4s from pages and enforces security boundaries", async () => {
  const { onMessage, fetchCalls } = loadBackgroundHarness({
    rules: { excluded_domains: "", captured_extensions: "zip" },
  });

  const directRes = await dispatch(
    onMessage,
    {
      type: "direct-video-download",
      url: "https://cdn.example.org/videos/clip.mp4",
      filename: "My Clip.mp4",
      referrer: "https://site.example/post/1",
    },
    { id: EXT_ID, tab: { id: 20, url: "https://site.example/post/1" }, url: "https://site.example/post/1", frameId: 0 },
  );
  assert.equal(directRes.ret, true);
  assert.equal(directRes.reply.status, 201);
  const dlCall = fetchCalls.find((c) => c.url.endsWith("/downloads"));
  assert.ok(dlCall);
  const body = JSON.parse(dlCall.init.body);
  assert.equal(body.url, "https://cdn.example.org/videos/clip.mp4");
  assert.equal(body.filename, "My Clip.mp4");
  assert.equal(body.referrer, "https://site.example/post/1");

  // Reject loopback and cross-site YouTube pages on direct-video-download.
  const loopback = await dispatch(
    onMessage,
    { type: "direct-video-download", url: "http://127.0.0.1:53841/secret.mp4" },
    { id: EXT_ID, tab: { id: 21, url: "https://evil.example/" }, url: "https://evil.example/", frameId: 0 },
  );
  assert.equal(loopback.ret, false);

  const crossSiteYt = await dispatch(
    onMessage,
    { type: "direct-video-download", url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ" },
    { id: EXT_ID, tab: { id: 22, url: "https://evil.example/" }, url: "https://evil.example/", frameId: 0 },
  );
  assert.equal(crossSiteYt.ret, false);
});

test("sniff-page-assets collects images with dimensions, classifies docs/archives, and rejects subframes", () => {
  const subframe = loadInterceptorHarness({ isTopFrame: false });
  let subReply = null;
  const subRet = subframe.onAssetsMessage(
    { type: "sniff-page-assets" },
    { id: EXT_ID },
    (r) => {
      subReply = r;
    },
  );
  assert.equal(subRet, false);
  assert.equal(subReply, null);

  const top = loadInterceptorHarness({
    isTopFrame: true,
    imgElements: [
      { currentSrc: "https://cdn.example.org/hero.png", naturalWidth: 1200, naturalHeight: 800 },
      { currentSrc: "https://cdn.example.org/hero.png", naturalWidth: 1200, naturalHeight: 800 },
      { currentSrc: "data:image/png;base64,AAA", naturalWidth: 16, naturalHeight: 16 },
    ],
    anchorElements: [
      { href: "https://example.org/manual.pdf" },
      { href: "https://example.org/release.zip" },
      { href: "javascript:alert(1)" },
    ],
  });
  let reply = null;
  top.onAssetsMessage({ type: "sniff-page-assets" }, { id: EXT_ID }, (r) => {
    reply = r;
  });
  assert.equal(reply?.items?.length, 3);
  assert.deepEqual(
    JSON.parse(JSON.stringify(reply.items.map((i) => [i.kind, i.width]))),
    [
      ["image", 1200],
      ["doc", 0],
      ["archive", 0],
    ],
  );
});
