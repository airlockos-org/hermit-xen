{ pkgs, inputs, toolchain }:
let
  platform = pkgs.makeRustPlatform { cargo = toolchain; rustc = toolchain; };
  # A single vendor directory also supplies cargo -Zbuild-std and hermit-builtins.
  deps = platform.fetchCargoVendor {
    name = "hermit-xen-deps";
    src = pkgs.runCommand "hermit-dependency-lock" {} ''
      mkdir -p $out
      cp ${./hermit-deps.lock} $out/Cargo.lock
    '';
    hash = "sha256-a+TFGXWniYXjmisqGfgwgUERFDgHAvCPoApVBZOMkRU=";
  };
  # The parent Git tree pins these two submodules. No fetched fallback or patches.
  sources = pkgs.lib.fileset.toSource {
    root = ../vendor;
    fileset = pkgs.lib.fileset.unions [ ../vendor/hermit-kernel ../vendor/rust-vmm-xen ];
  };
  setup = ''
    export CARGO_HOME=$TMPDIR/cargo-home
    mkdir -p "$CARGO_HOME"
    cp ${deps}/.cargo/config.toml "$CARGO_HOME/config.toml"
    substituteInPlace "$CARGO_HOME/config.toml" --replace-fail '@vendor@' '${deps}'
    export CARGO_NET_OFFLINE=true
    export CARGO_BUILD_JOBS=$NIX_BUILD_CORES
    export CARGO_TARGET_DIR=$TMPDIR/target
  '';
  ringCheck = pkgs.stdenv.mkDerivation {
    name = "xen-ring-tests";
    src = sources;
    nativeBuildInputs = [ toolchain ];
    dontConfigure = true;
    buildPhase = ''
      ${setup}
      cd rust-vmm-xen
      # Test the guest crates only; dom0 ioctl/store tools are outside this port.
      cat > Cargo.toml <<'TOML'
      [workspace]
      resolver = "2"
      members = ["xen", "xen-sys", "xen-bindings"]
      TOML
      rm -f Cargo.lock
      cargo generate-lockfile --offline
      cargo test --locked -p xen --lib
    '';
    installPhase = "touch $out";
  };
  kernel = pkgs.stdenv.mkDerivation {
    pname = "hermit-xen-kernel";
    version = "0.13.0-pvh";
    src = sources;
    nativeBuildInputs = [ toolchain ];
    dontConfigure = true;
    dontFixup = true;
    buildPhase = ''
      ${setup}
      cd hermit-kernel
      cat >> .cargo/config.toml <<'TOML'
      [profile.release]
      debug = 2
      TOML
      export CARGO_ENCODED_RUSTFLAGS='-Cforce-frame-pointers=yes'
      cargo run --locked --package xtask --no-default-features -- \
        build --arch x86_64 --profile release --target-dir "$CARGO_TARGET_DIR" \
        --no-default-features --features xen,fsgsbase,kernel-stack
    '';
    installPhase = ''
      mkdir -p $out/lib
      cp "$CARGO_TARGET_DIR/x86_64/release/libhermit.a" $out/lib/
    '';
  };
  provenance = pkgs.writeText "hermit-provenance.json" (builtins.toJSON {
        # Content-addressed source path includes both checked-out dependency trees.
        sources = toString sources;
        kernel-base = "ac927733a70871c4b9e33d1ad4f72823836793e6";
        xen-base = "a8529dc1e50445e5579a9aa0abd763348460bdcd";
        hermit-rs = inputs.hermit-rs.rev;
        rust = "nightly-2026-08-01";
        kernel-features = [ "xen" "pvh" "fsgsbase" "kernel-stack" ];
      });
  buildApplication = import ./application.nix { inherit pkgs toolchain kernel deps setup provenance; };
in { inherit kernel sources deps setup provenance buildApplication ringCheck; }
