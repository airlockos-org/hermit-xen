# Native Xen IPC foundation

This implementation connects a genuine Hermit PVH domU to a Rust process in Linux dom0 using Xen grant mappings and an interdomain event channel. Dom0 is the second Xen domain, not a host-side shared-memory emulator. The outer QEMU runs real Xen with KVM. Neither QEMU's Xen ABI emulation nor a socket carries application messages.

## Build and exercise

```sh
nix develop
cargo run -- test --guest ipc                       # build everything, cold boot, test, stop
cargo run -- test --guest ipc --no-build --repeat 3 # three independent cold boots
cargo run -- test --guest ipc --no-build --cycles 3 # one Xen boot, three fresh Hermit domains
cargo run -- test --guest ipc --ipc-case malformed --no-build
cargo run -- test --guest ipc --ipc-case peer-failure --no-build
cargo run -- test --guest hermit                    # original application regression
cargo run -- test                                  # original Linux PVH regression
cargo run -- logs
cargo run -- reset
cargo test --workspace
nix build .#controller .#checks.x86_64-linux.xen-ring --no-link
```

`nix build .#hermit-ipc -o result-ipc` and `nix build .#ipc-peer -o result-peer` build only the applications. The Linux peer is a static musl executable; changing it does not rebuild the dom0 image. Nix caches the kernel archive separately from the application, so changes to application/transport code do not rebuild Hermit's kernel or Xen. No ignored checkout or symlink is required by these Nix builds. All upstream inputs and Rust dependencies are pinned.

`--kernel /absolute/path/to/kernel.elf --guest ipc --no-build` tests a development ELF with the same controller. `--timeout` bounds guest completion. Every run retains its ELF and hashes. The RPC transport, test controller, and timeout/cleanup machinery are shared with the existing boot tests.

`--cycles` keeps Xen and dom0 alive for a bounded batch, destroys each tested domain, resets peer services, and creates a new domain with fresh pages and ports. It does not yet provide an interactive daemon that waits while an agent rebuilds between invocations. Cold boots remain the final isolation gate. A future persistent command can reuse these lifecycle operations without changing the transport.

## Layers and ownership

| Layer | Responsibility |
| --- | --- |
| `rust-vmm/xen` additions | 64-bit two-level event bitmaps; timer binding; versioned PV time; v1 grant publication/revocation; foreign mapping/unmapping |
| Hermit Xen platform | Shared-info and dedicated mapping pages; callback vector; scheduler timer integration; futex wakeup; single connection capability |
| `crates/xen-transport` | `no_std` bounded queues, private frame validation, deadlines, typed `Wire`/`Channel` API |
| `crates/xen-transport/src/hermit.rs` | Small application adapter through exported Hermit syscalls |
| `tests/guests/hermit-ipc` | Actual Rust application and deterministic acceptance checks |
| `tests/guests/linux-peer` | Rust Linux test peer using the public transport Linux adapter |
| Existing Rust controller | Explicit provisioning, lifecycle, metrics, assertions and preserved evidence |

There is one vCPU, one connection and one peer per Hermit domain. No networking, XenStore service, clipboard policy, or application-specific code is introduced.

## Establishment and authority

The application explicitly selects authorized peer domain 0 for this laboratory capability; the generic adapter accepts a caller-provisioned peer identity. It allocates an unbound event channel restricted to dom0, initializes one outgoing page, and grants that page to dom0. Its console offer carries the local event port, grant reference and a connection generation/session value. The controller obtains the actual current domain ID from Xen and passes the offer to the dom0 peer.

Linux binds the event channel, maps the Hermit page, allocates a Linux-owned grant page restricted to the Hermit domain, initializes it, then publishes its grant reference and a release-ordered ready flag in the bootstrap header. Hermit acquires that flag, maps the Linux grant into a reserved physical slot, and validates the protocol header/session.

This is controlled test provisioning. The controller, dom0 and hypervisor are trusted authorities. Port numbers, grant references, domain IDs and session values are **not credentials**. Production must bind an independently authorized service identity to the current Xen domain incarnation before exchanging capabilities; it must not trust a grant advertised by an arbitrary domain. Dom0/the hypervisor can inspect guest memory. The existing Hermit fallback entropy source is not suitable for secrets or cryptographic authentication.

