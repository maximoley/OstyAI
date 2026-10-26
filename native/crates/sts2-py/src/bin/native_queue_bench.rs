//! Standalone native benchmark; does not start a Python interpreter.
fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "results/native-queue".into());
    if let Err(error) = sts2_rs::native_queue_benchmark(std::path::Path::new(&root)) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
