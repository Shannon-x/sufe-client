# xboard-client

A small, fast, multi-platform client for Xboard panels. Powered by **mihomo (Clash.Meta)**, with a hot-swappable kernel and a placeholder for additional kernels (e.g. Xray) in the future.

## Targets (Phase 1)

| Platform | Status | Bundle goal |
|----------|--------|-------------|
| Windows  | planned | ≤ 15 MB (excl. kernel) |
| macOS    | planned | ≤ 15 MB Universal      |
| Linux    | planned | ≤ 15 MB AppImage       |
| Android  | planned | ≤ 30 MB APK (incl. kernel) |
| iOS      | future  | —                       |

## Layout

```
core/          Rust library — single source of truth (API, kernel, updater, profile)
desktop/       Tauri 2.x app (Vue 3 + Naive UI)
android/       Native Android app (Compose + Material 3)
ci/            GitHub Actions workflows + signing scripts
update-server/ Static metadata templates for self-hosted update channels
docs/          Architecture / VPN / kernel-update notes
kernels/       Local cache of mirrored mihomo binaries (gitignored)
```

## Quickstart

```bash
just bootstrap            # install Rust + Tauri CLI + Android targets
just core-test            # run core unit + integration tests
just desktop-install      # install JS deps (npm) for the Tauri shell
just desktop-check        # cargo check + clippy + vue-tsc on the desktop app
just desktop-dev          # launch desktop app in dev mode
just android-debug        # build android debug APK
```

### Desktop dev mode

```bash
cd desktop
npm install --no-audit --fund=false --cache=$PWD/.npm-cache
npm run tauri -- dev
```

The first launch compiles all Tauri/plugin dependencies — expect a few
minutes. Subsequent runs are sub-second (incremental). The app opens
straight on the login page; type any reachable Xboard backend URL +
credentials and hit "登录".

See `docs/architecture.md` for the full design (also mirrored in [`../streamed-moseying-firefly.md`](../.claude/plans/streamed-moseying-firefly.md)).

## Backend

Targets the API surface documented in [`../Xboard-API.md`](../Xboard-API.md) (cedar2025/Xboard derivative, Laravel 10 + Sanctum).
