use shiguredo_avm::{
    Decoder, DecoderConfig, EncodeOptions, Encoder, EncoderConfig, ImageData, ImageFormat,
};
use std::time::Instant;

/// 8 ビット YUV フレーム
struct Frame8bit {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

/// 8 ビットデータと 16 ビットデータの PSNR を計算する
///
/// decoded は 16 ビット (リトルエンディアン) で格納されている。
/// 実際の値は 8 ビット (0-255) が 16 ビットコンテナに入っている。
fn calculate_psnr(
    original: &[u8],
    decoded: &[u8],
    width: usize,
    height: usize,
    stride: usize,
) -> Result<f64, &'static str> {
    let max_value = 255.0f64;
    let mut sse = 0.0f64;
    let mut count = 0usize;

    for row in 0..height {
        for col in 0..width {
            let orig_idx = row * width + col;
            let dec_idx = row * stride + col * 2;

            if orig_idx >= original.len() {
                return Err("元画像の画素参照が境界外です");
            }
            if dec_idx + 1 >= decoded.len() {
                return Err("デコード画像の画素参照が境界外です");
            }

            let dec_val = u16::from_le_bytes([decoded[dec_idx], decoded[dec_idx + 1]]) as u8;
            let diff = original[orig_idx] as f64 - dec_val as f64;
            sse += diff * diff;
            count += 1;
        }
    }

    if count == 0 {
        return Err("比較可能な画素がありません");
    }
    if sse == 0.0 {
        return Ok(100.0);
    }

    let mse = sse / count as f64;
    Ok(10.0 * (max_value * max_value / mse).log10())
}

/// デコード済みフレームのメタデータとプレーンを検証して取り出す
fn decode_frame_planes(
    frame: &shiguredo_avm::DecodedFrame<'_>,
    width: usize,
    height: usize,
) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize, usize, usize) {
    let format = frame
        .format()
        .expect("フレームフォーマットの取得に失敗しました");
    assert_eq!(frame.width(), width, "フレーム幅が一致しません");
    assert_eq!(frame.height(), height, "フレーム高さが一致しません");
    assert_eq!(
        format,
        shiguredo_avm::ImageFormat::I42016,
        "フレームフォーマットが I42016 ではありません"
    );

    let y_data = frame
        .y_plane()
        .expect("Y プレーンの取得に失敗しました")
        .to_vec();
    let u_data = frame
        .u_plane()
        .expect("U プレーンの取得に失敗しました")
        .to_vec();
    let v_data = frame
        .v_plane()
        .expect("V プレーンの取得に失敗しました")
        .to_vec();
    let y_stride = frame.y_stride().expect("Y stride の取得に失敗しました");
    let u_stride = frame.u_stride().expect("U stride の取得に失敗しました");
    let v_stride = frame.v_stride().expect("V stride の取得に失敗しました");

    (y_data, u_data, v_data, y_stride, u_stride, v_stride)
}

