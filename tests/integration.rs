//! 集成测试：以外部使用者的视角调用 crate 的公开 API。
//!
//! 它是独立 crate，只能访问 `src/lib.rs` 导出的 `pub` 项，`main.rs` 里的内容在这里
//! 访问不到。
