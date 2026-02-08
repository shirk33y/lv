.PHONY: ci clean dev-linux dev-windows \
	build-linux-intel build-linux-arm build-windows-intel \
	docker-build-linux-intel docker-build-linux-arm docker-build-windows-intel

LV_VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')-$(shell git rev-parse --short HEAD 2>/dev/null || echo unknown)

# ── Checks ────────────────────────────────────────────────────────────
ci:
	@printf '%s\0' 'cargo test' 'cargo clippy -- -D warnings' 'cargo fmt -- --check' | bash scripts/multi.sh

# ── Dev ───────────────────────────────────────────────────────────────
# Separate target dirs so dev-linux and dev-windows never clash.
# dev-linux  → target-linux/  (default would be target/)
# dev-windows → uses Windows-native cargo.exe which writes to its own target/

dev-linux:
	CARGO_TARGET_DIR=target-linux-intel cargo run -- $(ARGS)

WIN_TARGET_PARENT := /mnt/c/Users/$(USER)/AppData/Local/lv-dev

dev-windows:
	@if grep -qi microsoft /proc/version 2>/dev/null; then \
		echo ":: building via Windows cargo.exe …"; \
		mkdir -p $(WIN_TARGET_PARENT); \
		WIN_TD=$$(wslpath -w "$(WIN_TARGET_PARENT)/target-windows-intel"); \
		cargo.exe build --target-dir "$$WIN_TD"; \
		echo ":: copying DLLs …"; \
		cp -u pkg/win64/SDL2.dll pkg/win64/libmpv-2.dll \
			$(WIN_TARGET_PARENT)/target-windows-intel/debug/; \
		echo ":: launching …"; \
		$(WIN_TARGET_PARENT)/target-windows-intel/debug/lv-imgui.exe $(ARGS); \
	else \
		echo ":: building + running for Windows (native) …"; \
		cargo run --target-dir target-windows-intel -- $(ARGS); \
	fi

# ── Native builds ─────────────────────────────────────────────────────
build-linux-intel:
	cargo build --release --target x86_64-unknown-linux-gnu
	strip target/x86_64-unknown-linux-gnu/release/lv-imgui

build-linux-arm:
	PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-linux-gnu \
	PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig \
	cargo build --release --target aarch64-unknown-linux-gnu
	aarch64-linux-gnu-strip target/aarch64-unknown-linux-gnu/release/lv-imgui

build-windows-intel:
	cargo xwin build --release --target x86_64-pc-windows-msvc
	@mkdir -p build-installer
	@cp target/x86_64-pc-windows-msvc/release/lv-imgui.exe build-installer/
	@cp pkg/win64/SDL2.dll pkg/win64/libmpv-2.dll pkg/installer.nsi build-installer/
	@cd build-installer && makensis -DLV_VERSION="$(LV_VERSION)" installer.nsi
	@echo "==> build-installer/lv-setup-$(LV_VERSION).exe"

# ── Docker builds → ./dist/ ──────────────────────────────────────────
docker-build-linux-intel:
	docker build -f docker/Dockerfile.linux-x86_64 -o dist .

docker-build-linux-arm:
	docker build -f docker/Dockerfile.linux-aarch64 -o dist .

docker-build-windows-intel:
	docker build -f docker/Dockerfile.windows-x86_64 -o dist .

# ── Clean ─────────────────────────────────────────────────────────────
clean:
	rm -rf dist/ build-installer/ target-linux-intel/ target-linux-arm/ target-windows-intel/
	rm -rf $(WIN_TARGET_PARENT)/target-windows-intel/ 2>/dev/null || true
