//! 实验：async 世界 ↔ 专用工作线程，用 channel 搭桥。

use std::{thread, time::Duration};

use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};

/// 一个任务：要处理的数据，外加一个把结果送回去的「回邮信封」。
struct Job {
    input: u64,
    reply: oneshot::Sender<u64>,
}

/// 同步的重活。放在专用线程里，随便阻塞。
fn heavy(n: u64) -> u64 {
    thread::sleep(Duration::from_millis(300));
    n * n
}

/// 专用工作线程：不在 runtime 里，可以放心阻塞
fn spawn_worker(mut rx: mpsc::Receiver<Job>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        // blocking_recv：在同步线程里等一个 async channel
        while let Some(job) = rx.blocking_recv() {
            let result = heavy(job.input);
            // 对方可能已经不等了（超时、取消），发不出去就算了
            let _unused = job.reply.send(result);
        }
        println!("worker: 所有 sender 都没了，收工");
    })
}

#[tokio::main]
async fn main() {
    let (tx, rx) = mpsc::channel::<Job>(2);
    let worker = spawn_worker(rx);
    let start = Instant::now();

    let mut pending = Vec::new();
    for input in 1..=5 {
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(Job {
            input,
            reply: reply_tx,
        })
        .await
        .expect("worker 已退出");
        println!("已提交 {input}     @ {:>4}ms", start.elapsed().as_millis());
        pending.push((input, reply_rx));
    }

    drop(tx);

    for (input, reply_rx) in pending {
        let result = reply_rx.await.expect("worker 没有回复");
        println!(
            "{input} 的结果 = {result:>2} @ {:>4}ms",
            start.elapsed().as_millis()
        );
    }
    worker.join().expect("worker 线程 panic 了");
}
