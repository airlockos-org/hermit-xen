# Public builder. The dependency vendor must contain the application's locked
# dependencies and the pinned build-std dependencies (see docs/building.md).
{ pkgs, toolchain, kernel, deps, setup, provenance }:
{ src, bin, manifestPath ? "Cargo.toml", cargoVendorDir ? deps, pname ? bin, features ? [] }:
pkgs.stdenv.mkDerivation {
  inherit pname src;
  version = "0.1.0";
  nativeBuildInputs = [ toolchain pkgs.binutils ];
  dontConfigure = true;
  dontFixup = true; # Keep Xen notes, OSABI, symbols and frame pointers.
  buildPhase = ''
    ${setup}
    cp ${cargoVendorDir}/.cargo/config.toml "$CARGO_HOME/config.toml"
    substituteInPlace "$CARGO_HOME/config.toml" --replace-fail '@vendor@' '${cargoVendorDir}'
    # Cargo's links override skips upstream hermit-rs's kernel-building script.
    # The unmodified, locked wrapper links the separately cached Xen kernel.
    cat >> "$CARGO_HOME/config.toml" <<TOML
    [target.x86_64-unknown-hermit.hermit]
    rustc-link-search = ["native=${kernel}/lib"]
    rustc-link-lib = ["static=hermit"]
    TOML
    export RUSTFLAGS='-Crelocation-model=static -Cforce-frame-pointers=yes'
    cargo build --locked --offline --manifest-path ${pkgs.lib.escapeShellArg manifestPath} \
      --bin ${pkgs.lib.escapeShellArg bin} ${pkgs.lib.optionalString (features != []) ("--features " + pkgs.lib.escapeShellArg (pkgs.lib.concatStringsSep "," features))} \
      --target x86_64-unknown-hermit -Zbuild-std=std,panic_abort --release
  '';
  installPhase = ''
    mkdir -p $out
    cp "$CARGO_TARGET_DIR/x86_64-unknown-hermit/release/"${pkgs.lib.escapeShellArg bin} $out/kernel.elf
    readelf -h -l -n $out/kernel.elf > $out/elf.txt
    sha256sum $out/kernel.elf > $out/sha256.txt
    cp ${provenance} $out/provenance.json
    cat > $out/application.json <<'JSON'
    ${builtins.toJSON { source = toString src; inherit bin manifestPath features; }}
    JSON
    cp $(dirname ${pkgs.lib.escapeShellArg manifestPath})/Cargo.lock $out/Cargo.lock
  '';
}
