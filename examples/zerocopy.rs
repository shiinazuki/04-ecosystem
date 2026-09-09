//! 实验：`Bytes` 的切片是零拷贝的 —— 用内存地址证明。

use bytes::{BufMut as _, Bytes, BytesMut};

fn main() {
    // ---------- Vec<u8>：每切一次就复制一次 ----------
    let source = b"hello world, goodbye world".to_vec();
    let head = source[..5].to_vec();
    let tail = source[13..].to_vec();
    println!("== Vec<u8> ==");
    println!("  source {:p}", source.as_ptr());
    println!("  head   {:p}  <- 另一块内存，数据被复制了", head.as_ptr());
    println!("  tail   {:p}  <- 又一块", tail.as_ptr());

    // ---------- Bytes：切片只是换个视图 ----------
    let mut buf = BytesMut::with_capacity(64);
    buf.put_slice(b"hello world, goodbye world");
    let all: Bytes = buf.freeze();
    let head = all.slice(..5);
    let tail = all.slice(13..);
    println!("\n== Bytes ==");
    println!("  all    {:p}", all.as_ptr());
    println!("  head   {:p}  <- 和 all 同一个地址", head.as_ptr());
    println!("  tail   {:p}  <- all + 13，同一块内存", tail.as_ptr());
    println!("  内容: head={head:?} tail={tail:?}");

    // ---------- BytesMut::split_to：从写缓冲里切走一帧 ----------
    let mut buf = BytesMut::with_capacity(64);
    buf.put_slice(b"frame-1|frame-2|");
    let base = buf.as_ptr();
    let first = buf.split_to(8);
    println!("\n== BytesMut::split_to ==");
    println!("  切走 {:?}  ptr {:p}（= 原起点）", first, first.as_ptr());
    println!("  剩下 {:?}  ptr {:p}（= 原起点 + 8）", buf, buf.as_ptr());
    println!("  原起点 {base:p}");
}
