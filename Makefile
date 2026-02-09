.PHONY: ci clean dev-linux dev-windows \
        build-linux-intel build-linux-arm build-windows-intel \
        docker-build smoke-test

# ── Checks ────────────────────────────────────────────────────────────
ci:
	bash scripts/ci.sh

# ── Dev ───────────────────────────────────────────────────────────────
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

# ── Docker builds → ./dist/ ──────────────────────────────────────────
docker-build:
	bash scripts/docker-build.sh $(TARGET)

# ── Test ──────────────────────────────────────────────────────────────
smoke-test:
	bash scripts/smoke-test.sh $(ARGS)

# ── Clean ─────────────────────────────────────────────────────────────
clean:
	bash scripts/clean.sh
