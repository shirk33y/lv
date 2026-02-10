.PHONY: ci clean dev-linux dev-windows \
        build-linux-intel build-linux-arm build-windows-intel \
        docker-build docker-smoke smoke-test

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

# ── Docker builds + smoke tests → ./dist/ ────────────────────────────
docker-build:
	bash scripts/smoke-test-docker.sh --build-only $(TARGET)

docker-smoke:
	bash scripts/smoke-test-docker.sh $(TARGET)

# ── Test ──────────────────────────────────────────────────────────────
smoke-test:
	bash scripts/smoke-test.sh $(ARGS)

# ── Clean ─────────────────────────────────────────────────────────────
clean:
	bash scripts/clean.sh
