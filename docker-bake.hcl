// docker-bake.hcl — orchestrate parallel multi-target Docker builds
//
// Usage:
//   docker buildx bake                    # build all targets (out)
//   docker buildx bake test               # run tests only
//   docker buildx bake smoke              # build + smoke test all
//   docker buildx bake linux-out          # Linux artifacts only
//   docker buildx bake windows-out        # Windows artifacts only
//
// With GHA cache (CI):
//   docker buildx bake --set '*.cache-from=type=gha' --set '*.cache-to=type=gha,mode=max'

group "default" {
  targets = ["linux-out", "windows-out"]
}

group "test" {
  targets = ["linux-test"]
}

group "smoke" {
  targets = ["linux-smoke", "windows-smoke"]
}

// ── Linux x86_64 ────────────────────────────────────────────────────

target "linux-test" {
  dockerfile = "docker/Dockerfile.linux-x86_64"
  context    = "."
  target     = "test"
}

target "linux-smoke" {
  dockerfile = "docker/Dockerfile.linux-x86_64"
  context    = "."
  target     = "smoke"
}

target "linux-out" {
  dockerfile = "docker/Dockerfile.linux-x86_64"
  context    = "."
  target     = "out"
  output     = ["type=local,dest=dist"]
}

// ── Windows x86_64 ─────────────────────────────────────────────────

target "windows-smoke" {
  dockerfile = "docker/Dockerfile.windows-x86_64"
  context    = "."
  target     = "smoke"
}

target "windows-out" {
  dockerfile = "docker/Dockerfile.windows-x86_64"
  context    = "."
  target     = "out"
  output     = ["type=local,dest=dist"]
}
