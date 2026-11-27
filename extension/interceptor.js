// Handles Alt+Click bypass (leave download to browser) and catches browser-viewable
// PDF/media links so they open in LajuFile instead of an inline browser viewer.
const INLINE_CATCH_EXTS = new Set([
  "pdf",
  "mp4",
  "mkv",
  "webm",
  "avi",
  "mov",
  "m4v",
  "mp3",
  "m4a",
  "wav",
  "flac",
  "ogg",
  "aac",
  "opus",
]);

let rulesState = {
  autoCapture: true,
  excludedDomains: "",
  remoteExcludedDomains: "",
  fileTypes: "",
  remoteCapturedExtensions: "",
};

function syncRulesFromStorage() {
  chrome.storage.local
    .get({
      autoCapture: true,
      excludedDomains: "",
      remoteExcludedDomains: "",
      fileTypes: "",
      remoteCapturedExtensions: "",
    })
    .then((stored) => {
      rulesState = stored;
    })
    .catch(() => {});
}

syncRulesFromStorage();
chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== "local") return;
  for (const key of Object.keys(rulesState)) {
    if (changes[key]) {
      rulesState[key] = changes[key].newValue;
    }
  }
});

const { parseTokens, isHostExcluded } = globalThis.LajuFileCommon;

function domainExcluded(host) {
  const merged = `${rulesState.excludedDomains || ""}\n${rulesState.remoteExcludedDomains || ""}`;
  return isHostExcluded(host, merged);
}

function isInlineCatchAllowed(urlObj) {
  const host = urlObj.hostname.toLowerCase();
  if (host === "localhost" || host === "127.0.0.1") return false;
  if (domainExcluded(location.hostname) || domainExcluded(host)) return false;
  if (
    (host === "youtube.com" || host === "www.youtube.com" || host === "m.youtube.com") &&
    urlObj.pathname === "/watch"
  ) {
    return false;
  }
  const seg = urlObj.pathname.split("/").pop() || "";
  const dot = seg.lastIndexOf(".");
  if (dot <= 0 || dot >= seg.length - 1) return false;
  const ext = seg.slice(dot + 1).toLowerCase();
  if (!INLINE_CATCH_EXTS.has(ext)) return false;

  const customSource = String(rulesState.fileTypes || "").trim()
    ? rulesState.fileTypes
    : rulesState.remoteCapturedExtensions;
  const list = parseTokens(customSource).map((t) => t.replace(/^\./, "")).filter(Boolean);
  if (list.length === 0 || list.includes("*")) return true;
  return list.includes(ext);
}

function openBypassed(anchor, url) {
  chrome.runtime.sendMessage({ type: "bypass-url", url }).catch(() => {}).finally(() => {
    if (anchor && anchor.target === "_blank") {
      const opened = window.open(url, "_blank", "noopener");
      if (!opened) {
        window.location.assign(url);
      }
    } else {
      window.location.assign(url);
    }
  });
}

document.addEventListener(
  "click",
  (event) => {
    if (!event.isTrusted || event.button !== 0) return;
    const target = event.target;
    if (!target || typeof target.closest !== "function") return;
    const anchor = target.closest("a[href]");
    if (!anchor || !anchor.href) return;

    let parsed;
    try {
      parsed = new URL(anchor.href);
    } catch {
      return;
    }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return;

    // Hold Alt + Click leaves any download or link directly to the browser.
    if (event.altKey) {
      chrome.runtime.sendMessage({ type: "bypass-url", url: parsed.href }).catch(() => {});
      return;
    }

    if (
      event.ctrlKey ||
      event.metaKey ||
      event.shiftKey ||
      anchor.hasAttribute("download") ||
      rulesState.autoCapture === false ||
      !isInlineCatchAllowed(parsed)
    ) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    chrome.runtime
      .sendMessage({
        type: "intercept-link",
        url: parsed.href,
        referrer: location.href,
      })
      .then((res) => {
        if (res && res.fallback) {
          openBypassed(anchor, parsed.href);
        }
      })
      .catch(() => {
        openBypassed(anchor, parsed.href);
      });
  },
  true,
);

