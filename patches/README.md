# Upstream patch series

These patches are applied to immutable flake inputs; no fork is required.
`SHA256SUMS` is checked by `nix flake check`. Application provenance records the
same hashes. Preserve upstream source headers and notices when editing patches;
see [third-party notices](../THIRD_PARTY.md).

| Target | Base | Apply in order |
| --- | --- | --- |
| hermit-os/kernel | `ac927733a70871c4b9e33d1ad4f72823836793e6` (PVH PR 2366) | `hermit/01-xen-boot.patch`, `hermit/02-xen-ipc.patch` |
| hermit-os/hermit-rs | `e9dc6784ad1a2dc6d18c6819ee51198cab9fb78a` | `hermit/03-wrapper.patch` |
| rust-vmm/xen | `a8529dc1e50445e5579a9aa0abd763348460bdcd` (guest PR 16) | `xen/01-guest-helpers.patch`, `xen/02-events-time-grants.patch` |

The boot patch adds Xen platform selection, console, shutdown and bootstrap
adaptations to upstream PVH. The IPC patch integrates event delivery, PV timers,
futex wakeups and single-connection grant lifetimes with Hermit's existing runtime.
The wrapper patch exposes the Xen feature and a prebuilt kernel archive override.

Generic Xen patches repair/export existing hypercall and console-ring helpers,
then add two-level event handling, versioned PV time and v1 grants. Linux
userspace grant/device support lives once in `xen-transport::linux`, rather than
requiring consumers to reconstruct a patched xen-unix dependency tree.

Consulted upstream references: Mini-OS `229260e02faacab0f4d5ccb82f013480d968d335`,
Mirage Xen `be1cb7e5fdf65d853d0cebefc37c929ce35003e1`, and Xen 4.20.4/Linux 6.18
public ABI and implementation. The PV time algorithm, Linux ioctl layouts and
mask/unmask behavior follow those contracts; Rust implementations use documented
unsafe boundaries. Upstream notices are retained in THIRD_PARTY.md and sources.

Changes remain experimental, not upstream-accepted. Suitable contribution units
are the generic Xen helpers/events/time/grants, Hermit platform integration and
prebuilt wrapper support. No separate upstream forks are published here.
