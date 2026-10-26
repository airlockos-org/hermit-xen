# xen-transport

A bounded SPSC message transport for coherent Xen shared memory. The core is
`no_std`, with no default dependencies. It provides `Message`, `Wire`, `Channel`,
`Backend`, and low-level page/queue capabilities. Wire payloads are limited to
224 bytes; each direction holds eight messages. Received frames are copied into
private storage before decoding. Notifications indicate possible work, not a
message count.

Enable `hermit` for the matching patched Hermit kernel, or `linux` for a Linux
userspace adapter using `/dev/xen/{evtchn,gntdev,gntalloc}`. The Linux adapter
requires device permissions and currently targets x86_64 Linux. No Xen definitions
or hypercall implementations are duplicated in the application-facing core.

```rust,ignore
use xen_transport::{Channel, Offer, Peer};
use xen_transport::linux::Linux;

// Obtain these values from a trusted provisioning authority, not an arbitrary
// peer message. Neither a domain ID, port, grant nor session authenticates anyone.
let peer = Peer { domain: authorized_domain };
let backend = Linux::open(peer, authenticated_offer)?;
let mut channel = Channel::new(backend);
let deadline = channel.deadline_after(1_000_000_000);
channel.send_typed(request_id, &request, deadline)?;
let (id, response) = channel.recv_typed::<Response>(deadline)?;
channel.close(channel.deadline_after(1_000_000_000))?;
```

For Hermit, `hermit::Hermit::connect(peer, timeout_ns, publish_offer)` returns a
channel. The callback must pass its `Offer` to the authorized peer through a
trusted provisioning mechanism. Connection establishment and the exported
`sys_xen_*` ABI are experimental and versioned by the repository revision.

Applications implement `Wire` for their own bounded request/response types.
`Full`, `Empty`, `Timeout`, `Closed`, `Malformed`, `Corrupt` and `Platform` errors
remain distinct. Sending acknowledges queue publication, not application execution.
Timeout does not retract an already published request. Cancellation and identity
policy belong to the application.

One channel owns its backend. Raw page APIs require the documented mapping,
lifetime and exclusive endpoint invariants; prefer `Channel` in applications.
On failed revocation, backing pages must remain pinned until domain teardown.
See the repository's IPC architecture for ordering, cleanup and threat assumptions.
