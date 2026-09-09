use std::{fmt, net::SocketAddr, sync::Arc};

use anyhow::Context;
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{info, warn};

const ADDR: &str = "127.0.0.1:9000";
const MAILBOX: usize = 128;

#[derive(Debug)]
enum Message {
    Joined(String),
    Left(String),
    Chat { sender: String, content: String },
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Message::Joined(who) => write!(f, "*** {who} 加入了聊天"),
            Message::Left(who) => write!(f, "*** {who} 离开了聊天"),
            Message::Chat { sender, content } => write!(f, "{sender}: {content}"),
        }
    }
}

/// 全服共享状态：谁在线，以及怎么给他投递消息
#[derive(Debug, Default)]
struct State {
    peers: DashMap<SocketAddr, mpsc::Sender<Arc<Message>>>,
}

impl State {
    /// 把消息投进每个人的信箱（发送者自己除外）
    fn broadcast(&self, from: SocketAddr, message: &Arc<Message>) {
        // 先把收件人抄出来再投递：DashMap 的迭代器持有分片锁，
        // 在迭代过程中 remove 同一张表会当场死锁。
        let recipients: Vec<_> = self
            .peers
            .iter()
            .filter(|entry| *entry.key() != from)
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();

        for (addr, mailbox) in recipients {
            // 用 try_send 而不是 send().await：广播里绝不能等，
            // 否则一个读得慢的客户端会把所有人的消息都堵住。
            match mailbox.try_send(Arc::clone(message)) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    warn!(%addr, "信箱已满，踢掉这个慢客户端");
                    self.peers.remove(&addr);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    self.peers.remove(&addr);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let listener = TcpListener::bind(ADDR)
        .await
        .with_context(|| format!("无法监听 {ADDR}"))?;
    info!(addr = ADDR, "聊天服务已启动，Ctrl-C 退出");

    let state = Arc::new(State::default());

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, addr) = accepted.context("accept 失败")?;
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(err) = handle(&state, stream, addr).await {
                          warn!(%addr, %err, "连接处理失败");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => {
                info!(online = state.peers.len(), "收到 Ctrl-C，停止接受新连接");
                break;
            }
        }
    }

    Ok(())
}

// user 先用 Empty 占位，报上名之后再 record 填进去
#[tracing::instrument(skip_all, fields(peer = %addr, user = tracing::field::Empty))]
async fn handle(state: &Arc<State>, stream: TcpStream, addr: SocketAddr) -> anyhow::Result<()> {
    let mut framed = Framed::new(stream, LinesCodec::new());
    framed.send("请输入昵称：").await?;

    let username = match framed.next().await {
        Some(Ok(name)) if !name.trim().is_empty() => name.trim().to_owned(),
        Some(Ok(_)) => {
            framed.send("昵称不能为空，再见").await?;
            return Ok(());
        }
        Some(Err(err)) => return Err(err).context("读取昵称失败"),
        None => return Ok(()),
    };
    tracing::Span::current().record("user", tracing::field::display(&username));

    // 拆成两半：读的一半留在本任务，写的一半交给一个专门的任务
    let (mut sink, mut incoming) = framed.split();
    let (mailbox_tx, mut mailbox_rx) = mpsc::channel::<Arc<Message>>(MAILBOX);
    state.peers.insert(addr, mailbox_tx);

    // 写任务：只管从自己的信箱取消息，写给这个客户端
    tokio::spawn(async move {
        while let Some(message) = mailbox_rx.recv().await {
            if sink.send(message.to_string()).await.is_err() {
                break;
            }
        }
    });

    info!(online = state.peers.len(), "加入");
    state.broadcast(addr, &Arc::new(Message::Joined(username.clone())));

    // 读循环：等这个用户打字
    while let Some(line) = incoming.next().await {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                warn!(%err, "读取失败，断开");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        state.broadcast(
            addr,
            &Arc::new(Message::Chat {
                sender: username.clone(),
                content: line,
            }),
        );
    }

    // 收尾：移除自己 → mailbox_tx 被 drop → 写任务的 recv 返回 None 自然退出
    state.peers.remove(&addr);
    info!(online = state.peers.len(), "离开");
    state.broadcast(addr, &Arc::new(Message::Left(username)));

    Ok(())
}
