const DEFAULTS = {
  port: 53841,
  token: "",
  autoCapture: true,
  excludedDomains: "",
};
const $ = (id) => document.getElementById(id);
let activeHost = "";

function parseTokens(raw) {
  return String(raw || "")
    .split(/[\s,;]+/)
    .map((t) => t.trim().toLowerCase())
    .filter(Boolean);
}

function isHostExcluded(host, rawList) {
  if (!host) return false;
  const h = host.toLowerCase();
  return parseTokens(rawList).some((t) => {
    const clean = t.replace(/^\*\.|^\./, "");
    return clean && (h === clean || h.endsWith(`.${clean}`));
  });
}

async function probeBridge(port, token) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 2000);
  try {
    const health = await fetch(`http://127.0.0.1:${port}/health`, {
      signal: controller.signal,
      cache: "no-store",
    });
    if (!health.ok) return { reachable: false, paired: false };
    if (!token) return { reachable: true, paired: false };
    const rules = await fetch(`http://127.0.0.1:${port}/rules`, {
      headers: { "x-lajufile-token": token },
      signal: controller.signal,
      cache: "no-store",
    });
    return { reachable: true, paired: rules.ok };
  } catch {
    return { reachable: false, paired: false };
  } finally {
    clearTimeout(timer);
  }
}

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
    if (tab && tab.url && /^https?:/i.test(tab.url)) {
      const host = new URL(tab.url).hostname.toLowerCase();
      if (host && host !== "localhost" && host !== "127.0.0.1") {
        activeHost = host;
      }
    }
  } catch {}
}

async function refresh() {
  await loadActiveTabHost();
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
    const data = JSON.parse(raw);
    const port = Number(data.port);
    const token = String(data.token || "").trim();
    if (!Number.isInteger(port) || port < 1024 || port > 65535) {
      throw new Error("Invalid port in Quick-Pair code.");
    }
    if (!/^[0-9a-fA-F]{64}$/.test(token)) {
      throw new Error("Invalid token in Quick-Pair code.");
    }
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
});