function safeDecodeSegment(raw) {
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

// On-demand media sniffer queried by the extension popup for <video>/<audio> sources.
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || message.type !== "sniff-page-media" || !sender || sender.id !== chrome.runtime.id) {
    return false;
  }
  const seen = new Set();
  const items = [];
  const pushUrl = (raw, kind) => {
    if (!raw || typeof raw !== "string") return;
    let parsed;
    try {
      parsed = new URL(raw, location.href);
    } catch {
      return;
    }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return;
    if (seen.has(parsed.href)) return;
    seen.add(parsed.href);
    const name = safeDecodeSegment(parsed.pathname.split("/").pop() || "") || kind;
    items.push({ url: parsed.href, kind, label: name.slice(0, 42) });
  };
  document.querySelectorAll("video, audio").forEach((el) => {
    const kind = el.tagName.toLowerCase();
    pushUrl(el.currentSrc || el.src, kind);
    el.querySelectorAll("source[src]").forEach((src) => pushUrl(src.src, kind));
  });
  if (items.length > 0) {
    sendResponse({ items: items.slice(0, 6) });
    return false;
  }
  let isTopFrame = true;
  try {
    isTopFrame = window === window.top;
  } catch {
    isTopFrame = false;
  }
  if (!isTopFrame) {
    return false;
  }
  setTimeout(() => sendResponse({ items: [] }), 40);
  return true;
});

function classifyAssetExtension(urlObj) {
  const seg = urlObj.pathname.split("/").pop() || "";
  const dot = seg.lastIndexOf(".");
  if (dot <= 0 || dot >= seg.length - 1) return "";
  const ext = seg.slice(dot + 1).toLowerCase();
  if (/^(png|jpe?g|webp|gif|svg|avif|bmp|ico)$/.test(ext)) return "image";
  if (/^(pdf|docx?|xlsx?|pptx?|epub|csv|md|txt)$/.test(ext)) return "doc";
  if (/^(zip|rar|7z|tar|gz|tgz|bz2|xz|iso|dmg|exe|msi)$/.test(ext)) return "archive";
  if (/^(mp4|mkv|webm|mov|m4v|mp3|m4a|flac|wav|ogg|aac)$/.test(ext)) return "media";
  return "";
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || message.type !== "sniff-page-assets" || !sender || sender.id !== chrome.runtime.id) {
    return false;
  }
  let isTopFrame = true;
  try {
    isTopFrame = window === window.top;
  } catch {
    isTopFrame = false;
  }
  if (!isTopFrame) {
    return false;
  }
  const seen = new Set();
  const items = [];
  const pushAsset = (raw, kind, width = 0, height = 0) => {
    if (!raw || typeof raw !== "string" || items.length >= 120) return;
    let parsed;
    try {
      parsed = new URL(raw, location.href);
    } catch {
      return;
    }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return;
    if (seen.has(parsed.href)) return;
    seen.add(parsed.href);
    const name = safeDecodeSegment(parsed.pathname.split("/").pop() || "") || kind;
    items.push({
      url: parsed.href,
      kind,
      label: name.slice(0, 44),
      width: Number(width) || 0,
      height: Number(height) || 0,
    });
  };

  document.querySelectorAll("img").forEach((img) => {
    pushAsset(img.currentSrc || img.src, "image", img.naturalWidth, img.naturalHeight);
  });
  document.querySelectorAll("a[href]").forEach((anchor) => {
    try {
      const parsed = new URL(anchor.href, location.href);
      const kind = classifyAssetExtension(parsed);
      if (kind) pushAsset(parsed.href, kind, 0, 0);
    } catch {}
  });
  document.querySelectorAll("video, audio").forEach((el) => {
    pushAsset(el.currentSrc || el.src, "media", 0, 0);
    el.querySelectorAll("source[src]").forEach((src) => pushAsset(src.src, "media", 0, 0));
  });

  sendResponse({ items });
  return false;
});
