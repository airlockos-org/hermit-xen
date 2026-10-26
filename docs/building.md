# Building applications

The application builder is independent of the QEMU/NixOS laboratory. It accepts a
source tree with a Cargo.lock and a binary target. It supplies the patched Hermit
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
your application's Cargo.lock. The builder substitutes the patched hermit-rs
source via Cargo configuration and enables `hermit/xen`; it does not edit your
checkout. Direct Cargo builds use the declared upstream source, which does not
contain these Xen patches. Use the builder for real Xen execution.

The output contains `kernel.elf`, `elf.txt`, `sha256.txt`, `provenance.json` and the
resolved build Cargo.lock. It preserves debug information, PVH notes and frame
pointers. `nix build .#hello -o result-hello` exercises the same exported function
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

Use one reviewed hermit-xen revision for the transport, builder and patches:

| Input | Revision |
| --- | --- |
| Hermit kernel (PVH PR 2366) | `ac927733a70871c4b9e33d1ad4f72823836793e6` |
| hermit-rs | `e9dc6784ad1a2dc6d18c6819ee51198cab9fb78a` |
| rust-vmm/Xen (guest PR 16) | `a8529dc1e50445e5579a9aa0abd763348460bdcd` |
| Rust | `nightly-2026-08-01`, build-std |

Kernel features are `xen,pvh,fsgsbase,kernel-stack`. The application-facing syscall
ABI, grant bootstrap header and wire format version 1 must match. The kernel
exports the `sys_xen_*` functions used by the Hermit adapter; neither the current
crates.io Hermit release nor upstream Git without patches is interchangeable.
The transport core and Linux adapter build with stable Rust; the pinned nightly
is required for the Hermit kernel/application build. The laboratory currently
validates one x86_64 vCPU and one peer. The caller selects and authorizes its peer;
the laboratory delegates that capability specifically to dom0.

`flake.lock` pins the remaining platform inputs. `patches/SHA256SUMS` and generated
provenance identify local modifications. Cargo lockfiles remain tracked for the
controller, test applications and standalone example.
