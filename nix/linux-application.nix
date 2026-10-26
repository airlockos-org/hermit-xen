# Static Linux counterparts for external services in the disposable Xen laboratory.
{ pkgs, toolchain, setup, deps }:
{ src, bin, manifestPath ? "Cargo.toml", features ? [], cargoVendorDir ? deps }:
pkgs.stdenv.mkDerivation {
  pname = bin;
  version = "0.1.0";
  inherit src;
  nativeBuildInputs = [ toolchain ];
  dontConfigure = true;
  buildPhase = ''
    ${setup}
    cp ${cargoVendorDir}/.cargo/config.toml "$CARGO_HOME/config.toml"
    substituteInPlace "$CARGO_HOME/config.toml" --replace-fail '@vendor@' '${cargoVendorDir}'
    export RUSTFLAGS='-Ctarget-feature=+crt-static -Clinker=rust-lld'
    cargo build --locked --offline --manifest-path ${pkgs.lib.escapeShellArg manifestPath} \
      --bin ${pkgs.lib.escapeShellArg bin} ${pkgs.lib.optionalString (features != []) ("--features " + pkgs.lib.escapeShellArg (pkgs.lib.concatStringsSep "," features))} \
      --target x86_64-unknown-linux-musl --release
  '';
  installPhase = ''
    mkdir -p $out/bin
    cp "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/"${pkgs.lib.escapeShellArg bin} $out/bin/
  '';
}
