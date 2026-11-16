const DEFAULTS = {
  port: 53841,
  token: "",
  autoCapture: true,
  excludedDomains: "",
  fileTypes: "",
};
const HEALTH_TTL_MS = 5000;
const RULES_TTL_MS = 5000;
const BYPASS_TTL_MS = 10000;
const handledIds = new Set();
const bypassUrls = new Map();
let healthCache = { port: null, ok: false, at: 0 };
let rulesCache = { port: null, token: "", excludedDomains: "", at: 0 };

function markBypass(url) {
  if (!url) return;
  const now = Date.now();
  bypassUrls.set(String(url), now + BYPASS_TTL_MS);
  for (const [key, exp] of bypassUrls.entries()) {
    if (exp < now) bypassUrls.delete(key);
  }
}

function isBypassed(url) {
  const exp = bypassUrls.get(String(url || ""));
  if (!exp) return false;
  if (Date.now() > exp) {
    bypassUrls.delete(String(url));
    return false;
  }
  return true;
}

async function getConfig() {
  const stored = await chrome.storage.local.get(DEFAULTS);
  return {
    port: Number(stored.port) || DEFAULTS.port,
    token: String(stored.token || ""),
    autoCapture: stored.autoCapture !== false,
    excludedDomains: String(stored.excludedDomains || ""),
    fileTypes: String(stored.fileTypes || ""),
  };
}

async function health(port) {
  const now = Date.now();
  if (healthCache.port === port && now - healthCache.at < HEALTH_TTL_MS) {
    return healthCache.ok;
  }
  let ok = false;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 2000);
  try {
    const response = await fetch(`http://127.0.0.1:${port}/health`, {
      signal: controller.signal,
      cache: "no-store",
    });
    ok = response.ok;
  } catch {
    ok = false;
  } finally {
    clearTimeout(timer);
  }
  healthCache = { port, ok, at: Date.now() };
  return ok;
}

async function postDownload(payload) {
  return post("downloads", payload);
}

async function post(path, payload) {
  const config = await getConfig();
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 70000);
  try {
    const response = await fetch(`http://127.0.0.1:${config.port}/${path}`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-lajufile-token": config.token,
      },
      body: JSON.stringify(payload),
      signal: controller.signal,
    });
    let body = {};
    try {
      body = await response.json();
    } catch {
      body = {};
    }
    return { status: response.status, body };
  } finally {
    clearTimeout(timer);
  }
}

async function cookieHeader(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((cookie) => `${cookie.name}=${cookie.value}`).join("; ");
  } catch {
    return "";
  }
}

function sanitizeCookieField(value) {
  return String(value ?? "").replace(/[\t\r\n]/g, " ");
}

async function netscapeCookies(rawUrl) {
  try {
    const parsed = new URL(rawUrl);
    const queries = [chrome.cookies.getAll({ url: rawUrl })];
    const host = parsed.hostname.toLowerCase();
    if (host === "youtu.be" || host === "youtube.com" || host.endsWith(".youtube.com")) {
      queries.push(chrome.cookies.getAll({ domain: "youtube.com" }));
    }
    const lists = await Promise.all(queries);
    const seen = new Set();
    let totalBytes = 0;
    const lines = ["# Netscape HTTP Cookie File"];
    for (const cookies of lists) {
      for (const c of cookies) {
        if (!c || !c.name) continue;
        const domain = sanitizeCookieField(c.domain || host);
        const path = sanitizeCookieField(c.path || "/");
        const name = sanitizeCookieField(c.name);
        const key = `${domain}\t${path}\t${name}`;
        if (seen.has(key)) continue;
        seen.add(key);
        const subdomains = domain.startsWith(".") ? "TRUE" : "FALSE";
        const secure = c.secure ? "TRUE" : "FALSE";
        const expiry = c.expirationDate ? Math.floor(c.expirationDate) : 0;
        const value = sanitizeCookieField(c.value);
        const line = `${domain}\t${subdomains}\t${path}\t${secure}\t${expiry}\t${name}\t${value}`;
        if (totalBytes + line.length + 1 > 48 * 1024) break;
        totalBytes += line.length + 1;
        lines.push(line);
      }
    }
    return lines.length > 1 ? `${lines.join("\n")}\n` : "";
  } catch {
    return "";
  }
}

