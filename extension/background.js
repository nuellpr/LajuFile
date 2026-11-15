const DEFAULTS = { port: 53841, token: "", autoCapture: true };
const HEALTH_TTL_MS = 5000;
const handledIds = new Set();
let healthCache = { port: null, ok: false, at: 0 };

async function getConfig() {
  const stored = await chrome.storage.local.get(DEFAULTS);
  return {
    port: Number(stored.port) || DEFAULTS.port,
    token: String(stored.token || ""),
    autoCapture: stored.autoCapture !== false,
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
  } catch {}
}

async function resumeItem(id) {
  try {
    await chrome.downloads.resume(id);
  } catch {}
}

function markHandled(id) {
  handledIds.add(id);
  setTimeout(() => handledIds.delete(id), 60000);
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

    if (!item.url || !/^https?:/i.test(item.url)) {
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
  const id = item.id;
  setTimeout(async () => {
    if (handledIds.has(id)) return;
    const url = item.finalUrl || item.url;
    if (!url || !/^https?:/i.test(url)) return;
    markHandled(id);
    await pauseItem(id);
    const result = await capture({ ...item, url }, true);
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

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== "lajufile-download" || !info.linkUrl) return;
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
    return;
  }
  if (message && message.type === "video-formats") {
    post("video/formats", { url: message.url })
      .then(sendResponse)
      .catch((error) => sendResponse({ status: 0, error: String(error) }));
    return true;
  }
  if (message && message.type === "video-download") {
    post("video", {
      url: message.url,
      video_format: message.selector,
      filename: message.filename,
    })
      .then(sendResponse)
      .catch((error) => sendResponse({ status: 0, error: String(error) }));
    return true;
  }
  return undefined;
});
