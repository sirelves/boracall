// BoraCall — runtime env bag. Overridden at deploy time (CI can rewrite this
// file before packaging) to point the desktop at a production backend.
//
// Leave these as-is for local dev (Colima + docker-compose + cargo run -p boracall-server).
(function () {
  window.BC_API_URL    = window.BC_API_URL    || "http://127.0.0.1:3030";
  window.BC_PUBLIC_URL = window.BC_PUBLIC_URL || "https://boracall.app";
})();