function notify(title, message) {
  chrome.notifications.create(`lajufile-${Date.now()}`, {
    type: "basic",
    iconUrl: "icons/icon-128.png",
    title,
    message,
  });
}

function basename(value) {
  if (!value) return undefined;
  const parts = String(value).split(/[\\/]/);
  const name = parts[parts.length - 1];
  return name || undefined;
}

function parseRuleTokens(raw) {
  return String(raw || "")
    .split(/[\s,;]+/)
    .map((token) => token.trim().toLowerCase())
    .filter(Boolean);
}

function hostMatchesExcluded(host, rulesRaw) {
  if (!host) return false;
  const normalizedHost = host.toLowerCase();
  return parseRuleTokens(rulesRaw).some((token) => {
    const domain = token.replace(/^\*\.|^\./, "");
    return domain && (normalizedHost === domain || normalizedHost.endsWith(`.${domain}`));
  });
}

function urlHost(rawUrl) {
  try {
    return rawUrl ? new URL(rawUrl).hostname.toLowerCase() : "";
  } catch {
    return "";
  }
}

function detectExtension(item, url) {
  const candidates = [];
  const fileBase = basename(item && item.filename);
  if (fileBase) candidates.push(fileBase);
  try {
    const pathname = new URL(url).pathname;
    const seg = basename(pathname);
    if (seg) candidates.push(seg);
  } catch { }
  for (const name of candidates) {
    const idx = name.lastIndexOf(".");
    if (idx > 0 && idx < name.length - 1) {
      return name.slice(idx + 1).toLowerCase();
    }
  }
  return "";
}

async function fetchBridgeExcludedDomains(config) {
  if (!config.token) return "";
  const now = Date.now();
  if (
    rulesCache.port === config.port &&
    rulesCache.token === config.token &&
    now - rulesCache.at < RULES_TTL_MS
  ) {
    return rulesCache.excludedDomains;
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 1200);
  try {
    const response = await fetch(`http://127.0.0.1:${config.port}/rules`, {
      headers: { "x-lajufile-token": config.token },
      signal: controller.signal,
      cache: "no-store",
    });
    if (!response.ok) return "";
    const body = await response.json();
    const excludedDomains = String((body && body.excluded_domains) || "");
    const capturedExtensions = String((body && body.captured_extensions) || "");
    rulesCache = {
      port: config.port,
      token: config.token,
      excludedDomains,
      at: Date.now(),
    };
    chrome.storage.local
      .set({
        remoteExcludedDomains: excludedDomains,
        remoteCapturedExtensions: capturedExtensions,
      })
      .catch(() => { });
    return excludedDomains;
  } catch {
    return "";
  } finally {
    clearTimeout(timer);
  }
}

function autoCaptureSkipReason(item, url, config, remoteExcluded) {
  const combinedExcluded = `${config.excludedDomains || ""}\n${remoteExcluded || ""}`;
  const downloadHost = urlHost(url);
  const referrerHost = urlHost(item && item.referrer);
  if (
    hostMatchesExcluded(downloadHost, combinedExcluded) ||
    hostMatchesExcluded(referrerHost, combinedExcluded)
  ) {
    return "excluded-domain";
  }
  const extRules = parseRuleTokens(config.fileTypes).map((t) => t.replace(/^\./, "")).filter(Boolean);
  if (extRules.length > 0 && !extRules.includes("*")) {
    const ext = detectExtension(item, url);
    if (!ext || !extRules.includes(ext)) {
      return "excluded-extension";
    }
  }
  return null;
}

