const {
  DEFAULTS,
  parseTokens,
  isHostExcluded,
  isSupportedVideoPage,
  parseQuickPair,
  probeBridge,
} = globalThis.LajuFileCommon;
const $ = (id) => document.getElementById(id);
let activeHost = "";
let activeTab = null;
let scannedAssets = [];
const selectedAssetUrls = new Set();

function setStatus(reachable, paired, port, hasToken) {
  $("dot").className = `dot ${reachable ? "ok" : "bad"}`;
  $("state").textContent = reachable ? "Desktop bridge online" : "LajuFile not running";
  $("detail").textContent = `127.0.0.1:${port}`;
  const badge = $("pairBadge");
  if (paired) {
    badge.textContent = "Paired";
    badge.className = "badge ok";
  } else if (hasToken) {
    badge.textContent = reachable ? "Token mismatch" : "Saved";
    badge.className = "badge pending";
  } else {
    badge.textContent = "Unpaired";
    badge.className = "badge pending";
  }
  $("quickPairBox").classList.toggle("hidden", paired && hasToken);
}

function updateSiteButton(excludedRaw) {
  if (!activeHost) {
    $("siteCard").classList.add("hidden");
    return;
  }
  $("siteCard").classList.remove("hidden");
  $("siteHost").textContent = activeHost;
  const paused = isHostExcluded(activeHost, excludedRaw);
  const btn = $("siteToggle");
  btn.textContent = paused ? "Paused · Resume" : "Pause on site";
  btn.className = paused ? "ghost-btn paused" : "ghost-btn";
}

async function loadActiveTabHost() {
  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    activeTab = tab || null;
    if (tab && tab.url && /^https?:/i.test(tab.url)) {
      const host = new URL(tab.url).hostname.toLowerCase();
      if (host && host !== "localhost" && host !== "127.0.0.1") {
        activeHost = host;
      }
    }
  } catch {}
}

async function refreshPageMedia() {
  const card = $("mediaCard");
  const list = $("mediaList");
  const feedback = $("mediaFeedback");
  list.textContent = "";
  feedback.textContent = "";
  if (!activeTab || !activeTab.url || !/^https?:/i.test(activeTab.url)) {
    card.classList.add("hidden");
    return;
  }
  const entries = [];
  if (isSupportedVideoPage(activeTab.url)) {
    entries.push({
      url: activeTab.url,
      label: activeTab.title ? activeTab.title.slice(0, 36) : "Download page video",
      badge: "yt-dlp",
    });
  }
  if (typeof activeTab.id === "number") {
    try {
      const res = await chrome.tabs.sendMessage(activeTab.id, { type: "sniff-page-media" });
      if (res && Array.isArray(res.items)) {
        for (const item of res.items) {
          if (!entries.some((e) => e.url === item.url)) {
            entries.push({ url: item.url, label: item.label, badge: item.kind });
          }
        }
      }
    } catch {}
  }
  if (entries.length === 0) {
    card.classList.add("hidden");
    return;
  }
  card.classList.remove("hidden");
  for (const entry of entries.slice(0, 4)) {
    const row = document.createElement("div");
    row.className = "media-item";
    const lbl = document.createElement("span");
    lbl.className = "media-label";
    lbl.textContent = entry.label;
    lbl.title = entry.url;
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ghost-btn";
    btn.textContent = `Download (${entry.badge})`;
    btn.addEventListener("click", async () => {
      feedback.textContent = "Sending…";
      const res = await chrome.runtime.sendMessage({
        type: "capture-media",
        url: entry.url,
        referrer: activeTab.url,
      });
      if (res && res.status === 201) {
        feedback.textContent = "Added!";
      } else if (res && res.status === 409) {
        feedback.textContent = "Already queued";
      } else {
        feedback.textContent = (res && res.body && res.body.error) || "Failed";
      }
    });
    row.append(lbl, btn);
    list.append(row);
  }
}

async function refresh() {
  await loadActiveTabHost();
  await refreshPageMedia();
  const stored = await chrome.storage.local.get(DEFAULTS);
  const port = Number(stored.port) || DEFAULTS.port;
  const token = String(stored.token || "").trim();
  $("autoCapture").checked = stored.autoCapture !== false;
  updateSiteButton(stored.excludedDomains);
  setStatus(false, false, port, Boolean(token));
  const { reachable, paired } = await probeBridge(port, token);
  setStatus(reachable, paired, port, Boolean(token));
}

async function toggleSiteExclusion() {
  if (!activeHost) return;
  const stored = await chrome.storage.local.get({ excludedDomains: "" });
  const tokens = parseTokens(stored.excludedDomains);
  let next;
  if (isHostExcluded(activeHost, stored.excludedDomains)) {
    next = tokens.filter((t) => {
      const clean = t.replace(/^\*\.|^\./, "");
      return !(activeHost === clean || activeHost.endsWith(`.${clean}`));
    });
  } else {
    next = [...tokens, activeHost];
  }
  const excludedDomains = next.join(", ");
  await chrome.storage.local.set({ excludedDomains });
  await chrome.runtime.sendMessage({ type: "config-updated" });
  updateSiteButton(excludedDomains);
}

async function applyQuickPair() {
  const raw = $("pairCode").value.trim();
  const feedback = $("pairFeedback");
  if (!raw) return;
  try {
    const { port, token } = parseQuickPair(raw);
    await chrome.storage.local.set({ port, token });
    await chrome.runtime.sendMessage({ type: "config-updated" });
    const check = await probeBridge(port, token);
    feedback.textContent = check.paired
      ? "Paired with LajuFile Desktop."
      : "Saved. Start LajuFile Desktop to verify.";
    feedback.className = `pair-feedback ${check.paired ? "ok" : ""}`;
    setStatus(check.reachable, check.paired, port, true);
  } catch (err) {
    feedback.textContent = err && err.message ? err.message : "Invalid Quick-Pair JSON code.";
    feedback.className = "pair-feedback bad";
  }
}

