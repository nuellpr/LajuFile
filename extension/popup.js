const DEFAULTS = { port: 53841, token: "", autoCapture: true };
const $ = (id) => document.getElementById(id);

async function health(port) {
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

function setStatus(ok, port) {
  $("dot").className = `dot ${ok ? "ok" : "bad"}`;
  $("state").textContent = ok ? "Connected" : "Not connected";
  $("detail").textContent = `Port ${port}`;
}

async function load() {
  const stored = await chrome.storage.local.get(DEFAULTS);
  const port = Number(stored.port) || DEFAULTS.port;
  $("autoCapture").checked = stored.autoCapture !== false;
  setStatus(false, port);
  const ok = await health(port);
  setStatus(ok, port);
}

async function test() {
  const stored = await chrome.storage.local.get(DEFAULTS);
  const port = Number(stored.port) || DEFAULTS.port;
  $("state").textContent = "Checking…";
  $("dot").className = "dot";
  const ok = await health(port);
  setStatus(ok, port);
}

document.addEventListener("DOMContentLoaded", () => {
  load().catch(() => setStatus(false, DEFAULTS.port));
  $("test").addEventListener("click", test);
  $("options").addEventListener("click", () => {
    chrome.runtime.openOptionsPage();
    window.close();
  });
  $("autoCapture").addEventListener("change", async (event) => {
    await chrome.storage.local.set({ autoCapture: event.target.checked });
    await chrome.runtime.sendMessage({ type: "config-updated" });
  });
});
