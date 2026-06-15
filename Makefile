.PHONY: ci clean dev dev-linux dev-windows \
        configure check test \
        build-linux-intel build-linux-arm build-windows-intel \
        docker-build docker-smoke smoke-test \
        flatpak-build flatpak-smoke flatpak-release

CONFIG_MK ?= config.local.mk
-include $(CONFIG_MK)
export LV_NATIVE_PREFIXES

FLATPAK_ARCH ?= x86_64
FLATPAK_PLATFORM_x86_64 := linux/amd64
FLATPAK_PLATFORM_aarch64 := linux/arm64
FLATPAK_PLATFORM ?= $(FLATPAK_PLATFORM_$(FLATPAK_ARCH))
FLATPAK_BUNDLE ?= build/lv-$(FLATPAK_ARCH).flatpak
FLATPAK_STATE_DIR ?= build/flatpak-state
FLATPAK_FORCE_CLEAN ?= 1
LV_SMOKE_LOG_DIR ?= build/flatpak-smoke-logs/$(FLATPAK_ARCH)
CONTAINER_RUNTIME ?= podman
export CONTAINER_RUNTIME
export FLATPAK_BUNDLE
export FLATPAK_STATE_DIR
export FLATPAK_FORCE_CLEAN

# ── Checks ────────────────────────────────────────────────────────────
ci:
	bash scripts/ci.sh

configure:
	bash scripts/configure.sh

check:
	cargo check

test:
	cargo test

# ── Dev ───────────────────────────────────────────────────────────────
dev:
	cargo run -- $(ARGS)

dev-linux:
	bash scripts/dev-linux.sh $(ARGS)

dev-windows:
	bash scripts/dev-windows.sh $(ARGS)

# ── Native builds ─────────────────────────────────────────────────────
build-linux-intel:
	bash scripts/build-linux-intel.sh

build-linux-arm:
	bash scripts/build-linux-arm.sh

build-windows-intel:
	bash scripts/build-windows-intel.sh

# ── Docker builds + smoke tests → ./dist/ ────────────────────────────
docker-build:
	bash scripts/smoke-test-docker.sh --build-only $(TARGET)

docker-smoke:
	bash scripts/smoke-test-docker.sh $(TARGET)

# ── Test ──────────────────────────────────────────────────────────────
smoke-test:
	bash scripts/smoke-test.sh $(ARGS)

# ── Flatpak ───────────────────────────────────────────────────────────
flatpak-build:
	CONTAINER_PLATFORM=$(FLATPAK_PLATFORM) bash scripts/build-flatpak.sh --build-only

flatpak-smoke:
	mkdir -p $(LV_SMOKE_LOG_DIR)
	$(CONTAINER_RUNTIME) build \
		--platform $(FLATPAK_PLATFORM) \
		-f docker/Dockerfile.flatpak-smoke \
		-t lv-flatpak-smoke-$(FLATPAK_ARCH) .
	$(CONTAINER_RUNTIME) run --rm --privileged \
		--platform $(FLATPAK_PLATFORM) \
		-v $(CURDIR)/$(FLATPAK_BUNDLE):/tmp/lv.flatpak:ro \
		-v $(CURDIR)/$(LV_SMOKE_LOG_DIR):/smoke-logs:rw \
		lv-flatpak-smoke-$(FLATPAK_ARCH) \
		bash -lc 'FLATPAK_BUNDLE=/tmp/lv.flatpak LV_FIXTURES=/fixtures LV_SMOKE_LOG_DIR=/smoke-logs bash /scripts/smoke-test-flatpak.sh'

flatpak-release: flatpak-build flatpak-smoke

# ── Clean ─────────────────────────────────────────────────────────────
clean:
	bash scripts/clean.sh