document.addEventListener("DOMContentLoaded", () => {
  refresh().catch(() => setStatus(false, false, DEFAULTS.port, false));
  $("test").addEventListener("click", () => {
    $("state").textContent = "Checking bridge…";
    $("dot").className = "dot";
    refresh().catch(() => setStatus(false, false, DEFAULTS.port, false));
  });
  $("siteToggle").addEventListener("click", () => {
    toggleSiteExclusion().catch(() => {});
  });
  $("pairApply").addEventListener("click", () => {
    applyQuickPair().catch(() => {});
  });
  $("options").addEventListener("click", () => {
    chrome.runtime.openOptionsPage();
    window.close();
  });
  $("autoCapture").addEventListener("change", async (event) => {
    await chrome.storage.local.set({ autoCapture: event.target.checked });
    await chrome.runtime.sendMessage({ type: "config-updated" });
  });
  $("scanAssets").addEventListener("click", () => {
    scanPageAssets().catch(() => {});
  });
  $("grabKind").addEventListener("change", renderGrabList);
  $("grabMinWidth").addEventListener("change", renderGrabList);
  $("grabSelectAll").addEventListener("change", (event) => {
    const checked = event.target.checked;
    for (const item of filteredGrabAssets()) {
      if (checked) selectedAssetUrls.add(item.url);
      else selectedAssetUrls.delete(item.url);
    }
    renderGrabList();
  });
  $("grabDownload").addEventListener("click", () => {
    downloadSelectedAssets().catch(() => {});
  });
});

function filteredGrabAssets() {
  const kindFilter = $("grabKind").value;
  const minW = Number($("grabMinWidth").value) || 0;
  return scannedAssets.filter((item) => {
    if (kindFilter === "image" && item.kind !== "image") return false;
    if (kindFilter === "doc" && item.kind !== "doc" && item.kind !== "archive") return false;
    if (kindFilter === "media" && item.kind !== "media") return false;
    if (minW > 0 && item.kind === "image" && item.width > 0 && item.width < minW) return false;
    if (minW > 0 && item.kind === "image" && item.width === 0) return false;
    return true;
  });
}

function updateGrabDownloadButton(items) {
  const count = items.filter((i) => selectedAssetUrls.has(i.url)).length;
  $("grabDownload").textContent = `Download selected (${count})`;
  $("grabDownload").disabled = count === 0;
}

function renderGrabList() {
  const list = $("grabList");
  list.textContent = "";
  const items = filteredGrabAssets();
  if (items.length === 0) {
    const empty = document.createElement("div");
    empty.className = "mini-hint";
    empty.textContent = "No matching assets found.";
    list.append(empty);
  }
  for (const item of items) {
    const row = document.createElement("label");
    row.className = "grab-item";
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = selectedAssetUrls.has(item.url);
    cb.addEventListener("change", () => {
      if (cb.checked) selectedAssetUrls.add(item.url);
      else selectedAssetUrls.delete(item.url);
      updateGrabDownloadButton(items);
    });
    const name = document.createElement("span");
    name.textContent = item.label;
    name.title = item.url;
    const badge = document.createElement("span");
    badge.className = "grab-badge";
    badge.textContent =
      item.kind === "image" && item.width > 0 ? `${item.width}x${item.height}` : item.kind;
    row.append(cb, name, badge);
    list.append(row);
  }
  $("grabSelectAll").checked = items.length > 0 && items.every((i) => selectedAssetUrls.has(i.url));
  updateGrabDownloadButton(items);
}

async function scanPageAssets() {
  const status = $("grabStatus");
  $("grabBody").classList.remove("hidden");
  status.textContent = "Scanning page…";
  if (!activeTab || typeof activeTab.id !== "number" || !/^https?:/i.test(activeTab.url || "")) {
    scannedAssets = [];
    selectedAssetUrls.clear();
    renderGrabList();
    status.textContent = "Open an HTTP(S) page first.";
    return;
  }
  try {
    const res = await chrome.tabs.sendMessage(activeTab.id, { type: "sniff-page-assets" });
    scannedAssets = res && Array.isArray(res.items) ? res.items : [];
    selectedAssetUrls.clear();
    for (const item of scannedAssets) selectedAssetUrls.add(item.url);
    renderGrabList();
    status.textContent = `${scannedAssets.length} item(s) found`;
  } catch {
    scannedAssets = [];
    selectedAssetUrls.clear();
    renderGrabList();
    status.textContent = "Reload the tab to scan.";
  }
}

async function downloadSelectedAssets() {
  const targets = filteredGrabAssets().filter((i) => selectedAssetUrls.has(i.url));
  if (targets.length === 0) return;
  const status = $("grabStatus");
  const btn = $("grabDownload");
  btn.disabled = true;
  let sent = 0;
  let okCount = 0;
  for (let i = 0; i < targets.length; i += 4) {
    const batch = targets.slice(i, i + 4);
    const replies = await Promise.all(
      batch.map((item) =>
        chrome.runtime
          .sendMessage({
            type: "capture-media",
            url: item.url,
            referrer: activeTab ? activeTab.url : undefined,
          })
          .catch(() => null),
      ),
    );
    sent += batch.length;
    okCount += replies.filter((r) => r && (r.status === 201 || r.status === 409)).length;
    status.textContent = `Sent ${sent}/${targets.length}…`;
  }
  status.textContent = `Queued ${okCount}/${targets.length} in LajuFile.`;
  btn.disabled = false;
}
