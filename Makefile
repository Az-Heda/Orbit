# Orbit build targets. `make build` produces a plain executable, NOT an installer
# (Tauri is invoked with --no-bundle).
#
#   make windows   -> bin/orbit.exe
#   make linux     -> bin/orbit   (Linux hosts only)
#   make clean     -> cargo clean and removes bin/
#
# On Linux/WSL, `windows` cross-compiles with cargo-xwin (needs clang, lld, llvm,
# the msvc rust target, and cargo-xwin). On a Windows host it builds natively.

WIN_TARGET := x86_64-pc-windows-msvc
UNAME_S    := $(shell uname -s 2>/dev/null || echo Windows)

.PHONY: help deps windows linux clean

help:
	@echo "make windows  - build orbit.exe (no installer)"
	@echo "make linux    - build Linux executable (Linux host only)"
	@echo "make clean    - cargo clean + remove bin/"

deps:
	pnpm install --frozen-lockfile

ifeq ($(UNAME_S),Linux)
windows: deps
	@command -v cargo-xwin >/dev/null || { echo "cargo-xwin missing: cargo install --locked cargo-xwin"; exit 1; }
	@command -v clang >/dev/null || { echo "clang/lld/llvm required for cross-compiling"; exit 1; }
	@rustup target list --installed | grep -qx $(WIN_TARGET) || rustup target add $(WIN_TARGET)
	pnpm tauri build --runner cargo-xwin --target $(WIN_TARGET) --no-bundle
	@mkdir -p bin && cp src-tauri/target/$(WIN_TARGET)/release/orbit.exe bin/orbit.exe
	@echo "Built: bin/orbit.exe"

linux: deps
	pnpm tauri build --no-bundle
	@mkdir -p bin && cp src-tauri/target/release/orbit bin/orbit
	@echo "Built: bin/orbit"
else
windows: deps
	pnpm tauri build --target $(WIN_TARGET) --no-bundle
	@mkdir -p bin && cp src-tauri/target/$(WIN_TARGET)/release/orbit.exe bin/orbit.exe
	@echo "Built: bin/orbit.exe"

linux:
	@echo "The linux target is only supported on a Linux host (needs webkit2gtk-4.1)."; exit 1
endif

clean:
	cd src-tauri && cargo clean
	rm -rf bin