## Shared-memory protocol

The upstream console byte ring is retained for console output. IPC uses a small separate framed SPSC queue because foreign mutable byte slices and a byte-stream API do not provide the ownership and private validation required for untrusted typed messages.

Each direction uses one page and one SPSC queue. The producer owns the page and producer counter; the peer owns its consumer counter. Capacity is eight 256-byte frames, with at most 224 payload bytes per message. Producer and consumer counters occupy separate cache lines. No allocation occurs per message.

The frame has an explicit little-endian version, kind, payload length, request ID, reserved fields and checksum. Rust enums and pointers are never transmitted. Receive copies a fixed-size frame through volatile accesses into private storage **before** checking its length, kind, checksum, padding and typed schema. A hostile peer cannot turn a shared length into an unchecked slice or Rust reference. The checksum detects inconsistent/corrupt snapshots; it provides no authentication.

Producer payload writes precede a release publication of the producer index. Consumer acquires that index, copies the frame, then releases the consumer index. Producer acquires the consumer index before reusing a slot. Index arithmetic wraps at `u32::MAX`; differences larger than capacity or modification of the locally owned counter fail closed. Unit tests cover rollover, full queues, malformed frames and invalid indices. These ordering assumptions require coherent Xen shared memory; validation currently covers x86_64.

A full queue returns `Full` for the nonblocking endpoint; the channel waits for consumer progress with a deadline. Malformed frames are consumed once and reported. A dead peer, corrupt queue, expired deadline and platform failure have distinct errors. A successful send means accepted into the queue, not that application processing completed. Request IDs and typed responses supply application acknowledgement. Timeout does not retract a message already published; cancellation policy belongs above the transport.

Application code can implement `Wire` and use `send_typed`/`recv_typed` without Xen structures. The demo schema intentionally stays small. Clipboard payloads larger than 224 bytes require a separately specified bounded fragmentation protocol or a larger bounded transport; unbounded blobs are not supported implicitly.

## Interrupts, blocking and time

The platform selects Xen's 64-bit shared-info ABI and installs HVM callback vector `0xf3`. Atomic pending/mask bitmap operations process a bounded snapshot. `EVTCHNOP_unmask` handles notifications accumulated while masked. Notifications are hints to inspect queue state, not message counts; both producer publication and consumer progress signal the peer.

The channel samples an event-generation counter before checking its queue, then the kernel compares that counter while atomically parking on Hermit's existing futex machinery. This closes the check/sleep race. Interrupts advance the generation and wake waiters. Hermit's idle path uses `sti; hlt`, not ordinary busy polling. An IPC deadline uses a dedicated slot in Hermit's existing timer list, canceled when an event wakes the task. No parallel scheduler or allocator is introduced.

The timer is Xen's `VIRQ_TIMER` plus absolute `set_timer_op`; the periodic Xen timer is disabled. Scheduler sleep and IPC deadlines share the existing timer multiplexer. PV time reads versioned shared-time snapshots with barriers, ordered RDTSC and Xen's scaling algorithm. TSC subtraction is wrapping: an HVM TSC offset can make the initial reference stamp wrap below zero. A saturating subtraction incorrectly froze early timestamps and was caught during implementation. A monotonic clamp handles rescheduling updates, but every read still obtains a fresh snapshot. Tests check time advancing while running, receive timeout, sleep, and monotonic timestamps across repeated requests. Suspend/resume and live migration are not validated.

## Grants and teardown

The platform maps a v1 grant-table page, reserves reference 8, and publishes its target domain/PFN before release-storing permission flags. Revocation uses compare-and-exchange and refuses while Xen's reading/writing bits are set. The test verifies that an active peer mapping makes revocation busy.

Foreign mapping checks both the hypercall result and per-operation grant status. Unmapping invalidates the mapping before its capability is discarded. Closing unmaps the incoming page, revokes the outgoing grant, masks and closes the event channel. A busy grant retains its backing page; it is never freed or reused merely because a deadline expired. Dedicated static physical slots are quarantined until domain teardown, including failed establishment. Reconnection within one Hermit domain is intentionally unsupported in this milestone.