async function capture(item, allowBrowserFallback) {
  const config = await getConfig();
  if (allowBrowserFallback && !config.autoCapture) {
    return { status: 0, skipped: true, reason: "disabled" };
  }
  if (!(await health(config.port))) {
    return { status: 0, skipped: true, reason: "unreachable" };
  }

  const url = item.url || item.finalUrl || item.linkUrl;
  if (!url || !/^https?:/i.test(url)) {
    return { status: 0, skipped: true, reason: "unsupported-url" };
  }
  if (allowBrowserFallback) {
    if (isBypassed(url)) {
      return { status: 0, skipped: true, reason: "alt-bypass" };
    }
    const remoteExcluded = await fetchBridgeExcludedDomains(config);
    const reason = autoCaptureSkipReason(item, url, config, remoteExcluded);
    if (reason) {
      return { status: 0, skipped: true, reason };
    }
  }

  const cookie = await cookieHeader(url);
  const payload = {
    url,
    referrer: item.referrer || undefined,
    user_agent: navigator.userAgent,
  };
  if (item.filename) payload.filename = basename(item.filename);
  if (cookie.trim()) payload.cookie = cookie;

  try {
    const result = await postDownload(payload);
    if (result.status === 401) {
      notify("LajuFile", "Pair the LajuFile extension in its options.");
    }
    return result;
  } catch (error) {
    return { status: 0, error: String(error && error.message ? error.message : error) };
  }
}

async function cancelAndErase(id) {
  await new Promise((resolve) => chrome.downloads.cancel(id, () => resolve()));
  // erase only clears history; removeFile deletes the file Edge wrote.
  await new Promise((resolve) => {
    chrome.downloads.removeFile(id, () => {
      void chrome.runtime.lastError;
      resolve();
    });
  });
  await new Promise((resolve) => {
    chrome.downloads.erase({ id }, () => {
      void chrome.runtime.lastError;
      resolve();
    });
  });
}

// The bridge may wait up to a minute for the user to pick a save folder, so
// the browser copy is held while LajuFile asks instead of racing it.
async function pauseItem(id) {
  try {
    await chrome.downloads.pause(id);
  } catch { }
}

async function resumeItem(id) {
  try {
    await chrome.downloads.resume(id);
  } catch { }
}

function isLiveDownload(item) {
  if (!item || item.state !== "in_progress") return false;
  if (item.startTime) {
    const startedAt = Date.parse(item.startTime);
    if (Number.isFinite(startedAt) && Date.now() - startedAt > 10000) {
      return false;
    }
  }
  return true;
}

function markHandled(id) {
  handledIds.add(id);
  setTimeout(() => handledIds.delete(id), 75000);
}

if (chrome.downloads.onDeterminingFilename) {
  chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
    let finished = false;
    const finish = () => {
      if (!finished) {
        finished = true;
        suggest();
      }
    };

    if (
      handledIds.has(item.id) ||
      !isLiveDownload(item) ||
      !item.url ||
      !/^https?:/i.test(item.url)
    ) {
      finish();
      return false;
    }

    markHandled(item.id);
    (async () => {
      await pauseItem(item.id);
      const result = await capture(item, true);
      if (result.status === 201 || result.status === 409) {
        await cancelAndErase(item.id);
      } else {
        await resumeItem(item.id);
      }
      finish();
    })().catch(finish);

    return true;
  });
}

chrome.downloads.onCreated.addListener((item) => {
  if (handledIds.has(item.id) || !isLiveDownload(item)) return;
  const id = item.id;
  setTimeout(async () => {
    if (handledIds.has(id)) return;
    const [current] = await chrome.downloads.search({ id }).catch(() => []);
    if (!current || current.state !== "in_progress") return;
    const url = current.finalUrl || current.url || item.finalUrl || item.url;
    if (!url || !/^https?:/i.test(url)) return;
    markHandled(id);
    await pauseItem(id);
    const result = await capture({ ...current, url }, true);
    if (result.status === 201 || result.status === 409) {
      await cancelAndErase(id);
    } else {
      await resumeItem(id);
    }
  }, 1500);
});

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "lajufile-download",
      title: "Download with LajuFile",
      contexts: ["link"],
    });
  });
});

