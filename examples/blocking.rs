//! 实验：一个阻塞调用如何拖垮整个 runtime。

use std::time::Duration;

use tokio::time::Instant;

const NAP: Duration = Duration::from_millis(500);

fn main() {
    // 单线程 runtime：只有一个 worker，现象最明显
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("构建 runtime 失败");

    rt.block_on(async {
        let start = Instant::now();
        let (_a, _b, _c) = tokio::join!(
            tokio::spawn(nap_async()),
            tokio::spawn(nap_async()),
            tokio::spawn(nap_async()),
        );
        println!("异步 sleep × 3  ->  {:?}", start.elapsed());

        let start = Instant::now();
        let (_a, _b, _c) = tokio::join!(
            tokio::spawn(nap_blocking()),
            tokio::spawn(nap_blocking()),
            tokio::spawn(nap_blocking()),
        );
        println!("阻塞 sleep × 3  ->  {:?}", start.elapsed());
    });
}

/// 有 `.await`，睡的时候把 worker 让出去
async fn nap_async() {
    tokio::time::sleep(NAP).await;
}

/// 没有 `.await`，睡的时候死死占着 worker 不放
#[expect(
    clippy::unused_async,
    reason = "故意写成 async fn 来演示阻塞调用的危害"
)]
async fn nap_blocking() {
    std::thread::sleep(NAP);
}
