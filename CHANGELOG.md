# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2025-12-16

### Added

- `PixstageRgba`: `RGBA8` pixel buffer with incremental texture uploads.
- `PixstageIndexed`: `Indexed8 + Palette` rendering, with GPU palette lookup (palette cycling friendly).
- `PixstageRgb565`: `RGB565` input with incremental uploads (converted to RGBA8 only for dirty regions).
- `PixstageArgb1555`: `ARGB1555` input with incremental uploads (1-bit alpha).
- Dirty-rect tracking utilities (`Rect` and internal tiling).
- `ScalingMode` and `PixstageOptions` for shared configuration.
- `SurfaceTexture` wrapper to validate surface size and carry a window handle.

### Changed

- Upgraded `wgpu` to `27.0.1` and updated internal API usage accordingly.
- Moved `winit` out of library dependencies; it is now only used by examples (`dev-dependency`).
- Switched error handling to a unified `pixstage::Error` / `pixstage::Result` (replacing `anyhow`).
- Migrated examples to the `winit 0.30` app lifecycle (`ApplicationHandler` + `EventLoop::run_app`).
- Updated WASM configuration to use `wgpu` WebGL backend (`default-features = false, features = ["webgl"]`).

### Removed

- Removed the legacy monolithic `State` API.
- Removed legacy shader and texture helpers (`src/shader.wgsl`, `src/texture.rs`).
