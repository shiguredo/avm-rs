//! 任意バイト列をデコーダーに渡してもパニックしないことを検証する

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_avm::{Decoder, DecoderConfig};

fuzz_target!(|data: &[u8]| {
    let config = DecoderConfig::default();
    let Ok(mut decoder) = Decoder::new(config) else {
        return;
    };
    let _ = decoder.decode(data);
    let _ = decoder.finish();
    while decoder.next_frame().is_some() {}
});