function isYouTubeVideoUrl(rawUrl) {
  try {
    const parsed = new URL(rawUrl);
    const host = parsed.hostname.toLowerCase();
    if (host === "youtu.be") {
      return parsed.pathname.length > 1;
    }
    if (host === "youtube.com" || host === "www.youtube.com" || host === "m.youtube.com") {
      return parsed.pathname === "/watch" && parsed.searchParams.has("v");
    }
    return false;
  } catch {
    return false;
  }
}

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== "lajufile-download" || !info.linkUrl) return;
  if (isYouTubeVideoUrl(info.linkUrl)) {
    try {
      const cookie = await netscapeCookies(info.linkUrl);
      const result = await post("video", {
        url: info.linkUrl,
        video_format: "bv*+ba/b",
        cookie: cookie || undefined,
      });
      if (result.status === 201) {
        notify("LajuFile", "Video added to LajuFile.");
      } else if (result.status === 409) {
        notify("LajuFile", "Already in LajuFile.");
      } else if (result.status === 401) {
        notify("LajuFile", "Pair the LajuFile extension in its options.");
      } else {
        notify(
          "LajuFile",
          result.body && result.body.error
            ? result.body.error
            : "Could not queue video download.",
        );
      }
    } catch {
      notify("LajuFile", "Could not reach LajuFile. Is it running?");
    }
    return;
  }
  const result = await capture(
    { url: info.linkUrl, referrer: tab && tab.url ? tab.url : undefined },
    false,
  );
  if (result.status === 201) {
    notify("LajuFile", "Download added to LajuFile.");
  } else if (result.status === 409) {
    notify("LajuFile", "Already in LajuFile.");
  } else if (result.reason === "unreachable") {
    notify("LajuFile", "Could not reach LajuFile. Is it running?");
  }
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message && message.type === "config-updated") {
    healthCache = { port: null, ok: false, at: 0 };
    rulesCache = { port: null, token: "", excludedDomains: "", at: 0 };
    getConfig()
      .then((config) => fetchBridgeExcludedDomains(config))
      .catch(() => { });
    return;
  }
  if (message && message.type === "bypass-url") {
    markBypass(message.url);
    sendResponse({ ok: true });
    return;
  }
  if (message && message.type === "intercept-link") {
    capture(
      {
        url: message.url,
        referrer: message.referrer,
        filename: message.filename,
      },
      true,
    )
      .then((result) => {
        if (result.status === 201 || result.status === 409) {
          sendResponse({ captured: true, status: result.status });
        } else {
          markBypass(message.url);
          sendResponse({ fallback: true, reason: result.reason || "error" });
        }
      })
      .catch(() => {
        markBypass(message.url);
        sendResponse({ fallback: true, reason: "error" });
      });
    return true;
  }
  if (message && message.type === "video-formats") {
    (async () => {
      const cookie = await netscapeCookies(message.url);
      return post("video/formats", {
        url: message.url,
        cookie: cookie || undefined,
      });
    })()
      .then(sendResponse)
      .catch((error) => sendResponse({ status: 0, error: String(error) }));
    return true;
  }
  if (message && message.type === "video-download") {
    (async () => {
      const cookie = await netscapeCookies(message.url);
      return post("video", {
        url: message.url,
        video_format: message.selector,
        filename: message.filename,
        cookie: cookie || undefined,
      });
    })()
      .then(sendResponse)
      .catch((error) => sendResponse({ status: 0, error: String(error) }));
    return true;
  }
  return undefined;
});
