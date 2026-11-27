// Adds an IDM-style "Unduh video ini" floating panel to the YouTube player and
// sends the picked quality to the LajuFile desktop app through the background worker.
const PANEL_ID = "lajufile-video-button";
let dismissedVideoId = null;

function currentVideoId() {
  if (location.pathname !== "/watch") return null;
  return new URLSearchParams(location.search).get("v");
}

function pageVideoTitle() {
  const heading = document.querySelector(
    "h1.ytd-watch-metadata yt-formatted-string, h1.title yt-formatted-string",
  );
  if (heading && heading.textContent && heading.textContent.trim()) {
    return heading.textContent.trim();
  }
  return document.title.replace(/\s*-\s*YouTube$/i, "").trim() || "video";
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
  return "Could not connect to LajuFile. Make sure LajuFile Desktop is running.";
}

function showStatusRow(menu, message, isError = false) {
  menu.textContent = "";
  const row = document.createElement("div");
  row.textContent = message;
  row.style.cssText = `padding:10px 14px;color:${isError ? "#ff6b6b" : "#cbd5e1"};font:12px/1.4 "Segoe UI",system-ui,sans-serif;`;
  menu.append(row);
  menu.style.display = "block";
}

function styleMenuRow(row) {
  row.style.cssText =
    'display:flex;align-items:center;justify-content:space-between;gap:16px;width:100%;box-sizing:border-box;background:transparent;color:#e7edf5;border:0;border-radius:7px;padding:7px 11px;cursor:pointer;font:12px "Segoe UI",system-ui,sans-serif;text-align:left;white-space:nowrap;transition:background 0.14s ease;';
  row.addEventListener("mouseenter", () => {
    row.style.background = "rgba(76,154,255,0.16)";
  });
  row.addEventListener("mouseleave", () => {
    row.style.background = "transparent";
  });
}

async function download(url, selector, label, title, menu, sizeBytes) {
  showStatusRow(menu, `Sending "${truncateTitle(title, 28)}" (${label}) to LajuFile...`);
  const ext = /audio/i.test(label) ? "m4a" : "mp4";
  const baseTitle = title || pageVideoTitle();
  const result = await chrome.runtime.sendMessage({
    type: "video-download",
    url,
    selector,
    filename: baseTitle ? `${baseTitle}.${ext}` : undefined,
    size: typeof sizeBytes === "number" && sizeBytes > 0 ? sizeBytes : undefined,
  });
  const status = result ? result.status : 0;
  if (status === 401) {
    showStatusRow(menu, "Pair the LajuFile extension token in Options first.", true);
  } else if (status === 409) {
    showStatusRow(menu, "This video is already in LajuFile.");
  } else if (status !== 201) {
    showStatusRow(menu, errorText(result), true);
  } else {
    showStatusRow(menu, `Added to LajuFile (${label}).`);
    setTimeout(() => {
      menu.style.display = "none";
    }, 1500);
  }
}

async function showFormats(menu, url) {
  showStatusRow(menu, "Loading video qualities...");
  const result = await chrome.runtime.sendMessage({ type: "video-formats", url });
  const formats = result && result.body ? result.body.formats : null;
  const rawTitle =
    result && result.body && result.body.title && result.body.title !== "video"
      ? result.body.title
      : pageVideoTitle();
  if (!Array.isArray(formats) || formats.length === 0) {
    showStatusRow(menu, errorText(result), true);
    return;
  }

  menu.textContent = "";
  const shortTitle = truncateTitle(rawTitle, 36);

  // Top "Download best" action row
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
    event.stopPropagation();
    const best = formats[0];
    download(url, best.selector, best.label, rawTitle, menu, best.size);
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
      event.stopPropagation();
      download(url, format.selector, format.label, rawTitle, menu, format.size);
    });
    menu.append(item);
  });
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

function mount() {
  const videoId = currentVideoId();
  const existing = document.getElementById(PANEL_ID);
  if (!videoId) {
    existing?.remove();
    return;
  }
  if (dismissedVideoId && dismissedVideoId !== videoId) {
    dismissedVideoId = null;
  }
  if (dismissedVideoId === videoId) {
    existing?.remove();
    return;
  }
  const host = document.getElementById("movie_player");
  if (!host || existing) return;

  const wrap = document.createElement("div");
  wrap.id = PANEL_ID;
  wrap.style.cssText =
    "position:absolute;top:12px;right:14px;z-index:9999;pointer-events:auto;user-select:none;font-family:'Segoe UI',system-ui,sans-serif;";
  wrap.addEventListener("click", (event) => event.stopPropagation());

  // Modern dark-glass floating pill bar matching LajuFile Desktop
  const bar = document.createElement("div");
  bar.style.cssText =
    "display:inline-flex;align-items:center;gap:4px;padding:4px;border-radius:11px;border:1px solid rgba(76,154,255,0.38);background:rgba(14,19,25,0.88);backdrop-filter:blur(12px);box-shadow:0 8px 24px rgba(0,0,0,0.45);";

  const button = document.createElement("button");
  button.type = "button";
  button.style.cssText =
    'display:inline-flex;align-items:center;gap:7px;padding:0 12px 0 8px;height:26px;border-radius:7px;border:0;background:linear-gradient(135deg,#4c9aff,#2563eb);color:#ffffff;font:600 12px "Segoe UI",system-ui,sans-serif;cursor:pointer;box-shadow:0 2px 10px rgba(37,99,235,0.35);';

  const label = document.createElement("span");
  label.textContent = "Download with LajuFile";
  button.append(createPlayIcon(), label);

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
    "display:none;position:absolute;top:100%;right:0;margin-top:6px;background:rgba(18,24,33,0.96);backdrop-filter:blur(14px);color:#e7edf5;border:1px solid rgba(148,163,184,0.22);border-radius:12px;box-shadow:0 16px 36px rgba(0,0,0,0.55);min-width:min(480px, calc(100vw - 28px));max-width:min(600px, calc(100vw - 28px));max-height:340px;overflow-y:auto;padding:6px;z-index:10000;";

  button.addEventListener("click", (event) => {
    event.stopPropagation();
    if (menu.style.display === "block") {
      menu.style.display = "none";
      return;
    }
    showFormats(menu, location.href);
  });

  helpBtn.addEventListener("click", (event) => {
    event.stopPropagation();
    showStatusRow(
      menu,
      'Click "Download with LajuFile" to choose a video resolution and download it with LajuFile.',
    );
  });

  closeBtn.addEventListener("click", (event) => {
    event.stopPropagation();
    dismissedVideoId = currentVideoId();
    wrap.remove();
  });

  bar.append(button, helpBtn, closeBtn);
  wrap.append(bar, menu);
  host.append(wrap);
}

document.addEventListener("click", () => {
  const wrap = document.getElementById(PANEL_ID);
  if (!wrap) return;
  const menu = wrap.querySelector("div:nth-child(2)");
  if (menu) menu.style.display = "none";
});

// YouTube swaps the player on in-page navigation, so mount on navigation and debounced DOM updates.
let mountTimer = null;
function scheduleMount() {
  if (mountTimer !== null) return;
  mountTimer = setTimeout(() => {
    mountTimer = null;
    if (/^\/(watch|shorts\/)/.test(location.pathname) && !document.getElementById(PANEL_ID)) {
      mount();
    }
  }, 350);
}

window.addEventListener("yt-navigate-finish", mount);
window.addEventListener("popstate", mount);
if (document.documentElement) {
  new MutationObserver(scheduleMount).observe(document.documentElement, {
    childList: true,
    subtree: true,
  });
}
mount();
