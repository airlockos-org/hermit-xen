# Public builder. The dependency vendor must contain the application's locked
# dependencies and the pinned build-std dependencies (see docs/building.md).
{ pkgs, toolchain, kernel, wrapper, deps, setup, provenance }:
{ src, bin, manifestPath ? "Cargo.toml", cargoVendorDir ? deps, pname ? bin }:
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
    cp -r ${wrapper} "$TMPDIR/hermit-rs"
    chmod -R u+w "$TMPDIR/hermit-rs"
    cat >> "$CARGO_HOME/config.toml" <<TOML
    [patch."https://github.com/hermit-os/hermit-rs"]
    hermit = { path = "$TMPDIR/hermit-rs/hermit" }
    TOML
    export HERMIT_LIB_DIR=${kernel}/lib
    export RUSTFLAGS='-Crelocation-model=static -Cforce-frame-pointers=yes'
    # Cargo changes the locked Hermit source to the matching local patched wrapper.
    # Offline resolution retains the other versions in the caller's Cargo.lock.
    cargo build --offline --manifest-path ${pkgs.lib.escapeShellArg manifestPath} \
      --bin ${pkgs.lib.escapeShellArg bin} --features hermit/xen \
      --target x86_64-unknown-hermit -Zbuild-std=std,panic_abort --release
  '';
  installPhase = ''
    mkdir -p $out
    cp "$CARGO_TARGET_DIR/x86_64-unknown-hermit/release/"${pkgs.lib.escapeShellArg bin} $out/kernel.elf
    readelf -h -l -n $out/kernel.elf > $out/elf.txt
    sha256sum $out/kernel.elf > $out/sha256.txt
    cp ${provenance} $out/provenance.json
    cp $(dirname ${pkgs.lib.escapeShellArg manifestPath})/Cargo.lock $out/Cargo.lock
  '';
}
