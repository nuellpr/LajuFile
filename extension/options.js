const DEFAULTS = {
  port: 53841,
  token: "",
  autoCapture: true,
  excludedDomains: "",
  fileTypes: "",
};
const $ = (id) => document.getElementById(id);

function setStatus(text, kind) {
  const status = $("status");
  status.textContent = text;
  status.className = `status${kind ? ` ${kind}` : ""}`;
}

function setPill(text, kind) {
  const pill = $("connPill");
  pill.textContent = text;
  pill.className = `pill ${kind}`;
}

function parsePort(value) {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) {
    throw new Error("Port must be between 1024 and 65535.");
  }
  return port;
}

async function testBridge(port, token) {
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

async function applyQuickPair(raw) {
  let data;
  try {
    data = JSON.parse(String(raw || "").trim());
  } catch {
    setStatus("Quick-Pair expects JSON from LajuFile Setup Wizard.", "bad");
    return false;
  }
  let port;
  try {
    port = parsePort(data.port);
  } catch (error) {
    setStatus(error.message, "bad");
    return false;
  }
  const token = String(data.token || "").trim();
  if (!/^[0-9a-fA-F]{64}$/.test(token)) {
    setStatus("Quick-Pair token must be 64 hexadecimal characters.", "bad");
    return false;
  }
  $("port").value = port;
  $("token").value = token;
  $("quickPairInput").value = "";
  await persistCurrentSettings();
  await test();
  return true;
}

async function load() {
  const stored = await chrome.storage.local.get(DEFAULTS);
  $("port").value = stored.port;
  $("token").value = stored.token;
  $("autoCapture").checked = stored.autoCapture !== false;
  $("fileTypes").value = stored.fileTypes || "";
  $("excludedDomains").value = stored.excludedDomains || "";
  syncChips();
  const token = String(stored.token || "").trim();
  const res = await testBridge(Number(stored.port) || DEFAULTS.port, token);
  if (res.paired) {
    setPill("Paired & Online", "ok");
  } else if (res.reachable) {
    setPill(token ? "Token mismatch" : "Bridge online · Unpaired", "pending");
  } else {
    setPill("Desktop offline", "bad");
  }
}

async function persistCurrentSettings() {
  const port = parsePort($("port").value);
  const token = $("token").value.trim();
  if (token && !/^[0-9a-fA-F]{64}$/.test(token)) {
    throw new Error("Token must be 64 hexadecimal characters. Copy it from LajuFile Settings.");
  }
  await chrome.storage.local.set({
    port,
    token,
    autoCapture: $("autoCapture").checked,
    fileTypes: $("fileTypes").value.trim(),
    excludedDomains: $("excludedDomains").value.trim(),
  });
  await chrome.runtime.sendMessage({ type: "config-updated" });
  return { port, token };
}

async function save(event) {
  event.preventDefault();
  try {
    await persistCurrentSettings();
    setStatus("Saved. Changes apply immediately on the next download.", "ok");
    await test();
  } catch (error) {
    setStatus(error.message, "bad");
  }
}

async function test() {
  let port;
  try {
    port = parsePort($("port").value);
  } catch (error) {
    setStatus(error.message, "bad");
    return;
  }
  const token = $("token").value.trim();
  setStatus("Checking connection & pairing…");
  const { reachable, paired } = await testBridge(port, token);
  if (paired) {
    setPill("Paired & Online", "ok");
    setStatus(`Paired cleanly with LajuFile on 127.0.0.1:${port}.`, "ok");
  } else if (reachable && !token) {
    setPill("Bridge online · Unpaired", "pending");
    setStatus("Bridge is reachable. Paste the Quick-Pair code or Access Token to finish pairing.", "bad");
  } else if (reachable) {
    setPill("Token mismatch", "bad");
    setStatus("Bridge reachable, but Access Token was rejected (401). Re-copy from LajuFile.", "bad");
  } else {
    setPill("Desktop offline", "bad");
    setStatus(`No LajuFile bridge on 127.0.0.1:${port}. Launch LajuFile Desktop and retry.`, "bad");
  }
}

function parseExtList(raw) {
  return String(raw || "")
    .split(/[\s,;]+/)
    .map((s) => s.trim().replace(/^\./, "").toLowerCase())
    .filter(Boolean);
}

function syncChips() {
  const active = new Set(parseExtList($("fileTypes").value));
  document.querySelectorAll("#extChips .chip").forEach((chip) => {
    const ext = chip.dataset.ext;
    chip.classList.toggle("active", active.has(ext));
  });
}

document.addEventListener("DOMContentLoaded", () => {
  load().catch(() => setStatus("Could not load saved settings.", "bad"));
  $("save").addEventListener("click", save);
  $("test").addEventListener("click", test);
  $("fileTypes").addEventListener("input", syncChips);
  document.querySelectorAll("#extChips .chip").forEach((chip) => {
    chip.addEventListener("click", () => {
      const ext = chip.dataset.ext;
      const list = parseExtList($("fileTypes").value);
      const next = list.includes(ext) ? list.filter((e) => e !== ext) : [...list, ext];
      $("fileTypes").value = next.join(", ");
      syncChips();
    });
  });
  $("applyPair").addEventListener("click", () => {
    applyQuickPair($("quickPairInput").value).catch(() => {});
  });
  $("token").addEventListener("paste", (event) => {
    const pasted = event.clipboardData ? event.clipboardData.getData("text") : "";
    if (pasted && pasted.trim().startsWith("{")) {
      event.preventDefault();
      applyQuickPair(pasted).catch(() => {});
    }
  });
});
