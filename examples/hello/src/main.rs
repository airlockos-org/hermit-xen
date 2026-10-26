#[cfg(target_os = "hermit")]
use hermit as _;

fn main() {
    let values: Vec<u64> = (1..=6).collect();
    assert_eq!(values.iter().sum::<u64>(), 21);
    println!("HELLO_HERMIT_XEN_PASS");
}
