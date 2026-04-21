// BoraCall — desktop bridge
// Runs before the JSX is compiled so `window.desktop` exists everywhere.
// When launched inside Tauri, exposes native commands via __TAURI_INTERNALS__.
// When opened in a plain browser (prototype mode), falls back to web equivalents
// so the UI keeps working for visual iteration without the native shell.
(function () {
  const internals =
    (typeof window !== "undefined" && window.__TAURI_INTERNALS__) || null;
  const isNative = !!internals;

  const invoke = isNative
    ? (cmd, args) => internals.invoke(cmd, args)
    : () => Promise.reject(new Error("bridge: not running inside Tauri"));

  // --- Clipboard ----------------------------------------------------------
  // In Tauri, hit the plugin-clipboard-manager command directly; in the browser
  // fall back to navigator.clipboard (same promise shape).
  const clipboard = {
    async writeText(text) {
      if (isNative) {
        return invoke("plugin:clipboard-manager|write_text", { label: null, text: String(text) });
      }
      if (navigator.clipboard && navigator.clipboard.writeText) {
        return navigator.clipboard.writeText(String(text));
      }
      throw new Error("clipboard: unsupported environment");
    },
    async readText() {
      if (isNative) {
        return invoke("plugin:clipboard-manager|read_text");
      }
      if (navigator.clipboard && navigator.clipboard.readText) {
        return navigator.clipboard.readText();
      }
      throw new Error("clipboard: unsupported environment");
    },
  };

  // --- Window controls ----------------------------------------------------
  const win = {
    minimize: () => (isNative ? invoke("window_minimize") : Promise.resolve()),
    toggleMaximize: () =>
      isNative ? invoke("window_toggle_maximize") : Promise.resolve(),
    setInvisibleMode: (enabled) =>
      isNative
        ? invoke("set_invisible_mode", { enabled: !!enabled })
        : Promise.resolve(),
  };

  // --- Opener (open external URLs in system browser) ----------------------
  const opener = {
    openUrl: (url) => {
      if (isNative) {
        return invoke("plugin:opener|open_url", { url, with: null });
      }
      try {
        window.open(url, "_blank", "noopener");
        return Promise.resolve();
      } catch (e) {
        return Promise.reject(e);
      }
    },
  };

  // --- Auto-updater (Tauri only) ------------------------------------------
  // checkForUpdate → { available, version, current_version, notes }
  // installUpdate → baixa e aplica; o app reinicia sozinho após sucesso.
  const updater = {
    check: () =>
      isNative
        ? invoke("check_for_update")
        : Promise.resolve({ available: false, current_version: null }),
    install: () =>
      isNative
        ? invoke("install_update")
        : Promise.reject(new Error("updater: browser mode")),
  };

  // --- Build the public bridge object -------------------------------------
  const bridge = {
    isNative,
    platform: null,
    arch: null,
    appVersion: null,
    version: "0.1.0",
    invoke,
    clipboard,
    window: win,
    opener,
    updater,
  };

  if (isNative) {
    invoke("platform_info")
      .then((info) => {
        bridge.platform = info.os;
        bridge.arch = info.arch;
        bridge.appVersion = info.app_version;
        document.documentElement.setAttribute("data-native", "tauri");
        document.documentElement.setAttribute("data-os", info.os);
      })
      .catch((err) => console.warn("bridge: platform_info failed", err));
  } else {
    document.documentElement.setAttribute("data-native", "browser");
  }

  window.desktop = bridge;
})();