Linux unmap notification clears the shared alive byte and sends an event even on `process::exit(42)`, where Rust destructors do not run. The peer-failure test exercises that kernel path. Linux may defer final grant-page reclamation while the other domain still maps it. Hermit must unmap before reclamation can finish. Controller failure cleanup stops the peer, captures state, destroys the domain and shuts down the outer VM.

Unsafe boundaries cover hypercalls/ioctls, raw mapped pages, versioned time reads and the interrupt ABI. Foreign memory is never exposed as an ordinary Rust slice. `Backend::pages` is unsafe and requires the backend to remain open; `Channel` owns the backend and prevents use after close. These are systems-level contracts, not claims of formally verified memory safety.

Useful future proof invariants are: exactly one owner per counter; `producer-consumer <= capacity`; a slot changes from free to published to consumed before reuse; decoded data lives in private memory; a grant page cannot be recycled until successful revocation; a foreign mapping cannot outlive its reserved slot; and sleep is entered only while the observed generation still matches.

## Evidence and limits

Run-specific evidence is generated under `.lab/runs/` and excluded from Git. See [testing](testing.md) for reproducible validation and historical measurements.

The run summary records actual QEMU exit status, Xen shutdown reason, assertions, idle samples, memory allocation and peer latency events. `cycle-N/` retains each guest's console, peer output/status, lifecycle, Xen configuration and idle measurements. Root artifacts contain full Xen/dom0 logs and the final diagnostic snapshot. Failure paths retain registers and symbolization where available.

Normal tests exercise 64 sequential RTTs, two queued batches, full request/response queues, 64 repeated notifications while masked, timeout, cancellation framing and close. Fault cases inject a malformed frame and terminate the peer without Rust cleanup. Cancellation is an acknowledged demo operation; it does not implement clipboard cancellation policy.

Measured Xen memory is allocated/resident domain memory, not live Rust heap usage. The kernel reserves most available RAM for its heap. `xl` CPU-time resolution is 0.1 seconds, so its idle delta only yields an upper bound; the controller also samples Xen's `xentop` CPU percentage. Latency is a dom0 userspace request-to-response measurement under nested virtualization and includes scheduling. These numbers are development-lab measurements, not production performance guarantees.

Supported scope remains a single x86_64 vCPU with one authorized peer. Reconnect, multi-peer resource accounting, SMP, suspend/resume/migration, XenStore discovery, device frontends and production identity delegation remain future work. A malicious authorized peer can deny service or retain a grant; memory safety is protected by bounded validation/quarantine, not by forcibly revoking an in-use page. Bootstrap mappings and entropy retain the earlier Hermit milestone's limitations.

## Upstream provenance

Base inputs remain kernel PR [2366](https://github.com/hermit-os/kernel/pull/2366), revision `ac927733a70871c4b9e33d1ad4f72823836793e6`; rust-vmm Xen PR [16](https://github.com/rust-vmm/xen/pull/16), revision `a8529dc1e50445e5579a9aa0abd763348460bdcd`; and hermit-rs revision `e9dc6784ad1a2dc6d18c6819ee51198cab9fb78a`. The template investigation and original boot changes are documented in `docs/boot.md`.

The maintained `vendor/rust-vmm-xen` submodule provides helpers, events, time and
grants; `vendor/hermit-kernel` integrates them with Hermit's timer, interrupt and
connection lifecycle. Linux device wrappers remain in the transport crate. The
parent Git tree pins compatible fork commits; Nix uses those checked-out sources
without applying patches. See [contributing](contributing.md) for provenance and
updates. The unchanged hermit-rs wrapper links the prebuilt archive using Cargo's
standard `links` build-script override.

Consulted references: [Mini-OS](https://github.com/xen-project/mini-os/tree/229260e02faacab0f4d5ccb82f013480d968d335) time/event handling; [Mirage Xen](https://github.com/mirage/mirage-xen/tree/be1cb7e5fdf65d853d0cebefc37c929ce35003e1) event bindings; [Xen 4.20.4 time implementation](https://github.com/xen-project/xen/blob/RELEASE-4.20.4/xen/arch/x86/time.c); and [Linux 6.18 gntalloc](https://github.com/torvalds/linux/blob/v6.18/drivers/xen/gntalloc.c). No source is fetched from those moving branches during builds.
