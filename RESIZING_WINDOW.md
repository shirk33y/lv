# Resizing Borderless Window

## Problem

Window is borderless (`.borderless()`) with a custom hit-test for both dragging (top bar) and resizing (6px edges/corners). Resize works functionally (drag edges to resize) but cursor does not change to resize cursors on hover.

Cursor only changes to resize cursors when drag-resize starts (click-drag on edge) — that's SDL2's internal hit-test system/the compositor taking over. Hover preview is missing.

## Root Cause

`cursor-shape-v1` Wayland protocol limitation: `wp_cursor_shape_device_v1_set_shape()` only accepts `wl_pointer.enter` serials, not motion serials. SDL2 2.32.10 uses this protocol when available (KDE 6 supports it). So `SDL_SetCursor` only takes effect on pointer enter events, not on hover motion.

Even XWayland eventually routes through the same `cursor-shape-v1` protocol at the compositor level, so switching to `SDL_VIDEO_DRIVER=x11` doesn't help.

## What We Tried

### 1. `SDL_SetCursor` in MouseMotion event handler

Set cursor via `Cursor::from_system(SystemCursor::SizeNS).set()` on every `MouseMotion` event. Result: cursor changes for 1 frame then reverts. Reappears on next motion event.

### 2. `SDL_SetCursor` at end of each frame

Same logic but called after `gl_swap_window()` per frame using `imgui_ctx.io().mouse_pos`. Result: worse — no cursor change at all (likely io().mouse_pos stale or borrow conflict with render).

### 3. Only set resize cursors (skip Arrow)

Only call `set()` when on an edge (skip Arrow for normal area). Result: no cursor change at all.

### 4. Raw `SDL_CreateSystemCursor` + `SDL_SetCursor` (bypass Rust wrapper)

Avoid sdl2 crate's `Cursor` wrapper, call SDL2 sys functions directly. Result: same as #1 (1-frame flicker).

### 5. Env vars for cursor theme

Set `XCURSOR_SIZE=24 XCURSOR_THEME=breeze_cursors` to ensure cursor theme loads correctly. Result: no change.

### 6. Force XWayland

`SDL_VIDEO_DRIVER=x11` and `WAYLAND_DISPLAY=`. Result: no change — cursor path still goes through `cursor-shape-v1` on the compositor side.

## Ideas for Fix

### A. Use `wl_pointer.set_cursor` directly (bypass cursor-shape-v1)

- Get `wl_pointer` from SDL2's internal Wayland state (hacky — not exposed)
- On each motion, call `wl_pointer.set_cursor(motion_serial, surface, hot_x, hot_y)`
- Need to create cursor surface with correct bitmap from theme
- Requires `wayland-client` crate alongside SDL2, fragile interop

### B. Custom RC (Rounded Corners) cursor overlay via OpenGL

- Hide SDL cursor entirely (`SDL_ShowCursor(SDL_DISABLE)`)
- Draw custom cursor texture in imgui/OpenGL at mouse position
- Full control over appearance, no Wayland/SDL cursor issues
- Downside: cursor won't show in system screenshots, slight latency

### C. LD_PRELOAD shim (reverse wlcursorfix)

- Hook `wp_cursor_shape_device_v1_set_shape` and convert to `wl_pointer.set_cursor`
- Too complex and fragile for a simple cursor preview

### D. Add window decorations (remove `.borderless()`)

- Let WM draw standard title bar + resize handles with native cursors
- Lose custom header bar (close/min/max buttons)
- Simplest solution but changes UX significantly

### E. Accept limitation

- Resize works, cursor changes on drag, just no hover preview
- Many Wayland-native apps behave the same way
- Minimal code, no hacks needed

## Current Status

Using approach #E — cursor code kept in `set_resize_cursor()` function but effect is limited by `cursor-shape-v1` protocol. Resize functional, cursor changes on drag.
