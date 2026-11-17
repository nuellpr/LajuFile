# Privacy Policy — LajuFile & LajuFile Capture

**Last updated:** November 17, 2026  
**Repository:** https://github.com/nuellpr/LajuFile

---

## English

### Overview

**LajuFile Capture** is the companion browser extension for **LajuFile Desktop**. LajuFile is built with a local-first architecture: we do **not** operate analytics servers, tracking beacons, ad networks, or remote user databases, and we do **not** collect, sell, rent, or transfer your personal data or browsing history to the developer or any third party.

### What Information the Extension Accesses Locally

To hand off downloads from your browser to LajuFile Desktop running on your own computer, **LajuFile Capture** accesses the following browser data strictly **locally on your device**:

1. **Download URLs & Metadata (`downloads` permission)**  
   When you start a download, click an intercepted media/PDF link, or use the right-click **"Download with LajuFile"** menu item, the extension sends only that target download URL, referrer URL (if present), browser User-Agent string, and suggested filename to your local LajuFile Desktop bridge at `http://127.0.0.1:<port>`. If LajuFile Desktop accepts the download (`201` or `409`), the extension cancels and cleans up the duplicate browser download entry.

2. **Session Cookies for the Target Download (`cookies` permission)**  
   When you initiate a download or request video formats on a watch page, the extension temporarily reads cookies scoped strictly to that target URL (or `youtube.com` for YouTube video requests) and passes them over loopback (`http://127.0.0.1:<port>`) to your local LajuFile Desktop app so authenticated downloads succeed. Cookies are **never** sent to developer-controlled servers or unrelated third parties. Locally inside LajuFile Desktop, stored cookies are encrypted at rest (Windows DPAPI on Windows, key-sealed local storage on macOS and Linux).

3. **Web Page Interaction & Link Capture (`<all_urls>`, `http://127.0.0.1/*`)**  
   - **Content script (`interceptor.js`):** Listens locally in-page for `Alt+Click` bypasses (leaving downloads in the browser) and intercepts clicks on supported direct media/PDF links only when auto-capture is enabled and the site is not excluded. It does not upload browsing history or page contents.
   - **YouTube player integration (`content.js`):** Runs on YouTube pages solely to display the optional **"Download with LajuFile"** floating quality picker on `/watch` pages and forward your explicit quality choice to your local LajuFile Desktop bridge.
   - **Local loopback (`http://127.0.0.1/*`):** Used exclusively to communicate with your own LajuFile Desktop bridge protected by a 64-character hexadecimal access token (`x-lajufile-token`).

4. **Active Tab Hostname (`activeTab` permission)**  
   Accessed only when you open the extension popup, solely to read the current site's hostname so you can toggle **"Pause on site"**.

5. **Local Extension Preferences (`storage` permission)**  
   Stores only your local configuration (`port`, `token`, `autoCapture`, `excludedDomains`, `fileTypes`) locally inside browser storage (`chrome.storage.local` / `browser.storage.local`).

6. **Notifications & Context Menu (`notifications`, `contextMenus`)**  
   Used purely for local pairing/queue status notifications and adding **"Download with LajuFile"** to link context menus.

### Desktop Application Network Requests

LajuFile Desktop performs network connections **only** to fulfill your explicit actions or configured settings:
- Downloading files/web links directly from the origin hosts you request (or via an HTTP/HTTPS/SOCKS5 proxy you configure locally).
- Downloading open-source companion video tools (`yt-dlp`, `deno`, and `ffmpeg`) over HTTPS with SHA-256 verification when you trigger video downloads and those tools are not yet installed.
- Checking `https://github.com/nuellpr/LajuFile` Releases for application updates when triggered manually (**Help -> Check for updates**) or via the built-in startup update check.

### Data Sharing & Selling

We do **not** sell, rent, share, or disclose user data to third parties, data brokers, or advertisers under any circumstances.

### Children's Privacy

LajuFile and LajuFile Capture do not collect personal information from anyone, including children under 13.

### Changes & Contact

Updates to this policy will be published in this repository. For questions or privacy concerns, please open an issue at:  
https://github.com/nuellpr/LajuFile/issues

---

## Bahasa Indonesia (Ringkasan)

**LajuFile Capture** tidak mengumpulkan, menyimpan di server pengembang, menjual, ataupun membagikan data pribadi, riwayat penjelajahan, atau telemetri ke pihak mana pun.

Seluruh komunikasi antara ekstensi browser dan aplikasi **LajuFile Desktop** berjalan **secara lokal saja** melalui loopback `http://127.0.0.1:<port>` yang dilindungi token akses heksadesimal 64 karakter:
- **URL unduhan, referrer, nama file, & cookie sesi URL tersebut** hanya dikirim lokal ke aplikasi LajuFile Desktop di komputer Anda sendiri agar unduhan terautentikasi berhasil.
- **Data pengaturan ekstensi** (port, token, daftar domain dikecualikan, filter ekstensi file) disimpan lokal di penyimpanan browser.
- **`activeTab`** hanya dibaca saat Anda membuka popup ekstensi untuk menampilkan tombol **Pause on site** pada situs aktif.

Pertanyaan atau laporan terkait privasi dapat dikirim melalui:  
https://github.com/nuellpr/LajuFile/issues
