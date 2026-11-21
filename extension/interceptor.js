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

function parseTokens(raw) {
  return String(raw || "")
    .split(/[\s,;]+/)
    .map((t) => t.trim().toLowerCase())
    .filter(Boolean);
}

function domainExcluded(host) {
  if (!host) return false;
  const h = host.toLowerCase();
  const merged = `${rulesState.excludedDomains || ""}\n${rulesState.remoteExcludedDomains || ""}`;
  return parseTokens(merged).some((token) => {
    const clean = token.replace(/^\*\.|^\./, "");
    return clean && (h === clean || h.endsWith(`.${clean}`));
  });
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
    if (event.button !== 0) return;
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
    const fileBase = parsed.pathname.split("/").pop() || undefined;
    chrome.runtime
      .sendMessage({
        type: "intercept-link",
        url: parsed.href,
        referrer: location.href,
        filename: fileBase,
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
    const name = decodeURIComponent(parsed.pathname.split("/").pop() || "") || kind;
    items.push({ url: parsed.href, kind, label: name.slice(0, 42) });
  };
  document.querySelectorAll("video, audio").forEach((el) => {
    const kind = el.tagName.toLowerCase();
    pushUrl(el.currentSrc || el.src, kind);
    el.querySelectorAll("source[src]").forEach((src) => pushUrl(src.src, kind));
  });
  sendResponse({ items: items.slice(0, 6) });
  return false;
});
