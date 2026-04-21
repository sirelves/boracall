// BoraCall — runtime env bag. Overridden at deploy time (CI can rewrite this
// file before packaging) to point the desktop at a production backend.
//
// Production build: points at the VPS backend (<REDACTED_IP> / boracall.com).
// For local dev, set window.BC_API_URL in devtools console to
// "http://127.0.0.1:3030" before the page boots, or rebuild with that value.
(function () {
  window.BC_API_URL    = window.BC_API_URL    || "http://<REDACTED_IP>";
  window.BC_PUBLIC_URL = window.BC_PUBLIC_URL || "https://boracall.com";
})();
