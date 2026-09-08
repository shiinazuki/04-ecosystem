//! 实验：`spawn_blocking` 如何把重活挪出 worker 线程

use std::time::Duration;

use tokio::time::Instant;

const HEAVY: Duration = Duration::from_millis(800);
const TICK: Duration = Duration::from_millis(100);

/// 模拟一个同步的重活：压缩、哈希、调用同步
fn heavy_work() -> u64 {
    std::thread::sleep(HEAVY);
    42
}

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("构建 runtime 失败");

    rt.block_on(async {
        println!("=== A：直接在任务里调用阻塞函数 ===");
        run(false).await;
        println!("\n=== B：交给 spawn_blocking ===");

        run(true).await;
    });
}

async fn run(offload: bool) {
    let start = Instant::now();

    let heartbear = tokio::spawn(async move {
        for i in 1..=8 {
            tokio::time::sleep(TICK).await;
            println!("  心跳 {i} @ {:>4}ms", start.elapsed().as_millis());
        }
    });

    let value = if offload {
        tokio::task::spawn_blocking(heavy_work)
            .await
            .expect("阻塞任务 panic 了")
    } else {
        heavy_work()
    };
    println!(
        "  重活完成 = {value} @ {:>4}ms",
        start.elapsed().as_millis()
    );

    heartbear.await.expect("心跳任务 panic 了");
}
