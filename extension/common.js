// Shared bridge, config, and rule-parsing helpers used by background, popup, and options.
(function initLajuFileCommon(globalObj) {
  const DEFAULTS = {
    port: 53841,
    token: "",
    autoCapture: true,
    excludedDomains: "",
    fileTypes: "",
  };

  function parseTokens(raw) {
    return String(raw || "")
      .split(/[\s,;]+/)
      .map((token) => token.trim().toLowerCase())
      .filter(Boolean);
  }

  function isHostExcluded(host, rawList) {
    if (!host) return false;
    const normalizedHost = host.toLowerCase();
    return parseTokens(rawList).some((token) => {
      const domain = token.replace(/^\*\.|^\./, "");
      return domain && (normalizedHost === domain || normalizedHost.endsWith(`.${domain}`));
    });
  }

  function parsePort(value) {
    const port = Number(value);
    if (!Number.isInteger(port) || port < 1024 || port > 65535) {
      throw new Error("Port must be between 1024 and 65535.");
    }
    return port;
  }

  function isValidToken(token) {
    return /^[0-9a-fA-F]{64}$/.test(String(token || ""));
  }

  function parseQuickPair(raw) {
    let data;
    try {
      data = JSON.parse(String(raw || "").trim());
    } catch {
      throw new Error("Quick-Pair expects JSON from LajuFile Setup Wizard.");
    }
    const port = parsePort(data && data.port);
    const token = String((data && data.token) || "").trim();
    if (!isValidToken(token)) {
      throw new Error("Quick-Pair token must be 64 hexadecimal characters.");
    }
    return { port, token };
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

  const SUPPORTED_VIDEO_HOSTS = [
    "youtube.com",
    "youtu.be",
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
  ];

  function isSupportedVideoPage(rawUrl) {
    if (typeof rawUrl !== "string" || !rawUrl.trim()) return false;
    try {
      const parsed = new URL(rawUrl);
      if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return false;
      const host = parsed.hostname.toLowerCase();
      if (host === "youtu.be") return parsed.pathname.length > 1;
      if (host === "youtube.com" || host === "www.youtube.com" || host === "m.youtube.com") {
        return (
          (parsed.pathname === "/watch" && parsed.searchParams.has("v")) ||
          parsed.pathname.startsWith("/shorts/")
        );
      }
      if (parsed.pathname.length <= 1) return false;
      return SUPPORTED_VIDEO_HOSTS.some(
        (domain) => host === domain || host.endsWith(`.${domain}`),
      );
    } catch {
      return false;
    }
  }

  globalObj.LajuFileCommon = {
    DEFAULTS,
    SUPPORTED_VIDEO_HOSTS,
    parseTokens,
    isHostExcluded,
    isSupportedVideoPage,
    parsePort,
    isValidToken,
    parseQuickPair,
    probeBridge,
  };
})(globalThis);
