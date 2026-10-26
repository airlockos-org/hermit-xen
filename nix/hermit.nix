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
  sources = pkgs.runCommand "hermit-xen-sources" { nativeBuildInputs = [ pkgs.patch ]; } ''
    mkdir -p $out
    cp -r ${inputs.hermit-kernel} $out/kernel
    cp -r ${inputs.xen-rust} $out/xen
    chmod -R u+w $out
    patch -d $out/kernel -p1 < ${../patches/hermit/01-xen-boot.patch}
    patch -d $out/xen -p1 < ${../patches/xen/01-guest-helpers.patch}
    patch -d $out/kernel -p1 < ${../patches/hermit/02-xen-ipc.patch}
    patch -d $out/xen -p1 < ${../patches/xen/02-events-time-grants.patch}
  '';
  wrapper = pkgs.applyPatches {
    name = "hermit-rs-prebuilt";
    src = inputs.hermit-rs;
    patches = [ ../patches/hermit/03-wrapper.patch ];
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
      cd xen
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
      cd kernel
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
        kernel = inputs.hermit-kernel.rev;
        hermit-rs = inputs.hermit-rs.rev;
        xen-rust = inputs.xen-rust.rev;
        rust = "nightly-2026-08-01";
        kernel-features = [ "xen" "pvh" "fsgsbase" "kernel-stack" ];
        kernel-patch = builtins.hashFile "sha256" ../patches/hermit/01-xen-boot.patch;
        xen-patch = builtins.hashFile "sha256" ../patches/xen/01-guest-helpers.patch;
        kernel-ipc-patch = builtins.hashFile "sha256" ../patches/hermit/02-xen-ipc.patch;
        xen-ipc-patch = builtins.hashFile "sha256" ../patches/xen/02-events-time-grants.patch;
        wrapper-patch = builtins.hashFile "sha256" ../patches/hermit/03-wrapper.patch;
      });
  buildApplication = import ./application.nix { inherit pkgs toolchain kernel wrapper deps setup provenance; };
in { inherit kernel sources wrapper deps setup provenance buildApplication ringCheck; }
