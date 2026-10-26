{
  description = "Real Xen on nested KVM, with Linux and native Hermit PVH smoke tests";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/5dfba6236110080a54247d6460bc2ff5dda939cc";
  inputs.rust-overlay = { url = "github:oxalica/rust-overlay"; inputs.nixpkgs.follows = "nixpkgs"; };
  inputs.hermit-kernel = { url = "github:hermit-os/kernel/ac927733a70871c4b9e33d1ad4f72823836793e6"; flake = false; };
  inputs.hermit-rs = { url = "github:hermit-os/hermit-rs/e9dc6784ad1a2dc6d18c6819ee51198cab9fb78a"; flake = false; };
  inputs.xen-rust = { url = "github:rust-vmm/xen/a8529dc1e50445e5579a9aa0abd763348460bdcd"; flake = false; };
  outputs = { self, nixpkgs, rust-overlay, ... }@inputs:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      rustPkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
      hermitToolchain = rustPkgs.rust-bin.nightly."2026-08-01".minimal.override {
        extensions = [ "rust-src" "llvm-tools" ];
        targets = [ "x86_64-unknown-none" "x86_64-unknown-linux-musl" ];
      };
      hermit = import ./nix/hermit.nix { inherit pkgs inputs; toolchain = hermitToolchain; };
      assets = import ./nix/lab.nix { inherit pkgs nixpkgs system; };
      fixtures = import ./nix/fixtures.nix { inherit pkgs hermit; toolchain = hermitToolchain; };
    in {
      lib.${system} = {
        buildHermitApplication = hermit.buildApplication;
        dependencyLock = ./nix/hermit-deps.lock;
      };
      nixosConfigurations.dom0 = assets.dom0;
      packages.${system} = {
        default = assets.lab;
        lab = assets.lab;
        hello = hermit.buildApplication {
          src = pkgs.lib.fileset.toSource { root = ./examples/hello; fileset = pkgs.lib.fileset.unions [ ./examples/hello/Cargo.toml ./examples/hello/Cargo.lock ./examples/hello/src ]; };
          bin = "hello-hermit-xen";
        };
        hermit-smoke = fixtures.smoke;
        hermit-ipc = fixtures.ipc;
        ipc-peer = fixtures.peer;
        hermit-kernel = hermit.kernel;
        hermit-sources = hermit.sources;
        boot-iso = assets.iso;
        guest-initrd = assets.guest;
        controller = pkgs.rustPlatform.buildRustPackage {
          pname = "xen-lab";
          version = "0.1.0";
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./crates/xen-transport ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          cargoTestFlags = [ "--workspace" ];
          meta.mainProgram = "xen-lab";
        };
      };
      checks.${system} = {
        xen-ring = hermit.ringCheck;
        controller = self.packages.${system}.controller;
        external-application = self.packages.${system}.hello;
        patch-hashes = pkgs.runCommand "patch-hashes" {} ''
          cd ${./patches}
          sha256sum --check SHA256SUMS
          touch $out
        '';
      };
      devShells.${system} = {
        default = pkgs.mkShell {
          packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.qemu_kvm pkgs.binutils ];
        };
        hermit = pkgs.mkShell {
          packages = [ hermitToolchain pkgs.pkg-config pkgs.openssl pkgs.binutils pkgs.gdb pkgs.qemu_kvm ];
        };
      };
    };
}
