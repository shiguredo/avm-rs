use shiguredo_avm::{
    Decoder, DecoderConfig, EncodeOptions, Encoder, EncoderConfig, ImageData, ImageFormat,
};

fn main() {
    let width = 640u32;
    let height = 480u32;

    // エンコーダーでフレームをエンコードする
    let enc_config = EncoderConfig::new(width, height, ImageFormat::I420);
    let mut encoder = match Encoder::new(enc_config) {
        Ok(encoder) => encoder,
        Err(e) => {
            eprintln!("failed to create encoder: {e}");
            std::process::exit(1);
        }
    };

    let y_size = (width * height) as usize;
    let uv_size = ((width / 2) * (height / 2)) as usize;

    let y_plane = vec![128u8; y_size];
    let u_plane = vec![128u8; uv_size];
    let v_plane = vec![128u8; uv_size];

    let image = ImageData::I420 {
        y: &y_plane,
        u: &u_plane,
        v: &v_plane,
    };

    let options = EncodeOptions {
        force_keyframe: true,
    };

    if let Err(e) = encoder.encode(&image, &options) {
        eprintln!("failed to encode: {e}");
        std::process::exit(1);
    }

    let mut compressed_data = Vec::new();
    while let Some(frame) = encoder.next_frame() {
        match frame.data() {
            Ok(data) => compressed_data.extend_from_slice(data),
            Err(e) => {
                eprintln!("failed to get encoded frame data: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Err(e) = encoder.finish() {
        eprintln!("failed to finish encoder: {e}");
        std::process::exit(1);
    }

    while let Some(frame) = encoder.next_frame() {
        match frame.data() {
            Ok(data) => compressed_data.extend_from_slice(data),
            Err(e) => {
                eprintln!("failed to get encoded frame data: {e}");
                std::process::exit(1);
            }
        }
    }

    println!("encoded data: {} bytes", compressed_data.len());

    // デコーダーでフレームをデコードする
    let dec_config = DecoderConfig::new();
    let mut decoder = match Decoder::new(dec_config) {
        Ok(decoder) => decoder,
        Err(e) => {
            eprintln!("failed to create decoder: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = decoder.decode(&compressed_data) {
        eprintln!("failed to decode: {e}");
        std::process::exit(1);
    }

    while let Some(frame) = decoder.next_frame() {
        match frame.format() {
            Ok(format) => {
                println!(
                    "decoded frame: {}x{}, format={:?}, high_depth={}",
                    frame.width(),
                    frame.height(),
                    format,
                    frame.is_high_depth()
                );
            }
            Err(e) => {
                eprintln!("failed to get frame format: {e}");
            }
        }
    }

    if let Err(e) = decoder.finish() {
        eprintln!("failed to finish decoder: {e}");
        std::process::exit(1);
    }

    while let Some(frame) = decoder.next_frame() {
        match frame.format() {
            Ok(format) => {
                println!(
                    "flushed frame: {}x{}, format={:?}, high_depth={}",
                    frame.width(),
                    frame.height(),
                    format,
                    frame.is_high_depth()
                );
            }
            Err(e) => {
                eprintln!("failed to get flushed frame format: {e}");
            }
        }
    }

    println!("decoding completed successfully");
}
