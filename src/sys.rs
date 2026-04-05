// bindgen が生成する FFI 定義は libavm の C API 命名規則に従うため、Rust の lint 命名規則と一致しない
#![expect(non_upper_case_globals)]
#![expect(non_camel_case_types)]
// macOS の bindgen 出力には Darwin 系ヘッダ由来の non_snake_case フィールドが含まれる
#![cfg_attr(target_os = "macos", expect(non_snake_case))]
#![expect(dead_code)]
// 生成コード側の import は crate 利用状況に依存する
#![expect(unused_imports)]

include!(concat!(env!("OUT_DIR"), "/metadata.rs"));
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
