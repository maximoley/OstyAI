fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/public/maxim/Codex/spire-mentor-gpu-feeder-20261024".into());
    if let Err(e) = sts2_rs::native_gpu_feed_benchmark(std::path::Path::new(&root)) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
