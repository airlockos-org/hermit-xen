#[cfg(target_os = "hermit")]
use hermit as _;

fn main() {
    println!("HERMIT_APP_ENTER");
    let values: Vec<u64> = (1..=6).map(|x| x * 2).collect();
    assert_eq!(values.iter().sum::<u64>(), 42);
    println!("HERMIT_APP_ALLOC_OK");
    let answer = std::thread::spawn(move || values.into_iter().sum::<u64>())
        .join()
        .expect("Hermit worker thread panicked");
    assert_eq!(answer, 42);
    println!("HERMIT_APP_THREAD_OK");
    let started = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(25));
    assert!(started.elapsed() >= std::time::Duration::from_millis(25));
    println!("HERMIT_APP_TIMER_OK");
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(
        seconds > 1_577_836_800,
        "Xen wallclock must be later than 2020"
    );
    println!("HERMIT_APP_CLOCK_OK");
    if std::env::args().any(|arg| arg == "--panic") {
        panic!("intentional smoke-test failure");
    }
    if std::env::args().any(|arg| arg == "--hang") {
        println!("HERMIT_APP_INTENTIONAL_HANG");
        loop {
            std::hint::spin_loop();
        }
    }
    println!("HERMIT_SMOKE_PASS");
}
