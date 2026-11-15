const DEFAULTS = { port: 53841, token: "", autoCapture: true };
const $ = (id) => document.getElementById(id);

function setStatus(text, kind) {
  const status = $("status");
  status.textContent = text;
  status.className = `status${kind ? ` ${kind}` : ""}`;
}

function parsePort(value) {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) {
    throw new Error("Port must be between 1024 and 65535.");
  }
  return port;
}

async function testConnection(port) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 2000);
  try {
    const response = await fetch(`http://127.0.0.1:${port}/health`, {
      signal: controller.signal,
      cache: "no-store",
    });
    return response.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

async function load() {
  const stored = await chrome.storage.local.get(DEFAULTS);
  $("port").value = stored.port;
  $("token").value = stored.token;
  $("autoCapture").checked = stored.autoCapture !== false;
}

async function save(event) {
  event.preventDefault();
  let port;
  try {
    port = parsePort($("port").value);
  } catch (error) {
    setStatus(error.message, "bad");
    return;
  }
  const token = $("token").value.trim();
  if (token && !/^[0-9a-fA-F]{64}$/.test(token)) {
    setStatus("Token must be 64 hexadecimal characters. Copy it from LajuFile Settings.", "bad");
    return;
  }
  await chrome.storage.local.set({
    port,
    token,
    autoCapture: $("autoCapture").checked,
  });
  await chrome.runtime.sendMessage({ type: "config-updated" });
  setStatus("Saved. LajuFile reads this on the next download.", "ok");
}

async function test() {
  let port;
  try {
    port = parsePort($("port").value);
  } catch (error) {
    setStatus(error.message, "bad");
    return;
  }
  setStatus("Checking…");
  const ok = await testConnection(port);
  setStatus(
    ok ? `Connected to LajuFile on port ${port}.` : `No LajuFile bridge on port ${port}.`,
    ok ? "ok" : "bad",
  );
}

document.addEventListener("DOMContentLoaded", () => {
  load().catch(() => setStatus("Could not load saved settings.", "bad"));
  $("save").addEventListener("click", save);
  $("test").addEventListener("click", test);
});
