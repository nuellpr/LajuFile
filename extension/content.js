// Adds a "Download with LajuFile" button to the YouTube player and sends the
// picked quality to the desktop app through the background worker.
const BUTTON_ID = "lajufile-video-button";

function isWatchPage() {
  return location.pathname === "/watch" && new URLSearchParams(location.search).has("v");
}

function styleButton(element) {
  element.style.cssText =
    "background:#c4302b;color:#fff;border:0;border-radius:4px;padding:4px 8px;cursor:pointer;font:12px system-ui;text-align:left;display:block;width:100%;";
}

function errorText(result) {
  if (result && result.body && result.body.error) return result.body.error;
  return "Could not reach LajuFile. Is it running?";
}

async function download(url, selector, label, menu) {
  const result = await chrome.runtime.sendMessage({
    type: "video-download",
    url,
    selector,
  });
  const status = result ? result.status : 0;
  let text = "Added to LajuFile";
  if (status === 401) text = "Pair the extension in its options";
  else if (status === 409) text = "Already in LajuFile";
  else if (status !== 201) text = errorText(result);
  else text = `Queued ${label}`;
  menu.textContent = text;
  menu.style.display = "block";
}

async function showFormats(menu, url) {
  menu.textContent = "Loading qualities...";
  menu.style.display = "block";
  const result = await chrome.runtime.sendMessage({ type: "video-formats", url });
  const formats = result && result.body ? result.body.formats : null;
  if (!Array.isArray(formats) || formats.length === 0) {
    menu.textContent = errorText(result);
    return;
  }
  menu.textContent = "";
  for (const format of formats) {
    const item = document.createElement("button");
    item.textContent = format.label;
    styleButton(item);
    item.addEventListener("click", (event) => {
      event.stopPropagation();
      download(url, format.selector, format.label, menu);
    });
    menu.append(item);
  }
}

function mount() {
  if (!isWatchPage()) {
    document.getElementById(BUTTON_ID)?.remove();
    return;
  }
  const host = document.getElementById("movie_player");
  if (!host || document.getElementById(BUTTON_ID)) return;

  const wrap = document.createElement("div");
  wrap.id = BUTTON_ID;
  wrap.style.cssText =
    "position:absolute;top:12px;right:12px;z-index:9999;max-width:220px;";
  wrap.addEventListener("click", (event) => event.stopPropagation());

  const button = document.createElement("button");
  button.textContent = "Download with LajuFile";
  styleButton(button);

  const menu = document.createElement("div");
  menu.style.cssText =
    "display:none;background:#fff;color:#111;border:1px solid #ccc;border-radius:4px;margin-top:4px;max-height:240px;overflow:auto;";

  button.addEventListener("click", (event) => {
    event.stopPropagation();
    if (menu.style.display === "block") {
      menu.style.display = "none";
      return;
    }
    menu.textContent = "";
    showFormats(menu, location.href);
  });

  wrap.append(button, menu);
  host.append(wrap);
}

// YouTube swaps the player on in-page navigation, so re-mount on a slow timer.
setInterval(mount, 1000);
mount();
