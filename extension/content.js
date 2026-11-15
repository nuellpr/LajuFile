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

function formatQualityDescription(format) {
  const label = String(format.label || "");
  if (/audio/i.test(label)) {
    return "M4A berkas, quality Audio only";
  }
  const match = label.match(/^(\d+)p$/i);
  if (!match) {
    return `MP4 berkas, quality ${label}`;
  }
  const height = Number(match[1]);
  const hd = height >= 720 ? " HD" : "";
  const fps = format.fps && Number(format.fps) > 30 ? ` ${Number(format.fps)} fps` : "";
  return `MP4 berkas, quality ${label}${hd}${fps}`;
}

function errorText(result) {
  if (result && result.body && result.body.error) return result.body.error;
  return "Tidak dapat terhubung ke LajuFile. Pastikan aplikasi LajuFile sedang berjalan.";
}

function showStatusRow(menu, message, isError = false) {
  menu.textContent = "";
  const row = document.createElement("div");
  row.textContent = message;
  row.style.cssText = `padding:8px 12px;color:${isError ? "#b42318" : "#1d2939"};font:12px "Segoe UI",Tahoma,sans-serif;`;
  menu.append(row);
  menu.style.display = "block";
}

function styleMenuRow(row) {
  row.style.cssText =
    'display:flex;align-items:center;justify-content:space-between;gap:18px;width:100%;box-sizing:border-box;background:transparent;color:#111;border:1px solid transparent;padding:4px 12px;cursor:pointer;font:12px "Segoe UI",Tahoma,sans-serif;text-align:left;white-space:nowrap;';
  row.addEventListener("mouseenter", () => {
    row.style.background = "#e5f1fb";
    row.style.borderColor = "#a8d2f0";
  });
  row.addEventListener("mouseleave", () => {
    row.style.background = "transparent";
    row.style.borderColor = "transparent";
  });
}

async function download(url, selector, label, title, menu) {
  showStatusRow(menu, `Mengirim "${truncateTitle(title, 28)}" (${label}) ke LajuFile...`);
  const ext = /audio/i.test(label) ? "m4a" : "mp4";
  const baseTitle = title || pageVideoTitle();
  const result = await chrome.runtime.sendMessage({
    type: "video-download",
    url,
    selector,
    filename: baseTitle ? `${baseTitle}.${ext}` : undefined,
  });
  const status = result ? result.status : 0;
  if (status === 401) {
    showStatusRow(menu, "Pasangkan token ekstensi LajuFile di menu Options terlebih dahulu.", true);
  } else if (status === 409) {
    showStatusRow(menu, "Video ini sudah ada di antrean LajuFile.");
  } else if (status !== 201) {
    showStatusRow(menu, errorText(result), true);
  } else {
    showStatusRow(menu, `Berhasil ditambahkan ke LajuFile (${label}).`);
    setTimeout(() => {
      menu.style.display = "none";
    }, 1500);
  }
}

async function showFormats(menu, url) {
  showStatusRow(menu, "Memuat daftar kualitas video...");
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

  // Top "Unduh semua" action row (queues best available quality)
  const allRow = document.createElement("button");
  allRow.type = "button";
  styleMenuRow(allRow);
  allRow.style.paddingLeft = "22px";
  allRow.style.marginBottom = "2px";
  allRow.style.borderBottom = "1px solid #e4e7ec";
  allRow.textContent = "Unduh semua";
  allRow.addEventListener("click", (event) => {
    event.stopPropagation();
    const best = formats[0];
    download(url, best.selector, best.label, rawTitle, menu);
  });
  menu.append(allRow);

  formats.forEach((format, index) => {
    const item = document.createElement("button");
    item.type = "button";
    styleMenuRow(item);

    const left = document.createElement("span");
    left.textContent = `${index + 1}. ${shortTitle}`;
    left.style.cssText = "overflow:hidden;text-overflow:ellipsis;color:#111;";

    const right = document.createElement("span");
    right.textContent = formatQualityDescription(format);
    right.style.cssText = "color:#222;flex-shrink:0;";

    item.append(left, right);
    item.addEventListener("click", (event) => {
      event.stopPropagation();
      download(url, format.selector, format.label, rawTitle, menu);
    });
    menu.append(item);
  });
}

