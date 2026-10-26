{ pkgs, hermit, toolchain }:
let
  transport = [ ../crates/xen-transport/Cargo.toml ../crates/xen-transport/src ];
  fixtureSource = name: extra: pkgs.lib.fileset.toSource {
    root = ../.;
    fileset = pkgs.lib.fileset.unions ([
      (../tests/guests + "/${name}/Cargo.toml")
      (../tests/guests + "/${name}/Cargo.lock")
      (../tests/guests + "/${name}/src")
    ] ++ extra);
  };
  smoke = hermit.buildApplication {
    src = fixtureSource "hermit-smoke" [];
    manifestPath = "tests/guests/hermit-smoke/Cargo.toml";
    bin = "hermit-xen-smoke";
  };
  ipc = hermit.buildApplication {
    src = fixtureSource "hermit-ipc" (transport ++ [ ../tests/guests/schema.rs ]);
    manifestPath = "tests/guests/hermit-ipc/Cargo.toml";
    bin = "hermit-xen-ipc";
  };
  peer = pkgs.stdenv.mkDerivation {
    name = "xen-ipc-peer";
    src = fixtureSource "linux-peer" (transport ++ [ ../tests/guests/schema.rs ]);
    nativeBuildInputs = [ toolchain ];
    dontConfigure = true;
    buildPhase = ''
      ${hermit.setup}
      export RUSTFLAGS='-Ctarget-feature=+crt-static -Clinker=rust-lld'
      cargo build --locked --manifest-path tests/guests/linux-peer/Cargo.toml --target x86_64-unknown-linux-musl --release
    '';
    installPhase = ''
      mkdir -p $out/bin
      cp "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/xen-ipc-peer" $out/bin/
    '';
  };
in { inherit smoke ipc peer; }
