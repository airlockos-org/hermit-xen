# Building applications

The application builder is independent of the QEMU/NixOS laboratory. It accepts a
source tree with a Cargo.lock and a binary target. It supplies the unmodified upstream Hermit
wrapper, prebuilt kernel archive, build-std dependencies and pinned Rust compiler.

```nix
# In an external flake, pin inputs.hermit-xen to a reviewed Git revision.
packages.x86_64-linux.default =
  inputs.hermit-xen.lib.x86_64-linux.buildHermitApplication {
    src = ./.;
    bin = "my-service";
    # manifestPath = "Cargo.toml";  # relative to src
  };
```

Use the pinned dependency declaration in `examples/hello/Cargo.toml` and commit
your application's Cargo.lock. The builder uses Cargo's standard
[`links` override](https://doc.rust-lang.org/cargo/reference/build-scripts.html#overriding-build-scripts)
for `target.x86_64-unknown-hermit.hermit`: it skips the upstream wrapper's build
script and links the prebuilt kernel. No wrapper fork, source patch or Cargo
source replacement is needed. Builds use `--locked --offline`. Kernel features
are chosen by the builder, not the application's `hermit` feature flags. This
builder does not support the wrapper's optional `libc` or `common-os` modes.
Direct Cargo builds without this configuration do not select the Xen kernel.

Use a Git flake reference for external consumers (GitHub archive references do
not carry submodules):

```nix
inputs.hermit-xen.url = "git+https://github.com/airlockos-org/hermit-xen.git?rev=<commit>&submodules=1";
```

Nix 2.27+ is required. `inputs.self.submodules = true` includes the checked-out
submodules for local builds and Git-based consumers. After cloning, run
`git submodule update --init --recursive`; CI checkout must also be recursive.
An uninitialized submodule is an error, not a request to fetch an alternate source.
See [contributing](contributing.md) for the editing workflow.

The output contains `kernel.elf`, `elf.txt`, `sha256.txt`, `provenance.json` and the
resolved build Cargo.lock. It preserves debug information, PVH notes and frame
pointers. After `mkdir -p .lab/build`, `nix build .#hello -o .lab/build/hello` exercises the same exported function
with an independent std-only application. It does not depend on the lab or smoke
application derivation. Applications changing without kernel changes reuse the
kernel archive. Linux peer builds do not depend on that archive.

The optional `cargoVendorDir` argument accepts a Nix `fetchCargoVendor` output
(with its `.cargo/config.toml`). The default vendor covers the pinned Rust std
and repository fixture dependencies. For additional crates, supply a vendor
containing **both** your locked application dependencies and the pinned build-std
requirements; `lib.x86_64-linux.dependencyLock` exports the existing union as a
starting point. Vendor updates must pin checksums and the fixed-output hash.
The builder resolves offline: it never silently downloads missing dependencies.
An arbitrary application dependency resolver/vendor generator is not provided.

## Compatibility

Use one reviewed hermit-xen revision for the transport, builder and submodules:

| Input | Revision |
| --- | --- |
| Hermit kernel upstream base (PVH PR 2366) | `ac927733a70871c4b9e33d1ad4f72823836793e6` |
| hermit-rs | `e9dc6784ad1a2dc6d18c6819ee51198cab9fb78a` |
| rust-vmm/Xen upstream base (guest PR 16) | `a8529dc1e50445e5579a9aa0abd763348460bdcd` |
| Rust | `nightly-2026-08-01`, build-std |

Kernel features are `xen,pvh,fsgsbase,kernel-stack`. The application-facing syscall
ABI, grant bootstrap header and wire format version 1 must match. The kernel
exports the `sys_xen_*` functions used by the Hermit adapter; neither the current
crates.io Hermit release nor upstream Git without patches is interchangeable.
The transport core and Linux adapter build with stable Rust; the pinned nightly
is required for the Hermit kernel/application build. The laboratory currently
validates one x86_64 vCPU and one peer. The caller selects and authorizes its peer;
the laboratory delegates that capability specifically to dom0.

The parent Git tree pins the modified sources exactly (`git submodule status`);
those pins supersede the upstream bases above. `flake.lock` pins the remaining
platform inputs. Application provenance records the content-addressed source tree,
upstream bases, wrapper revision and toolchain. It also identifies local edits in
uncommitted development builds without pretending they are a published revision.
Cargo lockfiles remain tracked for the controller, test applications and example.

External applications may pass `features = [ "feature-name" ];` to the Hermit
builder. `application.json` records the application source store path, binary,
manifest and selected features separately from the kernel's provenance. The
matching `lib.x86_64-linux.buildLinuxApplication` accepts `src`, `bin`,
`manifestPath`, `features` and `cargoVendorDir` and builds a static musl counterpart
for the laboratory. Both builders use locked, offline dependencies; neither
changes the consumer's Cargo manifests.
