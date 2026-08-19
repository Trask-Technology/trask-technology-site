# trask_graph — build the hero graph wasm bundles.
#
# Run from this directory (web/rust). Output lands in web/web/pkg/, which is
# where "Trask Technology Site.dc.html" imports ./pkg/ from.

PKG   := ../web/pkg
SERVE := ../web
PORT  ?= 8000

# RUSTFLAGS applies to every target, wasm32 included. A host tuning flag such as
# -C target-cpu=native leaks into the wasm build and makes wasm-bindgen fail with
# "failed to find intrinsics to enable clone_ref", so it is stripped here. Scope
# host tuning to [target.x86_64-unknown-linux-gnu] in ~/.cargo/config.toml so it
# never reaches a wasm build in the first place.
CARGO    := env -u RUSTFLAGS cargo
WASMPACK := env -u RUSTFLAGS wasm-pack

.PHONY: all wasm wasm-lite check clean serve tools help

all: wasm

## wasm       Full bundle: Graph + Stage, the wgpu/WebGPU renderer (~200 KB).
wasm:
	$(WASMPACK) build --target web --release --out-dir $(PKG)

## wasm-lite  Generation only: Graph alone, no wgpu, no Stage (~27 KB).
wasm-lite:
	$(WASMPACK) build --target web --release --no-default-features --out-dir $(PKG)

## check      Type-check against wasm32 without emitting a bundle.
check:
	$(CARGO) check --target wasm32-unknown-unknown

## clean      Drop the build cache and the emitted bundle.
clean:
	$(CARGO) clean
	rm -rf $(PKG)

## serve      Serve the page. WebGPU needs localhost or 127.0.0.1, not a LAN IP.
serve:
	@echo "open http://localhost:$(PORT)/Trask%20Technology%20Site.dc.html"
	cd $(SERVE) && python3 -m http.server $(PORT)

## tools      Install the one-time prerequisites.
tools:
	cargo install wasm-pack
	rustup target add wasm32-unknown-unknown

## help       List these targets.
help:
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/^## /  make /'