function createPlayIcon() {
  const badge = document.createElement("span");
  badge.style.cssText =
    "display:inline-flex;align-items:center;justify-content:center;width:16px;height:16px;border-radius:3px;background:linear-gradient(180deg,#5ee649 0%,#1ca814 55%,#127d0c 100%);border:1px solid #0e6309;box-shadow:inset 0 1px 0 rgba(255,255,255,0.65);flex-shrink:0;";
  const tri = document.createElement("span");
  tri.style.cssText =
    "display:block;width:0;height:0;border-top:4px solid transparent;border-bottom:4px solid transparent;border-left:7px solid #ffffff;margin-left:2px;";
  badge.append(tri);
  return badge;
}

function styleSmallControl(btn) {
  btn.type = "button";
  btn.style.cssText =
    'width:18px;height:20px;border-radius:3px;border:1px solid #6b8db8;background:linear-gradient(180deg,#f5f9ff 0%,#c7dcf4 52%,#a9c6e8 100%);color:#1e467a;font:bold 11px "Segoe UI",Tahoma,sans-serif;cursor:pointer;display:inline-flex;align-items:center;justify-content:center;padding:0;line-height:1;box-shadow:inset 0 1px 0 rgba(255,255,255,0.8);';
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
    "position:absolute;top:10px;right:12px;z-index:9999;pointer-events:auto;user-select:none;font-family:'Segoe UI',Tahoma,sans-serif;";
  wrap.addEventListener("click", (event) => event.stopPropagation());

  // IDM-style glossy blue-silver bar
  const bar = document.createElement("div");
  bar.style.cssText =
    "display:inline-flex;align-items:center;gap:3px;padding:2px 3px;border-radius:5px;border:1px solid #698bb7;background:linear-gradient(180deg,#ebf3fc 0%,#c4d9f2 48%,#a5c2e7 52%,#d5e5f7 100%);box-shadow:0 2px 7px rgba(0,0,0,0.35),inset 0 1px 0 rgba(255,255,255,0.85);";

  const button = document.createElement("button");
  button.type = "button";
  button.style.cssText =
    'display:inline-flex;align-items:center;gap:6px;padding:2px 10px 2px 6px;height:20px;border-radius:4px;border:1px solid #7899c2;background:linear-gradient(180deg,#f9fcff 0%,#d7e7f9 48%,#bbd5f1 52%,#e7f2fd 100%);color:#195d30;font:600 12px "Segoe UI",Tahoma,sans-serif;cursor:pointer;text-shadow:0 1px 0 rgba(255,255,255,0.9);box-shadow:inset 0 1px 0 rgba(255,255,255,0.9);';

  const label = document.createElement("span");
  label.textContent = "Unduh video ini";
  button.append(createPlayIcon(), label);

  const helpBtn = document.createElement("button");
  styleSmallControl(helpBtn);
  helpBtn.textContent = "?";
  helpBtn.title = "Bantuan LajuFile Video Capture";

  const closeBtn = document.createElement("button");
  styleSmallControl(closeBtn);
  closeBtn.textContent = "×";
  closeBtn.title = "Sembunyikan panel untuk video ini";

  const menu = document.createElement("div");
  menu.style.cssText =
    "display:none;position:absolute;top:100%;right:0;margin-top:2px;background:#ffffff;color:#111111;border:1px solid #7f9db9;box-shadow:0 4px 14px rgba(0,0,0,0.38);min-width:460px;max-width:560px;max-height:320px;overflow-y:auto;padding:3px 0;z-index:10000;";

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
      'Klik "Unduh video ini" untuk memilih resolusi video dan mengunduhnya dengan LajuFile.',
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

// YouTube swaps the player on in-page navigation, so re-mount on a slow timer.
setInterval(mount, 1000);
mount();
