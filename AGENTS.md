# AGENTS.md

## Cursor Cloud specific instructions

Taurium is a single **Tauri v2** desktop app (no backend/DB/network services): a Rust core
in `src-tauri/` plus a vanilla JS/Vite frontend in `src/`. The "services" it shows
(WhatsApp, Slack, GitHub…) are just external websites loaded in native webviews — nothing
to host locally.

### Toolchain notes
- Requires **Rust stable ≥ 1.85** (a transitive dep, `zbus_macros`, needs `edition2024`).
  The VM snapshot ships a recent stable via `rustup` (`rustup default stable`); Rust 1.83 is
  too old and fails to compile.
- Node LTS + npm and the Linux WebKitGTK dev libs (see `.github/workflows/ci.yml` for the
  exact `apt` list) are already installed in the snapshot.

### Build ordering gotcha
- `generate_context!` embeds `../dist`, so the frontend must be built **before** any Rust
  build/run: run `npm run build` before `cargo build`. `npm run tauri dev` handles this
  automatically via `beforeDevCommand` (it starts Vite first).

### Lint / test / build (commands mirror CI; run from `src-tauri/` for cargo)
- Format check: `cargo fmt --check`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Tests: `cargo test` (Rust is the only test suite; there is no JS lint/test configured)
- Compile check: `npm run build` then `cargo build --manifest-path src-tauri/Cargo.toml`

### Running the app (GUI)
- A headless X server is available at `DISPLAY=:1` (Xvfb). Launch with
  `DISPLAY=:1 npm run tauri dev`.
- `libEGL warning: DRI3 error ...` on startup is harmless (software rendering under Xvfb).
- On first launch the app writes its config to
  `~/.local/share/com.taurium.app/services.json`; delete it to reset to the default
  service list.
- External sites (WhatsApp, Gmail…) won't fully load/authenticate in the sandbox — that is
  expected. In-app core features (settings, adding services from the catalog, sidebar
  switching, config persistence) work fully offline and are the best things to exercise.
