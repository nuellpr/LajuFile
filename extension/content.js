// Adds an IDM-style "Download with LajuFile" floating panel to videos on YouTube
// and any other website (TikTok, Instagram, X, Facebook, embedded players, HTML5 <video>,
// and HLS/DASH streams) and routes downloads through the LajuFile background worker.
(() => {
  if (globalThis.__lajufileVideoPanelLoaded) return;
  globalThis.__lajufileVideoPanelLoaded = true;

  const PANEL_ID = "lajufile-video-button";
  const STREAM_RE = /\.(m3u8|mpd)(\?|#|$)/i;
  const MEDIA_RESOURCE_RE = /\.(m3u8|mpd|mp4|webm|m4v|mov)(\?|#|$)/i;
  const DIRECT_VIDEO_EXT_RE = /\.(mp4|webm|m4v|mov)(\?|#|$)/i;
  const MAX_RESOURCE_URLS = 24;

  const KNOWN_VIDEO_DOMAINS = [
    "youtube.com",
    "youtu.be",
    "youtube-nocookie.com",
    "vimeo.com",
    "twitch.tv",
    "tiktok.com",
    "x.com",
    "twitter.com",
    "dailymotion.com",
    "soundcloud.com",
    "bilibili.com",
    "reddit.com",
    "instagram.com",
    "facebook.com",
    "fb.watch",
  ];

  const dismissedKeys = new Set();
  const dismissedVideoState = new WeakMap();
  const resourceUrls = [];
  let hoveredVideo = null;
  let activeMountState = null; // { mode, video, targetKey, wrap, bar, button, labelEl, menu, menuOpen }
  let exclusionState = {
    excludedDomains: "",
    remoteExcludedDomains: "",
  };

  function hostMatchesRule(host, rawList) {
    if (!host) return false;
    if (
      globalThis.LajuFileCommon &&
      typeof globalThis.LajuFileCommon.isHostExcluded === "function"
    ) {
      return globalThis.LajuFileCommon.isHostExcluded(host, rawList);
    }
    const normalized = host.toLowerCase();
    return String(rawList || "")
      .split(/[\s,;]+/)
      .map((t) => t.trim().toLowerCase().replace(/^\*\.|^\./, ""))
      .filter(Boolean)
      .some((domain) => normalized === domain || normalized.endsWith(`.${domain}`));
  }

  function isCurrentSiteExcluded() {
    const merged = `${exclusionState.excludedDomains || ""}\n${exclusionState.remoteExcludedDomains || ""}`;
    return hostMatchesRule(location.hostname, merged);
  }

  function syncExclusions() {
    if (!chrome?.storage?.local) return;
    chrome.storage.local
      .get({ excludedDomains: "", remoteExcludedDomains: "" })
      .then((stored) => {
        exclusionState = stored;
        if (isCurrentSiteExcluded()) {
          unmountPanel();
        } else {
          scheduleSync(true);
        }
      })
      .catch(() => {});
  }

  if (chrome?.storage?.onChanged) {
    chrome.storage.onChanged.addListener((changes, areaName) => {
      if (areaName !== "local") return;
      let changed = false;
      for (const key of ["excludedDomains", "remoteExcludedDomains"]) {
        if (changes[key]) {
          exclusionState[key] = changes[key].newValue;
          changed = true;
        }
      }
      if (changed) {
        if (isCurrentSiteExcluded()) {
          unmountPanel();
        } else {
          scheduleSync(true);
        }
      }
    });
  }

  function recordResourceUrl(raw) {
    if (typeof raw !== "string" || !MEDIA_RESOURCE_RE.test(raw)) return;
    let parsed;
    try {
      parsed = new URL(raw, location.href);
    } catch {
      return;
    }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return;
    const href = parsed.href;
    const existingIdx = resourceUrls.indexOf(href);
    if (existingIdx !== -1) {
      resourceUrls.splice(existingIdx, 1);
    }
    resourceUrls.push(href);
    while (resourceUrls.length > MAX_RESOURCE_URLS) {
      resourceUrls.shift();
    }
  }

  function initResourceObserver() {
    if (typeof performance !== "undefined" && typeof performance.getEntriesByType === "function") {
      try {
        for (const entry of performance.getEntriesByType("resource")) {
          if (entry && entry.name) recordResourceUrl(entry.name);
        }
      } catch {}
    }
    if (typeof PerformanceObserver === "function") {
      try {
        const observer = new PerformanceObserver((list) => {
          for (const entry of list.getEntries()) {
            if (entry && entry.name) recordResourceUrl(entry.name);
          }
        });
        observer.observe({ type: "resource", buffered: true });
      } catch {
        try {
          const observer = new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) {
              if (entry && entry.name) recordResourceUrl(entry.name);
            }
          });
          observer.observe({ entryTypes: ["resource"] });
        } catch {}
      }
    }
  }

  function hostMatchesList(host, domains) {
    const clean = String(host || "").toLowerCase();
    return domains.some((d) => clean === d || clean.endsWith(`.${d}`));
  }

  function isKnownVideoSite() {
    return hostMatchesList(location.hostname, KNOWN_VIDEO_DOMAINS);
  }

  function isYouTubeHost(host = location.hostname) {
    return hostMatchesList(host, ["youtube.com", "youtu.be", "youtube-nocookie.com"]);
  }

  function isYouTubeWatchPage() {
    return (
      isYouTubeHost() &&
      location.pathname === "/watch" &&
      Boolean(new URLSearchParams(location.search).get("v"))
    );
  }

  function pageVideoTitle() {
    if (isYouTubeHost()) {
      const heading = document.querySelector(
        "h1.ytd-watch-metadata yt-formatted-string, h1.title yt-formatted-string, ytd-reel-video-renderer[is-active] h2",
      );
      if (heading && heading.textContent && heading.textContent.trim()) {
        return heading.textContent.trim();
      }
      return document.title.replace(/\s*-\s*YouTube$/i, "").trim() || "video";
    }
    const ogTitle = document.querySelector('meta[property="og:title"], meta[name="twitter:title"]');
    if (ogTitle && ogTitle.content && ogTitle.content.trim()) {
      return ogTitle.content.trim();
    }
    return document.title.replace(/\s*[-|·]\s*(TikTok|Instagram|X|Twitter|Vimeo).*$/i, "").trim() || "video";
  }

  function truncateTitle(title, maxLen = 36) {
    const clean = String(title || "video").replace(/\s+/g, " ").trim();
    if (clean.length <= maxLen) return clean;
    return `${clean.slice(0, maxLen).trimEnd()} ...`;
  }

  function formatBytes(bytes) {
    const value = Number(bytes);
    if (!Number.isFinite(value) || value <= 0) return "";
    if (value < 1024) return `${Math.round(value)} B`;
    const units = ["KB", "MB", "GB", "TB"];
    let num = value / 1024;
    let unit = 0;
    while (num >= 1024 && unit + 1 < units.length) {
      num /= 1024;
      unit += 1;
    }
    return `${num.toFixed(1)} ${units[unit]}`;
  }

  function createSizeBadge(sizeBytes) {
    const text = formatBytes(sizeBytes);
    if (!text) return null;
    const badge = document.createElement("span");
    badge.textContent = `~${text}`;
    badge.style.cssText =
      'padding:2px 7px;border-radius:999px;background:rgba(76,154,255,0.16);border:1px solid rgba(76,154,255,0.32);color:#93c5fd;font:600 10.5px "Segoe UI",system-ui,sans-serif;font-variant-numeric:tabular-nums;flex-shrink:0;';
    return badge;
  }

  function formatQualityDescription(format) {
    const label = String(format.label || "");
    if (/audio/i.test(label)) {
      return "M4A · Audio only";
    }
    const match = label.match(/^(\d+)p$/i);
    if (!match) {
      return `MP4 · ${label}`;
    }
    const height = Number(match[1]);
    const hd = height >= 720 ? " HD" : "";
    const fps = format.fps && Number(format.fps) > 30 ? ` ${Number(format.fps)} fps` : "";
    return `MP4 · ${label}${hd}${fps}`;
  }

  function errorText(result) {
    if (result && result.body && result.body.error) return result.body.error;
    if (result && result.error) return result.error;
    return "Could not connect to LajuFile. Make sure LajuFile Desktop is running.";
  }

  function showStatusRow(menu, message, isError = false) {
    menu.textContent = "";
    const row = document.createElement("div");
    row.textContent = message;
    row.style.cssText = `padding:10px 14px;color:${isError ? "#ff6b6b" : "#cbd5e1"};font:12px/1.4 "Segoe UI",system-ui,sans-serif;white-space:normal;`;
    menu.append(row);
    menu.style.display = "block";
  }

  function styleMenuRow(row) {
    row.style.cssText =
      'display:flex;align-items:center;justify-content:space-between;gap:14px;width:100%;box-sizing:border-box;background:transparent;color:#e7edf5;border:0;border-radius:7px;padding:7px 11px;cursor:pointer;font:12px "Segoe UI",system-ui,sans-serif;text-align:left;white-space:nowrap;transition:background 0.14s ease;';
    row.addEventListener("mouseenter", () => {
      row.style.background = "rgba(76,154,255,0.16)";
    });
    row.addEventListener("mouseleave", () => {
      row.style.background = "transparent";
    });
  }

  function handleSendResult(menu, result, successMessage) {
    const status = result ? result.status : 0;
    if (status === 401) {
      showStatusRow(menu, "Pair the LajuFile extension token in Options first.", true);
    } else if (status === 409) {
      showStatusRow(menu, "This video is already in LajuFile.");
    } else if (status !== 201) {
      showStatusRow(menu, errorText(result), true);
    } else {
      showStatusRow(menu, successMessage);
      setTimeout(() => {
        menu.style.display = "none";
      }, 1500);
    }
  }

  async function downloadVideoSelector(url, selector, label, title, menu, sizeBytes) {
    showStatusRow(menu, `Sending "${truncateTitle(title, 28)}" (${label}) to LajuFile...`);
    let ext = "mp4";
    if (selector === "audio:mp3") {
      ext = "mp3";
    } else if (selector === "subs:srt") {
      ext = "srt";
    } else if (/audio/i.test(label)) {
      ext = "m4a";
    }
    const baseTitle = title || pageVideoTitle();
    const result = await chrome.runtime.sendMessage({
      type: "video-download",
      url,
      selector,
      filename: baseTitle ? `${baseTitle}.${ext}` : undefined,
      referrer: location.href,
      size: typeof sizeBytes === "number" && sizeBytes > 0 ? sizeBytes : undefined,
    });
    handleSendResult(menu, result, `Added to LajuFile (${label}).`);
  }

  async function downloadDirectVideo(url, ext, title, menu) {
    showStatusRow(menu, `Sending "${truncateTitle(title, 28)}" to LajuFile...`);
    const baseTitle = title || pageVideoTitle();
    const safeExt = ext || "mp4";
    const result = await chrome.runtime.sendMessage({
      type: "direct-video-download",
      url,
      filename: baseTitle ? `${baseTitle}.${safeExt}` : undefined,
      referrer: location.href,
    });
    handleSendResult(menu, result, "Added video to LajuFile.");
  }

  function guessVideoExt(rawUrl) {
    try {
      const pathname = new URL(rawUrl).pathname;
      const m = pathname.match(/\.([a-z0-9]{2,5})$/i);
      if (m && /^(mp4|webm|m4v|mov|mkv|avi)$/i.test(m[1])) {
        return m[1].toLowerCase();
      }
    } catch {}
    return "mp4";
  }

  function renderDirectAndStreamFallbacks(menu, target) {
    menu.textContent = "";
    const shortTitle = truncateTitle(target.title, 32);
    const heightLabel =
      target.video && target.video.videoHeight > 0 ? `${target.video.videoHeight}p` : "";

    let added = 0;
    for (const directUrl of target.directUrls.slice(0, 3)) {
      const ext = guessVideoExt(directUrl);
      const row = document.createElement("button");
      row.type = "button";
      styleMenuRow(row);
      const left = document.createElement("span");
      left.textContent = added === 0 ? `Download video (${shortTitle})` : `Source ${added + 1}`;
      left.style.cssText = "overflow:hidden;text-overflow:ellipsis;color:#e7edf5;";
      const right = document.createElement("span");
      right.textContent = [ext.toUpperCase(), heightLabel].filter(Boolean).join(" · ");
      right.style.cssText = "color:#94a3b8;font-size:11px;flex-shrink:0;";
      row.append(left, right);
      row.addEventListener("click", (event) => {
        if (!event.isTrusted) return;
        event.stopPropagation();
        downloadDirectVideo(directUrl, ext, target.title, menu);
      });
      menu.append(row);
      added += 1;
    }

    for (const streamUrl of target.streamUrls.slice(0, 2)) {
      const kind = /\.mpd(\?|#|$)/i.test(streamUrl) ? "DASH" : "HLS";
      const row = document.createElement("button");
      row.type = "button";
      styleMenuRow(row);
      const left = document.createElement("span");
      left.textContent = `Download ${kind} stream (Best)`;
      left.style.cssText = "overflow:hidden;text-overflow:ellipsis;color:#4c9aff;font-weight:600;";
      const right = document.createElement("span");
      right.textContent = "MP4";
      right.style.cssText = "color:#94a3b8;font-size:11px;flex-shrink:0;";
      row.append(left, right);
      row.addEventListener("click", (event) => {
        if (!event.isTrusted) return;
        event.stopPropagation();
        downloadVideoSelector(streamUrl, "bv*+ba/b", "Best", target.title, menu);
      });
      menu.append(row);
      added += 1;
    }

    menu.style.display = added > 0 ? "block" : "none";
    return added > 0;
  }

  function renderFormatsMenu(menu, url, formats, rawTitle) {
    menu.textContent = "";
    const shortTitle = truncateTitle(rawTitle, 36);

    const allRow = document.createElement("button");
    allRow.type = "button";
    styleMenuRow(allRow);
    allRow.style.marginBottom = "4px";
    allRow.style.borderBottom = "1px solid rgba(148,163,184,0.18)";
    allRow.style.fontWeight = "600";
    allRow.style.color = "#4c9aff";
    const allLeft = document.createElement("span");
    allLeft.textContent = `Download best (${formats[0].label})`;
    allRow.append(allLeft);
    const allSize = createSizeBadge(formats[0].size);
    if (allSize) allRow.append(allSize);
    allRow.addEventListener("click", (event) => {
      if (!event.isTrusted) return;
      event.stopPropagation();
      const best = formats[0];
      downloadVideoSelector(url, best.selector, best.label, rawTitle, menu, best.size);
    });
    menu.append(allRow);

    formats.forEach((format, index) => {
      const item = document.createElement("button");
      item.type = "button";
      styleMenuRow(item);

      const left = document.createElement("span");
      left.textContent = `${index + 1}. ${shortTitle}`;
      left.style.cssText = "overflow:hidden;text-overflow:ellipsis;color:#e7edf5;";

      const right = document.createElement("span");
      right.style.cssText = "display:inline-flex;align-items:center;gap:8px;flex-shrink:0;";
      const desc = document.createElement("span");
      desc.textContent = formatQualityDescription(format);
      desc.style.cssText = "color:#94a3b8;font-size:11px;";
      right.append(desc);
      const sizeBadge = createSizeBadge(format.size);
      if (sizeBadge) right.append(sizeBadge);

      item.append(left, right);
      item.addEventListener("click", (event) => {
        if (!event.isTrusted) return;
        event.stopPropagation();
        downloadVideoSelector(url, format.selector, format.label, rawTitle, menu, format.size);
      });
      menu.append(item);
    });

    for (const extra of [
      { label: "Audio only (MP3)", selector: "audio:mp3", desc: "MP3 · Audio only" },
      { label: "Subtitles only (.srt)", selector: "subs:srt", desc: "SRT · Subtitles" },
    ]) {
      const row = document.createElement("button");
      row.type = "button";
      styleMenuRow(row);
      const left = document.createElement("span");
      left.textContent = extra.label;
      left.style.cssText = "overflow:hidden;text-overflow:ellipsis;color:#e7edf5;";
      const right = document.createElement("span");
      right.textContent = extra.desc;
      right.style.cssText = "color:#94a3b8;font-size:11px;flex-shrink:0;";
      row.append(left, right);
      row.addEventListener("click", (event) => {
        if (!event.isTrusted) return;
        event.stopPropagation();
        downloadVideoSelector(url, extra.selector, extra.label, rawTitle, menu);
      });
      menu.append(row);
    }
    menu.style.display = "block";
  }

  async function showFormatsOrFallback(menu, target) {
    const queryUrl = target.ytdlpUrl || target.streamUrls[0] || "";
    if (!queryUrl) {
      if (!renderDirectAndStreamFallbacks(menu, target)) {
        showStatusRow(menu, "No downloadable video stream found on this player.", true);
      }
      return;
    }

    showStatusRow(menu, "Loading video qualities...");
    let result;
    try {
      result = await chrome.runtime.sendMessage({
        type: "video-formats",
        url: queryUrl,
        referrer: location.href,
      });
    } catch (error) {
      result = { status: 0, error: String(error) };
    }
    const formats = result && result.body ? result.body.formats : null;
    const rawTitle =
      result && result.body && result.body.title && result.body.title !== "video"
        ? result.body.title
        : target.title;

    if (Array.isArray(formats) && formats.length > 0) {
      renderFormatsMenu(menu, queryUrl, formats, rawTitle);
      return;
    }

    if (target.directUrls.length > 0 || (target.ytdlpUrl && target.streamUrls.length > 0)) {
      renderDirectAndStreamFallbacks(menu, target);
      return;
    }
    if (target.streamUrls.length > 0 && result && result.status !== 401 && result.status !== 0) {
      renderDirectAndStreamFallbacks(menu, target);
      return;
    }
    showStatusRow(menu, errorText(result), true);
  }

  function collectVideos(root = document, out = [], seen = new Set()) {
    if (!root || typeof root.querySelectorAll !== "function") return out;
    for (const video of root.querySelectorAll("video")) {
      if (!seen.has(video)) {
        seen.add(video);
        out.push(video);
      }
    }
    for (const el of root.querySelectorAll("*")) {
      if (el.shadowRoot) {
        collectVideos(el.shadowRoot, out, seen);
      }
    }
    return out;
  }

  function visibleArea(rect) {
    const vw = window.innerWidth || document.documentElement.clientWidth || 0;
    const vh = window.innerHeight || document.documentElement.clientHeight || 0;
    const left = Math.max(0, rect.left);
    const right = Math.min(vw, rect.right);
    const top = Math.max(0, rect.top);
    const bottom = Math.min(vh, rect.bottom);
    if (right <= left || bottom <= top) return 0;
    return (right - left) * (bottom - top);
  }

  function isElementVisible(el) {
    const style = getComputedStyle(el);
    if (
      style.display === "none" ||
      style.visibility === "hidden" ||
      parseFloat(style.opacity || "1") <= 0.05
    ) {
      return false;
    }
    return true;
  }

  function hasPlayableSource(video) {
    return Boolean(
      video.currentSrc ||
        video.src ||
        video.readyState > 0 ||
        video.querySelector("source[src]") ||
        isKnownVideoSite(),
    );
  }

  function findBestActiveVideo() {
    const candidates = [];
    for (const video of collectVideos()) {
      const rect = video.getBoundingClientRect();
      if (rect.width < 200 || rect.height < 140) continue;
      const area = visibleArea(rect);
      if (area <= 0) continue;
      if (!isElementVisible(video) || !hasPlayableSource(video)) continue;
      const playing = !video.paused && !video.ended && video.readyState >= 2;
      const hovered =
        hoveredVideo === video || (typeof video.matches === "function" && video.matches(":hover"));
      candidates.push({ video, rect, area, playing, hovered });
    }
    if (candidates.length === 0) return null;
    candidates.sort((a, b) => {
      if (a.playing !== b.playing) return a.playing ? -1 : 1;
      if (a.hovered !== b.hovered) return a.hovered ? -1 : 1;
      return b.area - a.area;
    });
    return candidates[0];
  }

  function parentAcrossShadow(el) {
    if (!el) return null;
    if (el.parentElement) return el.parentElement;
    const root = el.getRootNode && el.getRootNode();
    return root && root.host ? root.host : null;
  }

  function findNearbyLink(video, selectors) {
    let node = parentAcrossShadow(video);
    for (let depth = 0; node && depth < 10; depth += 1) {
      if (node.querySelectorAll && node.querySelectorAll("video").length > 1) {
        break;
      }
      if (node.querySelector) {
        for (const selector of selectors) {
          const anchor = node.querySelector(selector);
          if (anchor && anchor.href) {
            try {
              const parsed = new URL(anchor.href, location.href);
              if (parsed.protocol === "http:" || parsed.protocol === "https:") {
                return parsed.href;
              }
            } catch {}
          }
        }
      }
      node = parentAcrossShadow(node);
    }
    return "";
  }

  function sortStreamCandidates(urls) {
    return [...urls].sort((a, b) => {
      const aStream = STREAM_RE.test(a) ? 0 : 1;
      const bStream = STREAM_RE.test(b) ? 0 : 1;
      if (aStream !== bStream) return aStream - bStream;
      const masterRe = /(master|index|playlist|manifest)/i;
      const aMaster = masterRe.test(a) ? 0 : 1;
      const bMaster = masterRe.test(b) ? 0 : 1;
      return aMaster - bMaster;
    });
  }

  function resolveVideoTarget(video) {
    const host = location.hostname.toLowerCase();
    const directUrls = [];
    const streamUrls = [];

    const pushCandidateUrl = (raw) => {
      if (!raw || typeof raw !== "string") return;
      let parsed;
      try {
        parsed = new URL(raw, location.href);
      } catch {
        return;
      }
      if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return;
      if (STREAM_RE.test(parsed.href)) {
        if (!streamUrls.includes(parsed.href)) streamUrls.push(parsed.href);
      } else if (!directUrls.includes(parsed.href)) {
        directUrls.push(parsed.href);
      }
    };

    if (video) {
      pushCandidateUrl(video.currentSrc || video.src);
      if (video.querySelectorAll) {
        video.querySelectorAll("source[src]").forEach((s) => pushCandidateUrl(s.src));
      }
    }

    for (const resUrl of sortStreamCandidates(resourceUrls)) {
      if (STREAM_RE.test(resUrl)) {
        if (!streamUrls.includes(resUrl)) streamUrls.push(resUrl);
      } else if (DIRECT_VIDEO_EXT_RE.test(resUrl) && directUrls.length === 0) {
        directUrls.push(resUrl);
      }
    }

    let ytdlpUrl = "";
    if (isYouTubeHost(host)) {
      if (
        (location.pathname === "/watch" && new URLSearchParams(location.search).has("v")) ||
        location.pathname.startsWith("/shorts/")
      ) {
        ytdlpUrl = location.href;
      } else {
        const embedMatch = location.pathname.match(/^\/embed\/([A-Za-z0-9_-]+)/);
        if (embedMatch) {
          ytdlpUrl = `https://www.youtube.com/watch?v=${embedMatch[1]}`;
        }
      }
    } else if (host === "tiktok.com" || host.endsWith(".tiktok.com")) {
      if (/\/(video|photo)\/\d+/i.test(location.pathname)) {
        ytdlpUrl = location.href;
      } else if (video) {
        ytdlpUrl = findNearbyLink(video, ['a[href*="/video/"]', 'a[href*="/photo/"]']);
      }
    } else if (host === "instagram.com" || host.endsWith(".instagram.com")) {
      if (/^\/(?:reel|reels|p|tv)\/[^/]+/i.test(location.pathname)) {
        ytdlpUrl = location.href;
      } else if (video) {
        ytdlpUrl = findNearbyLink(video, [
          'a[href*="/reel/"]',
          'a[href*="/reels/"]',
          'a[href*="/p/"]',
        ]);
      }
    } else if (
      host === "x.com" ||
      host.endsWith(".x.com") ||
      host === "twitter.com" ||
      host.endsWith(".twitter.com")
    ) {
      if (/\/status\/\d+/i.test(location.pathname)) {
        ytdlpUrl = location.href;
      } else if (video) {
        ytdlpUrl = findNearbyLink(video, ['a[href*="/status/"]']);
      }
    } else if (
      host === "facebook.com" ||
      host.endsWith(".facebook.com") ||
      host === "fb.watch"
    ) {
      if (
        /^\/(?:reel|watch|.*\/videos\/)/i.test(location.pathname) ||
        new URLSearchParams(location.search).has("v")
      ) {
        ytdlpUrl = location.href;
      } else if (video) {
        ytdlpUrl = findNearbyLink(video, [
          'a[href*="/reel/"]',
          'a[href*="/videos/"]',
          'a[href*="/watch"]',
        ]);
      }
    } else if (
      globalThis.LajuFileCommon &&
      typeof globalThis.LajuFileCommon.isSupportedVideoPage === "function" &&
      globalThis.LajuFileCommon.isSupportedVideoPage(location.href)
    ) {
      ytdlpUrl = location.href;
    } else if (!directUrls.length && !streamUrls.length && video) {
      // Generic site with blob:/MSE player: yt-dlp supports 1,000+ video sites by page URL.
      ytdlpUrl = location.href;
    }

    const specificKey = ytdlpUrl || directUrls[0] || streamUrls[0] || "";
    return {
      video,
      title: pageVideoTitle(),
      ytdlpUrl,
      directUrls,
      streamUrls,
      specificKey,
    };
  }

  function isTargetDismissed(target) {
    if (target.specificKey && dismissedKeys.has(target.specificKey)) {
      return true;
    }
    if (!target.specificKey && target.video) {
      const state = dismissedVideoState.get(target.video);
      const currentState = target.video.currentSrc || target.video.src || location.href;
      if (state && state === currentState) return true;
    }
    return false;
  }

  function dismissTarget(target) {
    if (target.specificKey) {
      dismissedKeys.add(target.specificKey);
    } else if (target.video) {
      dismissedVideoState.set(
        target.video,
        target.video.currentSrc || target.video.src || location.href,
      );
    }
  }

  function createPlayIcon() {
    const img = document.createElement("img");
    img.src = chrome.runtime.getURL("icons/icon-32.png");
    img.alt = "";
    img.width = 16;
    img.height = 16;
    img.style.cssText =
      "display:block;width:16px;height:16px;border-radius:4px;flex-shrink:0;object-fit:contain;pointer-events:none;";
    return img;
  }

  function styleSmallControl(btn) {
    btn.type = "button";
    btn.style.cssText =
      'width:24px;height:26px;border-radius:7px;border:1px solid rgba(148,163,184,0.2);background:rgba(255,255,255,0.06);color:#cbd5e1;font:600 12px "Segoe UI",system-ui,sans-serif;cursor:pointer;display:inline-flex;align-items:center;justify-content:center;padding:0;line-height:1;';
  }

  function unmountPanel() {
    if (activeMountState && activeMountState.wrap) {
      activeMountState.wrap.remove();
    }
    const stray = document.getElementById(PANEL_ID);
    if (stray) stray.remove();
    activeMountState = null;
  }

  function createPanelElements() {
    const wrap = document.createElement("div");
    wrap.id = PANEL_ID;
    wrap.addEventListener("click", (event) => event.stopPropagation());
    wrap.addEventListener("mousedown", (event) => event.stopPropagation());
    wrap.addEventListener("pointerdown", (event) => event.stopPropagation());

    const bar = document.createElement("div");
    bar.style.cssText =
      "display:inline-flex;align-items:center;gap:4px;padding:4px;border-radius:11px;border:1px solid rgba(76,154,255,0.38);background:rgba(14,19,25,0.88);backdrop-filter:blur(12px);box-shadow:0 8px 24px rgba(0,0,0,0.45);";

    const button = document.createElement("button");
    button.type = "button";
    button.title = "Download with LajuFile";
    button.style.cssText =
      'display:inline-flex;align-items:center;gap:7px;padding:0 12px 0 8px;height:26px;border-radius:7px;border:0;background:linear-gradient(135deg,#4c9aff,#2563eb);color:#ffffff;font:600 12px "Segoe UI",system-ui,sans-serif;cursor:pointer;box-shadow:0 2px 10px rgba(37,99,235,0.35);white-space:nowrap;';

    const labelEl = document.createElement("span");
    labelEl.textContent = "Download with LajuFile";
    button.append(createPlayIcon(), labelEl);

    const helpBtn = document.createElement("button");
    styleSmallControl(helpBtn);
    helpBtn.textContent = "?";
    helpBtn.title = "LajuFile Video Capture Help";

    const closeBtn = document.createElement("button");
    styleSmallControl(closeBtn);
    closeBtn.textContent = "×";
    closeBtn.title = "Hide panel for this video";

    const menu = document.createElement("div");
    menu.style.cssText =
      "display:none;position:absolute;top:100%;right:0;margin-top:6px;background:rgba(18,24,33,0.96);backdrop-filter:blur(14px);color:#e7edf5;border:1px solid rgba(148,163,184,0.22);border-radius:12px;box-shadow:0 16px 36px rgba(0,0,0,0.55);min-width:min(320px, calc(100vw - 24px));max-width:min(480px, calc(100vw - 24px));max-height:min(340px, calc(100vh - 80px));overflow-y:auto;padding:6px;z-index:2147483647;";

    button.addEventListener("click", (event) => {
      if (!event.isTrusted) return;
      event.stopPropagation();
      if (!activeMountState) return;
      if (menu.style.display === "block") {
        menu.style.display = "none";
        return;
      }
      const target = resolveVideoTarget(activeMountState.video);
      showFormatsOrFallback(menu, target);
    });

    helpBtn.addEventListener("click", (event) => {
      if (!event.isTrusted) return;
      event.stopPropagation();
      showStatusRow(
        menu,
        'Click "Download with LajuFile" to choose a quality or download this video directly with LajuFile.',
      );
    });

    closeBtn.addEventListener("click", (event) => {
      if (!event.isTrusted) return;
      event.stopPropagation();
      if (activeMountState) {
        dismissTarget(resolveVideoTarget(activeMountState.video));
      }
      unmountPanel();
    });

    bar.append(button, helpBtn, closeBtn);
    wrap.append(bar, menu);
    return { wrap, bar, button, labelEl, menu };
  }

  function desiredMountContainer() {
    const fs = document.fullscreenElement;
    if (fs && fs.tagName !== "VIDEO" && fs.tagName !== "IFRAME") {
      return fs;
    }
    return document.documentElement;
  }

  function applyLayout(state, mode, host, rect) {
    const { wrap, labelEl, menu } = state;
    if (mode === "youtube-watch") {
      if (wrap.parentElement !== host) {
        host.append(wrap);
      }
      wrap.style.cssText =
        "position:absolute;top:12px;right:14px;z-index:9999;pointer-events:auto;user-select:none;font-family:'Segoe UI',system-ui,sans-serif;";
      labelEl.textContent = "Download with LajuFile";
      menu.style.left = "auto";
      menu.style.right = "0";
      menu.style.minWidth = "min(440px, calc(100vw - 28px))";
      return;
    }

    const container = desiredMountContainer();
    if (!container) return;
    if (wrap.parentElement !== container) {
      container.append(wrap);
    }
    const vw = window.innerWidth || document.documentElement.clientWidth || 1024;
    const top = Math.max(8, Math.round(Math.max(rect.top, 0) + 12));
    const right = Math.max(8, Math.round(vw - Math.min(rect.right, vw) + 12));
    wrap.style.cssText = `position:fixed;top:${top}px;right:${right}px;z-index:2147483647;pointer-events:auto;user-select:none;font-family:'Segoe UI',system-ui,sans-serif;`;
    labelEl.textContent = rect.width < 360 ? "Download" : "Download with LajuFile";

    if (rect.right < 340) {
      menu.style.right = "auto";
      menu.style.left = "0";
    } else {
      menu.style.left = "auto";
      menu.style.right = "0";
    }
    menu.style.minWidth = "min(320px, calc(100vw - 24px))";
  }

  function syncPanel() {
    if (isCurrentSiteExcluded()) {
      unmountPanel();
      return;
    }

    if (isYouTubeWatchPage()) {
      const host = document.getElementById("movie_player");
      if (!host) {
        unmountPanel();
        return;
      }
      const target = resolveVideoTarget(host.querySelector("video"));
      if (isTargetDismissed(target)) {
        unmountPanel();
        return;
      }
      const targetKey = target.specificKey || location.href;
      if (
        activeMountState &&
        activeMountState.mode === "youtube-watch" &&
        activeMountState.targetKey === targetKey &&
        activeMountState.wrap.isConnected
      ) {
        applyLayout(activeMountState, "youtube-watch", host, null);
        return;
      }
      unmountPanel();
      const els = createPanelElements();
      activeMountState = {
        mode: "youtube-watch",
        video: target.video,
        targetKey,
        ...els,
      };
      applyLayout(activeMountState, "youtube-watch", host, null);
      return;
    }

    const best = findBestActiveVideo();
    if (!best) {
      unmountPanel();
      return;
    }
    const target = resolveVideoTarget(best.video);
    if (isTargetDismissed(target)) {
      unmountPanel();
      return;
    }
    const targetKey =
      target.specificKey || best.video.currentSrc || best.video.src || location.href;
    if (
      activeMountState &&
      activeMountState.mode === "fixed" &&
      activeMountState.video === best.video &&
      activeMountState.targetKey === targetKey &&
      activeMountState.wrap.isConnected
    ) {
      applyLayout(activeMountState, "fixed", null, best.rect);
      return;
    }

    const preserveOpen =
      activeMountState &&
      activeMountState.mode === "fixed" &&
      activeMountState.video === best.video &&
      activeMountState.targetKey === targetKey;
    if (!preserveOpen) {
      unmountPanel();
      const els = createPanelElements();
      activeMountState = {
        mode: "fixed",
        video: best.video,
        targetKey,
        ...els,
      };
    }
    applyLayout(activeMountState, "fixed", null, best.rect);
  }

  let rafPending = false;
  let domTimer = null;
  function scheduleSync(immediate = false) {
    if (immediate) {
      if (rafPending) return;
      rafPending = true;
      requestAnimationFrame(() => {
        rafPending = false;
        syncPanel();
      });
      return;
    }
    if (domTimer !== null) return;
    domTimer = setTimeout(() => {
      domTimer = null;
      scheduleSync(true);
    }, 400);
  }

  document.addEventListener("click", () => {
    if (activeMountState && activeMountState.menu) {
      activeMountState.menu.style.display = "none";
    }
  });

  document.addEventListener(
    "mouseover",
    (event) => {
      const target = event.target;
      const vid =
        target && target.tagName === "VIDEO"
          ? target
          : target && typeof target.closest === "function"
            ? target.closest("video")
            : null;
      if (vid && vid !== hoveredVideo) {
        hoveredVideo = vid;
        scheduleSync(true);
      }
    },
    true,
  );

  for (const evt of ["play", "playing", "loadedmetadata", "pause"]) {
    document.addEventListener(
      evt,
      (event) => {
        if (event.target && event.target.tagName === "VIDEO") {
          scheduleSync(true);
        }
      },
      true,
    );
  }

  window.addEventListener("scroll", () => scheduleSync(true), { capture: true, passive: true });
  window.addEventListener("resize", () => scheduleSync(true), { passive: true });
  document.addEventListener("fullscreenchange", () => scheduleSync(true));
  window.addEventListener("yt-navigate-finish", () => scheduleSync(true));
  window.addEventListener("popstate", () => scheduleSync(true));

  if (document.documentElement) {
    new MutationObserver(() => scheduleSync(false)).observe(document.documentElement, {
      childList: true,
      subtree: true,
    });
  }

  initResourceObserver();
  syncExclusions();
  syncPanel();
})();