#[test]
fn encode_decode_psnr() {
    let width = 160usize;
    let height = 120usize;
    let fps = 15u32;
    let duration_secs = 1u32;
    let total_frames = fps * duration_secs;
    let target_bitrate = 100u32; // kbps

    println!("--- encode settings ---");
    println!("resolution: {width}x{height}");
    println!("fps: {fps}");
    println!("duration: {duration_secs}s");
    println!("frames: {total_frames}");
    println!("target bitrate: {target_bitrate} kbps");

    let mut config = EncoderConfig::new(width as u32, height as u32, ImageFormat::I420);
    // 品質テストに必要十分な速度設定（libavm デフォルト cpu_used=0 は遅すぎる）
    config.g_lag_in_frames = Some(0);
    config.rc_target_bitrate = target_bitrate;
    config.cpu_used = Some(8);
    config.g_threads = Some(4);
    config.enable_tpl_model = Some(false);
    let mut encoder = Encoder::new(config).expect("エンコーダーの作成に失敗しました");

    let y_size = width * height;
    let uv_width = width / 2;
    let uv_height = height / 2;
    let uv_size = uv_width * uv_height;

    let mut original_frames: Vec<Frame8bit> = Vec::new();
    let mut encoded_data = Vec::new();
    let mut encoded_frame_count = 0u32;

    let encode_start = Instant::now();

    for frame_idx in 0..total_frames {
        let y_plane: Vec<u8> = (0..y_size)
            .map(|i| {
                let x = i % width;
                let y = i / width;
                let base = ((x + y + frame_idx as usize) % 256) as u8;
                let noise = ((i * 7 + frame_idx as usize * 13) % 32) as u8;
                base.wrapping_add(noise)
            })
            .collect();
        let u_plane: Vec<u8> = (0..uv_size)
            .map(|i| {
                let x = i % uv_width;
                let base = ((x + frame_idx as usize) % 256) as u8;
                let noise = ((i * 11 + frame_idx as usize * 17) % 16) as u8;
                base.wrapping_add(noise)
            })
            .collect();
        let v_plane: Vec<u8> = (0..uv_size)
            .map(|i| {
                let y = i / uv_width;
                let base = ((y + frame_idx as usize) % 256) as u8;
                let noise = ((i * 13 + frame_idx as usize * 19) % 16) as u8;
                base.wrapping_add(noise)
            })
            .collect();

        original_frames.push(Frame8bit {
            y: y_plane.clone(),
            u: u_plane.clone(),
            v: v_plane.clone(),
        });

        let image = ImageData::I420 {
            y: &y_plane,
            u: &u_plane,
            v: &v_plane,
        };

        let options = EncodeOptions {
            force_keyframe: frame_idx == 0,
        };

        encoder
            .encode(&image, &options)
            .unwrap_or_else(|e| panic!("フレーム {frame_idx} のエンコードに失敗しました: {e}"));

        while let Some(frame) = encoder.next_frame() {
            let data = frame
                .data()
                .expect("エンコード済みフレームデータの取得に失敗しました");
            encoded_data.extend_from_slice(data);
            encoded_frame_count += 1;
        }
    }

    encoder
        .finish()
        .expect("エンコーダーのフラッシュに失敗しました");
    while let Some(frame) = encoder.next_frame() {
        let data = frame
            .data()
            .expect("フラッシュ後のエンコード済みフレームデータの取得に失敗しました");
        encoded_data.extend_from_slice(data);
        encoded_frame_count += 1;
    }

    let encode_elapsed = encode_start.elapsed();

    assert!(!encoded_data.is_empty(), "エンコード結果が空です");
    assert_eq!(
        encoded_frame_count, total_frames,
        "エンコードフレーム数が一致しません: 期待={total_frames}, 実際={encoded_frame_count}"
    );

    let encoded_bytes = encoded_data.len();
    let actual_bitrate = (encoded_bytes as f64 * 8.0) / (duration_secs as f64 * 1000.0);

    println!("--- encode result ---");
    println!("encoded frames: {encoded_frame_count}");
    println!("encoded bytes: {encoded_bytes}");
    println!("actual bitrate: {actual_bitrate:.1} kbps");
    println!("encode time: {:.2}s", encode_elapsed.as_secs_f64());
    println!(
        "encode fps: {:.1}",
        encoded_frame_count as f64 / encode_elapsed.as_secs_f64()
    );

    let dec_config = DecoderConfig {
        threads: Some(1),
        w: Some(width as u32),
        h: Some(height as u32),
    };
    let mut decoder = Decoder::new(dec_config).expect("デコーダーの作成に失敗しました");

    let decode_start = Instant::now();

    decoder
        .decode(&encoded_data)
        .expect("エンコードデータのデコードに失敗しました");

    let mut decoded_frames = Vec::new();
    while let Some(frame) = decoder.next_frame() {
        decoded_frames.push(decode_frame_planes(&frame, width, height));
    }

    decoder
        .finish()
        .expect("デコーダーのフラッシュに失敗しました");
    while let Some(frame) = decoder.next_frame() {
        decoded_frames.push(decode_frame_planes(&frame, width, height));
    }

    let decode_elapsed = decode_start.elapsed();

    println!("--- decode result ---");
    println!("decoded frames: {}", decoded_frames.len());
    println!("decode time: {:.2}s", decode_elapsed.as_secs_f64());
    println!(
        "decode fps: {:.1}",
        decoded_frames.len() as f64 / decode_elapsed.as_secs_f64()
    );

    assert_eq!(
        original_frames.len(),
        decoded_frames.len(),
        "フレーム数が一致しません: 元={} デコード={}",
        original_frames.len(),
        decoded_frames.len()
    );

    let mut total_psnr_y = 0.0f64;
    let mut total_psnr_u = 0.0f64;
    let mut total_psnr_v = 0.0f64;
    let mut min_psnr_y = f64::MAX;
    let mut max_psnr_y = 0.0f64;

    for (orig, dec) in original_frames.iter().zip(decoded_frames.iter()) {
        let (dec_y, dec_u, dec_v, y_stride, u_stride, v_stride) = dec;

        let psnr_y = calculate_psnr(&orig.y, dec_y, width, height, *y_stride)
            .expect("Y プレーンの PSNR 計算に失敗しました");
        let psnr_u = calculate_psnr(&orig.u, dec_u, uv_width, uv_height, *u_stride)
            .expect("U プレーンの PSNR 計算に失敗しました");
        let psnr_v = calculate_psnr(&orig.v, dec_v, uv_width, uv_height, *v_stride)
            .expect("V プレーンの PSNR 計算に失敗しました");

        total_psnr_y += psnr_y;
        total_psnr_u += psnr_u;
        total_psnr_v += psnr_v;

        if psnr_y < min_psnr_y {
            min_psnr_y = psnr_y;
        }
        if psnr_y > max_psnr_y {
            max_psnr_y = psnr_y;
        }
    }

    let frame_count = original_frames.len() as f64;
    let avg_psnr_y = total_psnr_y / frame_count;
    let avg_psnr_u = total_psnr_u / frame_count;
    let avg_psnr_v = total_psnr_v / frame_count;

    println!("--- psnr ---");
    println!("avg PSNR Y: {avg_psnr_y:.2} dB");
    println!("avg PSNR U: {avg_psnr_u:.2} dB");
    println!("avg PSNR V: {avg_psnr_v:.2} dB");
    println!("min PSNR Y: {min_psnr_y:.2} dB");
    println!("max PSNR Y: {max_psnr_y:.2} dB");

    assert!(
        avg_psnr_y > 20.0,
        "Y プレーンの平均 PSNR が低すぎます: {avg_psnr_y:.2} dB"
    );
    assert!(
        avg_psnr_u > 20.0,
        "U プレーンの平均 PSNR が低すぎます: {avg_psnr_u:.2} dB"
    );
    assert!(
        avg_psnr_v > 20.0,
        "V プレーンの平均 PSNR が低すぎます: {avg_psnr_v:.2} dB"
    );
}

#[test]
fn calculate_psnr_returns_error_when_no_samples() {
    // 幅 0 のため比較ループが回らず count が 0 のままになる
    let result = calculate_psnr(&[0], &[0], 0, 10, 20);
    assert_eq!(result, Err("比較可能な画素がありません"));
}
