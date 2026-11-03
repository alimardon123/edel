# A test authority

`ca.pem` is an authority made only for `edel`'s tests (M8.40), and `server.pem` and `server.key` a certificate it signed for `localhost` and `127.0.0.1`, both valid for a hundred years from 2026-11-03. `trust.rs`'s tests serve HTTPS with them and check that a download is trusted with `ca.pem` in the machine's list and refused without it. The key is public on purpose and trusted nowhere else; the authority's own key was thrown away.
