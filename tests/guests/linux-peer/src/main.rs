use std::time::{Duration, Instant};
use xen_transport::{
    Backend, CAPACITY, Consumer, Error, Kind, MAX_PAYLOAD, Message, Offer, Peer, Producer, Wire,
    linux::Linux,
};
#[path = "../../schema.rs"]
mod schema;
use schema::{Operation, Request, Response};
fn request(id: u64, operation: Operation) -> Message {
    let r = Request {
        operation,
        value: id,
    };
    let mut b = [0; MAX_PAYLOAD];
    let n = r.encode(&mut b).unwrap();
    Message::new(Kind::Request, id, &b[..n]).unwrap()
}
fn recv_raw(backend: &mut Linux, rx: &mut Consumer, deadline: u64) -> Result<Message, Error> {
    loop {
        let generation = backend.generation();
        match rx.try_recv() {
            Err(Error::Empty) => backend.wait(generation, deadline)?,
            r => {
                backend.notify()?;
                return r;
            }
        }
    }
}
fn send_raw(
    backend: &mut Linux,
    tx: &mut Producer,
    m: &Message,
    deadline: u64,
) -> Result<(), Error> {
    loop {
        let generation = backend.generation();
        match tx.try_send(m) {
            Ok(()) => return backend.notify(),
            Err(Error::Full) => backend.wait(generation, deadline)?,
            Err(e) => return Err(e),
        }
    }
}
fn verify(message: Message, id: u64, last: &mut u64) {
    assert_eq!(message.id, id);
    assert_eq!(message.kind, Kind::Response);
    let r = Response::decode(message.payload()).unwrap();
    assert_eq!(r.value, id * 2);
    assert!(r.timestamp_ns >= *last);
    *last = r.timestamp_ns;
}
fn run() -> Result<(), Error> {
    let a: Vec<_> = std::env::args().collect();
    assert_eq!(a.len(), 6, "peer DOMID PORT GRANT SESSION CASE");
    let mut backend = Linux::open(
        Peer {
            domain: a[1].parse().unwrap(),
        },
        Offer {
            port: a[2].parse().unwrap(),
            grant: a[3].parse().unwrap(),
            session: a[4].parse().unwrap(),
        },
    )?;
    println!("{{\"event\":\"connected\",\"peer_domid\":{}}}", a[1]);
    // Guest verifies an idle deadline before this peer sends any data.
    std::thread::sleep(Duration::from_millis(500));
    let (p, q) = unsafe { backend.pages() };
    let mut tx = unsafe { Producer::new(p) };
    let mut rx = unsafe { Consumer::new(q) };
    if a[5] == "peer-failure" {
        println!("{{\"event\":\"intentional_peer_exit\"}}");
        std::process::exit(42);
    }
    if a[5] == "malformed" {
        // Publish a malformed first frame before notifying the receiver.
        // Peer has not been signaled and waits for data; only mapped raw bytes
        // are touched. The transport copies and validates before decoding.
        unsafe {
            let (p, _) = backend.pages();
            corrupt_first_frame(p);
        }
        backend.notify()?;
        let deadline = backend.now_ns() + 2_000_000_000;
        assert_eq!(recv_raw(&mut backend, &mut rx, deadline)?.kind, Kind::Error);
        backend.close(0)?;
        println!("{{\"event\":\"resources_released\",\"maps\":0,\"grants\":0}}");
        println!("IPC_PEER_PASS");
        return Ok(());
    }
    let mut latencies = Vec::new();
    let mut last = 0;
    for id in 0..64 {
        let start = Instant::now();
        let deadline = backend.now_ns() + 2_000_000_000;
        send_raw(
            &mut backend,
            &mut tx,
            &request(id, Operation::Echo),
            deadline,
        )?;
        verify(recv_raw(&mut backend, &mut rx, deadline)?, id, &mut last);
        latencies.push(start.elapsed().as_nanos() as u64);
    }
    let deadline = backend.now_ns() + 2_000_000_000;
    send_raw(
        &mut backend,
        &mut tx,
        &request(99, Operation::Mask),
        deadline,
    )?;
    verify(recv_raw(&mut backend, &mut rx, deadline)?, 99, &mut last);
    for id in 100..100 + u64::from(CAPACITY) {
        tx.try_send(&request(id, Operation::Echo))?;
    }
    assert_eq!(
        tx.try_send(&request(999, Operation::Echo)),
        Err(Error::Full)
    );
    for _ in 0..64 {
        backend.notify()?;
    }
    println!("{{\"event\":\"full_queue_and_repeated_notifications\",\"notifications\":64}}");
    std::thread::sleep(Duration::from_millis(350));
    // Response queue is now full. Queue another batch before consuming it,
    // forcing the guest to sleep waiting for response capacity.
    for id in 108..116 {
        let deadline = backend.now_ns() + 2_000_000_000;
        send_raw(
            &mut backend,
            &mut tx,
            &request(id, Operation::Echo),
            deadline,
        )?;
    }
    std::thread::sleep(Duration::from_millis(50));
    for id in 100..116 {
        let deadline = backend.now_ns() + 2_000_000_000;
        verify(recv_raw(&mut backend, &mut rx, deadline)?, id, &mut last);
    }
    for (id, op) in [(900, Operation::Cancel), (999, Operation::Close)] {
        let deadline = backend.now_ns() + 2_000_000_000;
        send_raw(&mut backend, &mut tx, &request(id, op), deadline)?;
        verify(recv_raw(&mut backend, &mut rx, deadline)?, id, &mut last);
    }
    latencies.sort_unstable();
    println!(
        "{{\"event\":\"latency\",\"samples\":64,\"p50_ns\":{},\"p95_ns\":{},\"max_ns\":{}}}",
        latencies[32], latencies[60], latencies[63]
    );
    backend.close(0)?;
    println!("{{\"event\":\"resources_released\",\"maps\":0,\"grants\":0}}");
    println!("IPC_PEER_PASS");
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("IPC_PEER_FAIL {e:?}");
        std::process::exit(1);
    }
}

unsafe fn corrupt_first_frame(page: xen_transport::Page) {
    // Fresh outgoing queue: this fixture owns producer index zero exclusively.
    unsafe {
        for i in 256..512 {
            page.as_ptr().add(i).write_volatile(0xff);
        }
        core::sync::atomic::AtomicU32::from_ptr(page.as_ptr().add(64).cast())
            .store(1, core::sync::atomic::Ordering::Release);
    }
}
