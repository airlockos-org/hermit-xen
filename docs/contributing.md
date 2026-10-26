# Contributing and dependency updates

The two forks hold native Xen support pending upstream integration:

| Checkout | Fork branch | Upstream |
| --- | --- | --- |
| `vendor/hermit-kernel` | `airlockos-org/hermit-kernel`, `xen-pvh` | `hermit-os/kernel` |
| `vendor/rust-vmm-xen` | `airlockos-org/rust-vmm-xen`, `xen-pvh` | `rust-vmm/xen` |

The parent Git links are the authoritative pins. There is no patch application
step, second kernel/Xen fetch, or generated dependency checkout. Hermit's Xen
Cargo path dependencies resolve to the sibling rust-vmm-xen submodule. The
upstream rust-vmm-ci nested submodule is retained at its original pin.

## Edit, test, publish

```sh
git clone --recurse-submodules https://github.com/airlockos-org/hermit-xen.git
cd hermit-xen
# Also needed after switching parent revisions:
git submodule update --init --recursive
# Submodules normally start detached: create a topic branch before editing.
git -C vendor/hermit-kernel switch -c my-xen-change
# Edit either vendor checkout directly, then run the normal builder and tests.
nix build .#hermit-kernel .#hermit-ipc
cargo run -- test --guest ipc
```

Run the [full regression suite](testing.md) before publishing a compatible update.
`nix build` includes tracked submodule edits; add new files with Git so Nix sees
them. Cached kernel and outer VM artifacts remain reusable for application edits.
`nix build .#hermit-sources` exports the exact source tree for inspection.

Commit inside each changed submodule first. HTTPS URLs are public; contributors
with write access can set an SSH push URL locally, for example:

```sh
git -C vendor/hermit-kernel remote set-url --push origin git@github.com:airlockos-org/hermit-kernel.git
git -C vendor/hermit-kernel add <files>
git -C vendor/hermit-kernel commit
# Publish the topic branch for review, or fast-forward xen-pvh after review.
git -C vendor/hermit-kernel push -u origin HEAD
# Repeat for rust-vmm-xen when changed. Verify both commits are public first.
git add vendor/hermit-kernel vendor/rust-vmm-xen
git commit -m 'Update compatible Xen guest dependencies'
git push origin main
```

To consume an already published update, fetch the relevant fork and check out the
reviewed commit inside its submodule, initialize nested submodules recursively,
validate, then commit the parent Git link. Do not use `submodule update --remote`
as an implicit build step. Do not force-push or rewrite published upstream history.
A fresh recursive clone of the parent commit must build without local artifacts.
Nix configuration details and external flake URL requirements are in
[building applications](building.md).

## Provenance

The migration preserves the validated source from parent commit
`8b2abd8f6989d11071f407eb1d760b7061329c8d`. Its four kernel/Xen patches became
ordinary commits, in their original order, on these upstream PR bases:

- Hermit PVH [PR #2366](https://github.com/hermit-os/kernel/pull/2366):
  `ac927733a70871c4b9e33d1ad4f72823836793e6`. Boot/console/shutdown changes precede
  scheduler, event, PV timer and grant-connection integration.
- rust-vmm/Xen guest [PR #16](https://github.com/rust-vmm/xen/pull/16):
  `a8529dc1e50445e5579a9aa0abd763348460bdcd`. Hypercall/console fixes precede
  reusable events, versioned time and v1 grants.
- The former hermit-rs linking patch was eliminated using Cargo's supported
  build-script override. Unmodified hermit-rs remains pinned at
  `e9dc6784ad1a2dc6d18c6819ee51198cab9fb78a` with nightly-2026-08-01.

Each fork's `XEN-PVH.md` records scope and attribution. Mini-OS
`229260e02faacab0f4d5ccb82f013480d968d335` and Mirage Xen
`be1cb7e5fdf65d853d0cebefc37c929ce35003e1` informed time/event behavior; notices
remain in [THIRD_PARTY.md](../THIRD_PARTY.md) and the Xen fork.

For upstream contributions, separate generic Xen helpers/events/time/grants from
Hermit's platform/runtime integration. Compare against the recorded PR bases and
coordinate with those PR authors; do not resubmit their existing commits as new
work. Submit focused commits to the corresponding upstream repository, retaining
attribution and test evidence. Sibling path wiring is integration configuration
that needs adjustment to an accepted upstream dependency before upstreaming.
These branches do not imply upstream acceptance. Existing single-vCPU/peer,
224-byte framing, fresh-domain reconnection and entropy limitations remain.
