//! 实验：TCP 没有消息边界，靠 `Decoder` 分帧。
//!
//! 协议：2 字节大端长度前缀 + 该长度的 UTF-8 内容

use std::io::ErrorKind::InvalidInput;

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

struct LenPrefixed;

impl Decoder for LenPrefixed {
    type Error = std::io::Error;
    type Item = String;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 2 {
            return Ok(None);
        }

        let len = usize::from(u16::from_be_bytes([src[0], src[1]]));

        if src.len() < 2 + len {
            src.reserve(2 + len - src.len());
            return Ok(None);
        }

        src.advance(2);
        let frame = src.split_to(len);

        String::from_utf8(frame.to_vec())
            .map(Some)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    }
}

impl Encoder<String> for LenPrefixed {
    type Error = std::io::Error;

    fn encode(&mut self, item: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let len = u16::try_from(item.len()).map_err(|_| {
            std::io::Error::new(
                InvalidInput,
                format!("消息太长：{} 字节，协议上限 {}", item.len(), u16::MAX),
            )
        })?;
        dst.reserve(2 + item.len());
        dst.put_u16(len);
        dst.put_slice(item.as_bytes());
        Ok(())
    }
}

fn main() {
    let mut codec = LenPrefixed;
    let mut wire = BytesMut::new();
    for msg in ["hi", "hello", "world!", "dfaffdasfasd"] {
        codec
            .encode(msg.to_owned(), &mut wire)
            .expect("encode failed");
    }
    let wire = wire.freeze();
    println!("发送端产出：{} 字节 {wire:?}\n", wire.len());

    let mut buf = BytesMut::new();
    let mut all = Vec::new();

    for (i, chunk) in wire.chunks(3).enumerate() {
        buf.put_slice(chunk);
        print!(
            "第 {} 次读到 {} 字节，缓冲区共 {:>2} 字节 -> ",
            i + 1,
            chunk.len(),
            buf.len()
        );
        let mut decoded = Vec::new();
        while let Some(msg) = codec.decode(&mut buf).expect("解码失败") {
            decoded.push(msg);
        }

        if decoded.is_empty() {
            println!("数据不够，等下一批");
        } else {
            println!("解出 {decoded:?}");
        }
        all.extend(decoded);
    }

    assert_eq!(
        all,
        ["hi", "hello", "world!", "dfaffdasfasd"],
        "往返之后应该拿回原样"
    );
    println!("\n往返成功：{all:?}");
}
