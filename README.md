# hermit-xen

Native Xen PVH support for the Rust Hermit unikernel, with a reproducible
real-Xen laboratory and a small typed Rust transport. This is experimental
work on pinned upstream revisions, not an upstream-supported production release.

Working features include direct Hermit ELF boot, console output, shutdown,
event-channel reception/notification, blocking receive, Xen PV time/timers,
grant-backed bounded queues and typed bidirectional Hermit/Linux communication.

## Build and test

Requires an x86_64 Linux host with Nix 2.27+ and flakes, `/dev/kvm` access, nested VMX/SVM,
and about 6 GiB of available memory. No permanent host configuration changes are
made. The first build downloads pinned dependencies; later builds use Nix caching.

```sh
git clone --recurse-submodules https://github.com/airlockos-org/hermit-xen.git
cd hermit-xen
nix develop
cargo run -- doctor
cargo run -- test --guest ipc          # build, boot real Xen, test, collect, stop
cargo run -- test --guest ipc --no-build --cycles 3  # reuse Xen/dom0
cargo run -- test --guest hermit       # Hermit boot regression
cargo run -- test                      # Linux PVH regression
cargo run -- logs
cargo run -- reset
```

Xen 4.20.4 runs under QEMU/KVM with NixOS dom0. Each guest gets disposable state.
Logs, retained ELFs and JSON summaries live in `.lab/runs/` and are never committed.
`--repeat 3` performs isolated cold boots; `--cycles 3` creates fresh domains
within one outer VM. See [testing and diagnostics](docs/testing.md) for fault tests.

## Use from another project

`crates/xen-transport` is `no_std` by default, with optional `hermit` and `linux`
adapters. Consume it as a Cargo Git dependency pinned to a reviewed commit:

```toml
xen-transport = { git = "https://github.com/airlockos-org/hermit-xen", rev = "<commit>", features = ["hermit"] }
```

The flake exports `lib.x86_64-linux.buildHermitApplication` for external application
sources. It links the matching forked kernel and selects the pinned toolchain.
[Building applications](docs/building.md) gives the complete interface and example;
[the transport README](crates/xen-transport/README.md) describes the Rust API.

## Scope

Currently: x86_64, one vCPU, one authorized peer, eight queued messages per direction,
224-byte payloads, and a fresh domain for reconnection. Peer authorization and
application policy are the caller's responsibility. Suspend/resume, migration,
SMP and production identity provisioning are not validated.

Use the kernel, transport and toolchain together as specified by the
[compatibility contract](docs/building.md#compatibility). Detailed
[IPC architecture](docs/xen-ipc.md), [boot notes](docs/boot.md) and
[contributor workflow and provenance](docs/contributing.md) describe the implementation.

Original code: MIT OR Apache-2.0. See [third-party notices](THIRD_PARTY.md) for
upstream attribution and the separate licenses of laboratory dependencies.
