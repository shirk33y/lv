.PHONY: ci dist dist-linux dist-arm64 dist-win clean

# ── Checks (parallel via multi.sh) ────────────────────────────────────
ci:
	@printf '%s\0' 'cargo test' 'cargo clippy -- -D warnings' 'cargo fmt -- --check' | bash scripts/multi.sh

# ── Multi-stage Docker builds → ./dist/ ───────────────────────────────
# Each target is a self-contained multi-stage Dockerfile.
# `docker build -o dist` copies artifacts directly from the final scratch stage.

dist: dist-linux dist-arm64 dist-win

dist-linux:
	docker build -f docker/Dockerfile.linux-x86_64 -o dist .

dist-arm64:
	docker build -f docker/Dockerfile.linux-aarch64 -o dist .

dist-win:
	docker build -f docker/Dockerfile.windows-x86_64 -o dist .

# ── Clean ─────────────────────────────────────────────────────────────
clean:
	rm -rf dist/ build-installer/
