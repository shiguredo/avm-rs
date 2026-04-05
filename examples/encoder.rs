use shiguredo_avm::{EncodeOptions, Encoder, EncoderConfig, ImageData, ImageFormat};

fn main() {
    let width = 640u32;
    let height = 480u32;

    let config = EncoderConfig::new(width, height, ImageFormat::I420);

    let mut encoder = match Encoder::new(config) {
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
        force_keyframe: false,
    };

    if let Err(e) = encoder.encode(&image, &options) {
        eprintln!("failed to encode: {e}");
        std::process::exit(1);
    }

    while let Some(frame) = encoder.next_frame() {
        match frame.data() {
            Ok(data) => {
                println!(
                    "encoded frame: {} bytes, keyframe={}",
                    data.len(),
                    frame.is_keyframe()
                );
            }
            Err(e) => {
                eprintln!("failed to get encoded frame data: {e}");
            }
        }
    }

    if let Err(e) = encoder.finish() {
        eprintln!("failed to finish encoder: {e}");
        std::process::exit(1);
    }

    while let Some(frame) = encoder.next_frame() {
        match frame.data() {
            Ok(data) => {
                println!(
                    "flushed frame: {} bytes, keyframe={}",
                    data.len(),
                    frame.is_keyframe()
                );
            }
            Err(e) => {
                eprintln!("failed to get flushed frame data: {e}");
            }
        }
    }

    println!("encoding completed successfully");
}
