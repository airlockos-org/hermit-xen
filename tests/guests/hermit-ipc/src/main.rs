use hermit as _;
mod controls;
#[path = "../../schema.rs"]
mod schema;
use controls::{mask_notifications, verify_busy_revocation};
use schema::{Operation, Request, Response};
use xen_transport::{Error, Kind, Message, Peer, Wire, hermit::Hermit};
fn main() {
    println!("HERMIT_IPC_APP_ENTER");
    let case = std::env::args().nth(1).unwrap_or_else(|| "normal".into());
    let mut channel = Hermit::connect(
        Peer { domain: 0 },
        10_000_000_000,
        |xen_transport::Offer {
             port,
             grant,
             session,
         }| {
            println!("XEN_IPC_OFFER port={port} grant={grant} session={session}");
        },
    )
    .expect("establish authorized lab channel");
    println!("HERMIT_IPC_CONNECTED");
    let clock_start = channel.now_ns();
    for _ in 0..100_000 {
        core::hint::spin_loop();
    }
    let clock_elapsed = channel.now_ns().saturating_sub(clock_start);
    println!("HERMIT_IPC_RUNNING_CLOCK elapsed_ns={clock_elapsed}");
    assert!(clock_elapsed > 0, "PV clock stalled while running");
    verify_busy_revocation().expect("active peer mapping must prevent grant reuse");
    println!("HERMIT_IPC_REVOCATION_BUSY_OK");
    let start = channel.now_ns();
    assert_eq!(
        channel.recv(channel.deadline_after(100_000_000)),
        Err(Error::Timeout)
    );
    assert!(channel.now_ns() >= start + 100_000_000);
    println!("HERMIT_IPC_TIMEOUT_OK");
    let mut count = 0u64;
    let mut last = channel.now_ns();
    loop {
        let result = channel.recv(channel.deadline_after(3_000_000_000));
        let now = channel.now_ns();
        assert!(now >= last, "PV clock regressed after block");
        last = now;
        let message = match result {
            Err(Error::Closed) if case == "peer-failure" => {
                println!("HERMIT_IPC_PEER_FAILURE_OK");
                break;
            }
            Err(Error::Malformed) if case == "malformed" => {
                println!("HERMIT_IPC_MALFORMED_OK");
                channel
                    .send(
                        &Message::new(Kind::Error, 0, b"malformed").unwrap(),
                        channel.deadline_after(1_000_000_000),
                    )
                    .unwrap();
                break;
            }
            result => result.expect("IPC request"),
        };
        assert_eq!(message.kind, Kind::Request);
        let request = Request::decode(message.payload()).expect("typed request schema");
        if request.operation == Operation::Mask {
            mask_notifications(true).unwrap();
        }
        let response = Response {
            value: request.value.wrapping_mul(2),
            timestamp_ns: channel.now_ns(),
        };
        channel
            .send_typed(message.id, &response, channel.deadline_after(2_000_000_000))
            .unwrap();
        count += 1;
        if request.operation == Operation::Mask {
            std::thread::sleep(std::time::Duration::from_millis(250));
            mask_notifications(false).unwrap();
            println!("HERMIT_IPC_MASK_OK");
        }
        if request.operation == Operation::Cancel {
            println!("HERMIT_IPC_CANCEL_OK");
        }
        if request.operation == Operation::Close {
            break;
        }
    }
    if case == "normal" {
        assert!(
            channel.full_waits() > 0,
            "response backpressure was not exercised"
        );
        println!("HERMIT_IPC_BACKPRESSURE_OK waits={}", channel.full_waits());
    }
    channel
        .close(channel.deadline_after(2_000_000_000))
        .expect("release both grants and event port");
    println!(
        "HERMIT_IPC_STATS requests={count} elapsed_ns={}",
        last.saturating_sub(start)
    );
    println!("HERMIT_IPC_PASS");
}
