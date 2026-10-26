# Testing and diagnostics

Run commands from the repository root after `nix develop`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
nix flake check
cargo run -- test --guest ipc --repeat 3
cargo run -- test --guest ipc --no-build --cycles 3
cargo run -- test --guest ipc --ipc-case malformed --no-build
cargo run -- test --guest ipc --ipc-case peer-failure --no-build
cargo run -- test --guest hermit
cargo run -- test --guest linux
```

The fork formatters use the same pinned nightly as the kernel:

```sh
nix develop .#hermit -c cargo fmt --manifest-path vendor/hermit-kernel/Cargo.toml --all --check
nix develop .#hermit -c cargo fmt --manifest-path vendor/rust-vmm-xen/Cargo.toml --all --check
```

The normal IPC test checks receive timeout, running-clock progress, busy grant
revocation, 83 typed requests, masked/repeated notifications, both full queues,
blocking response backpressure, cancellation framing and clean shutdown. Fault
scenarios must still end in clean Hermit shutdown; intentional peer exit 42 is
expected only for the peer-failure case.

These negative tests **must exit nonzero**, retain diagnostics and leave no VM:

```sh
cargo run -- test --guest hermit --no-build --cmdline ' -- --panic'
cargo run -- test --guest hermit --no-build --cmdline ' -- --hang' --timeout 5
cargo run -- test --guest ipc --no-build --expect DELIBERATELY_MISSING
cargo run -- reset
```

`test --kernel PATH --guest hermit --no-build` tests another ELF. For a custom
application with different markers, use `--guest linux --kernel PATH --initrd
none --expect YOUR_MARKER --no-build`; the generic lifecycle checker still
requires real Xen PVH and clean poweroff. The built-in Hermit mode deliberately
requires all boot-fixture assertions. Do not use `--no-build` after source edits
unless intentionally testing a previously built image.

Each run has `.lab/runs/<UTC>-<pid>/summary.json`; `.lab/latest` identifies the
latest directory. QEMU stdout/stderr/status, serial console, QMP/RPC transcripts,
Xen logs, dom0 journal, guest ELF/hash, peer binary/hash, and best-effort vCPU
registers/symbolization are retained. `cycle-N/` contains per-guest lifecycle,
console, peer exit status, assertions, idle samples and latency measurements.
Failed runs retain evidence before bounded teardown. `logs` reads the summary;
`reset` stops verified laboratory processes without deleting diagnostic history.

Warm cycles preserve Xen/dom0 but recreate guest resources. They run the same
input ELF for a bounded batch; an interactive daemon supporting rebuilds between
commands is not implemented. Cold runs remain the isolation gate.

Historical measurements on the development AMD nested-KVM host were 22–23 s
cold, 3.4–3.5 s warm (including a deliberate 2 s idle sample), median RTT 148–258 µs,
and 0.0% sampled idle CPU. These are not guarantees. xentop memory measures domain
allocation, not live Rust heap usage; xl CPU-time resolution is coarse. Fresh
summaries contain the measurements from the actual test environment.

Generated logs, evidence and binaries are ignored. Only deterministic test source
belongs in Git. The Python dom0 RPC helper executes bounded argv requests; all
lab lifecycle, assertions and result handling remain in Rust.
