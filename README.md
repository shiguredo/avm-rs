# avm-rs

[![crates.io](https://img.shields.io/crates/v/shiguredo_avm.svg)](https://crates.io/crates/shiguredo_avm)
[![docs.rs](https://docs.rs/shiguredo_avm/badge.svg)](https://docs.rs/shiguredo_avm)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![GitHub Actions](https://github.com/shiguredo/avm-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/shiguredo/avm-rs/actions/workflows/ci.yml)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/shiguredo)

## About Shiguredo's open source software

We will not respond to PRs or issues that have not been discussed on Discord. Also, Discord is only available in Japanese.

Please read <https://github.com/shiguredo/oss> before use.

## 時雨堂のオープンソースソフトウェアについて

利用前に <https://github.com/shiguredo/oss> をお読みください。

## 概要

> [!WARNING]
> AVM が realtime モード（`AVM_USAGE_REALTIME` 相当）を実装するまでは、本 crate のメンテナンスを積極的には行いません。
>
> AVM v1.0.0 時点の libavm は `AVM_USAGE_GOOD_QUALITY` のみをサポートしており、エンコード速度も realtime 用途には十分ではありません。

[AVM](https://github.com/AOMediaCodec/avm)（Alliance for Open Media Video Model）の AV2 コーデックを、C ライブラリ libavm 経由で Rust から使うためのバインディング crate です。

### 用語

| 用語 | 意味 |
|---|---|
| AVM | [AOMediaCodec/avm](https://github.com/AOMediaCodec/avm) プロジェクト、AV2 コーデック、リリースタグ（例: `v1.0.0`）、ソースツリー |
| libavm | AVM からビルドする C ライブラリ（`libavm.a` 等）。本 crate がリンクする FFI 実装 |

## 特徴

- AV2 エンコーダー / デコーダーの Rust API（`Encoder`, `Decoder`）
- コーデック対応情報の照会（`supported_codecs()`）
- 入力画像フォーマット: I420, YV12, I422, I444 および各 16-bit 版（I42016, I42216, I44416）
- `EncoderConfig` は libavm の `avm_codec_enc_cfg_t` および `avm_codec_control` に沿ったフィールド名
- レート制御（VBR / CBR / CQ / Q）、タイル、行マルチスレッド、ロスレス、CDEF / リストレーション等の制御パラメータ
- per-frame のキーフレーム強制（`EncodeOptions::force_keyframe`）
- シンボル書き換えによるリンク時の衝突回避（`shiguredo_avm_` プレフィックス）
- デフォルトは GitHub Releases の libavm prebuilt（`libavm-<target>.tar.gz`）を利用したビルド
- libavm prebuilt 取得失敗時は AVM ソースからの libavm ビルドへ自動フォールバック
- 明示的な AVM ソースからの libavm ビルド（`--features source-build`）

## 動作要件

libavm prebuilt または AVM ソースからの libavm ビルドが成功するプラットフォーム:

| OS | アーキテクチャ | libavm prebuilt 名の例 |
|---|---|---|
| Ubuntu 24.04 | x86_64 | `ubuntu-24.04_x86_64` |
| macOS 26 | arm64 | `macos_arm64` |

### AVM ソースから libavm をビルドする場合の追加要件

- Git
- C / C++ コンパイラ
- CMake
- NASM
- curl, tar（libavm prebuilt ダウンロード時）

```bash
# Ubuntu
sudo apt-get install -y build-essential cmake nasm git curl

# macOS
brew install nasm cmake
```

## インストール

`Cargo.toml` に追加します。

```toml
[dependencies]
shiguredo_avm = "2026.0.0"
```

## ビルド

### 通常ビルド（libavm prebuilt）

デフォルトでは、crate バージョンに対応する GitHub Releases から libavm prebuilt を取得します。

```bash
cargo build
```

libavm prebuilt が存在しない、またはダウンロードに失敗した場合は、ビルドスクリプトが自動的に AVM ソースから libavm をビルドします（初回は時間がかかります）。

### 最初から AVM ソースから libavm をビルドする

```bash
cargo build --features source-build
```

### docs.rs 向け

libavm をリンクしないドキュメント生成のみ:

```bash
DOCS_RS=1 cargo doc --no-deps
```

### 環境変数（ビルド時）

| 変数 | 説明 |
|---|---|
| `LIBAVM_TARGET` | libavm prebuilt のプラットフォーム名を明示する（例: `ubuntu-24.04_x86_64`）。未設定時はホスト OS から推定する |
| `CARGO_FEATURE_SOURCE_BUILD` | 有効時は常に AVM ソースから libavm をビルド（`--features source-build` と同等） |
| `DOCS_RS` | 設定時は FFI ビルドをスキップしスタブ定義のみ出力する |

## 使い方

### 基本的な流れ

エンコード・デコードともにプル型 API です。1 回の `encode` / `decode` のあと、`next_frame()` を `None` になるまで呼び出します。ストリーム終端では `finish()` のあと、再度 `next_frame()` で残りフレームを取り出します。

```text
Encoder::encode → while next_frame() { ... }
Encoder::finish → while next_frame() { ... }

Decoder::decode → while next_frame() { ... }
Decoder::finish → while next_frame() { ... }
```

### コーデック対応情報

```rust
use shiguredo_avm::supported_codecs;

let info = supported_codecs();
assert_eq!(info.codec, shiguredo_avm::VideoCodecType::Av2);

match &info.encoding.profiles {
    shiguredo_avm::EncodingProfiles::Av2(profiles) => {
        for profile in profiles {
            println!("supported profile: {:?}", profile);
        }
    }
    shiguredo_avm::EncodingProfiles::Unsupported => {}
}
```

`supported_codecs()` はソフトウェア実装のため `hardware_accelerated` は常に `false` です。

### エンコード

```rust
use shiguredo_avm::{
    AvmRational, EncodeOptions, Encoder, EncoderConfig,
    ImageData, ImageFormat, RateControlMode, Usage,
};

let mut config = EncoderConfig::new(
    1920,              // 幅
    1080,              // 高さ
    ImageFormat::I420, // 入力フォーマット
);

config.g_usage = Usage::GoodQuality; // AVM v1.0.0 時点の libavm で唯一サポートされる usage
config.rc_end_usage = RateControlMode::Cbr;
config.rc_target_bitrate = 4000; // kbps
config.g_timebase = AvmRational { num: 1, den: 30 };
config.cpu_used = Some(8);   // 0=最遅〜9=最速（品質とのトレードオフ）
config.g_threads = Some(4);
config.g_lag_in_frames = Some(0); // 低遅延向け

let mut encoder = Encoder::new(config)?;

let image = ImageData::I420 {
    y: &y_plane,
    u: &u_plane,
    v: &v_plane,
};

encoder.encode(&image, &EncodeOptions { force_keyframe: false })?;

while let Some(frame) = encoder.next_frame() {
    let data = frame.data()?;
    let key = frame.is_keyframe();
    // data をファイルやネットワークへ
}

encoder.finish()?;
while let Some(frame) = encoder.next_frame() {
    let data = frame.data()?;
    // フラッシュで出た残りフレーム
}
```

### デコード

```rust
use shiguredo_avm::{Decoder, DecoderConfig};

let config = DecoderConfig {
    threads: Some(4),
    w: Some(1920), // ヒント（任意）
    h: Some(1080),
};
let mut decoder = Decoder::new(config)?;

decoder.decode(&compressed_data)?;

while let Some(frame) = decoder.next_frame() {
    let format = frame.format()?;
    let width = frame.width();
    let height = frame.height();
    let y = frame.y_plane()?;
    let u = frame.u_plane()?;
    let v = frame.v_plane()?;
    let y_stride = frame.y_stride()?;
    // 高ビット深度出力の場合は I42016 等。is_high_depth() で判定可能
}

decoder.finish()?;
while let Some(frame) = decoder.next_frame() {
    // 残りフレーム
}
```

デコーダー出力のピクセルフォーマットはビットストリームに依存します。10-bit プロファイルなどでは `I42016` など 16-bit 格納になり、`is_high_depth()` が `true` になります。自動的に 8-bit へ変換はしません。

### 例

```bash
cargo run --example encoder
cargo run --example decoder
```

## API リファレンス（概要）

詳細なフィールド一覧は [docs.rs](https://docs.rs/shiguredo_avm) を参照してください。

### 主要な型

| 型 | 説明 |
|---|---|
| `Encoder` / `Decoder` | AV2 エンコーダー / デコーダー |
| `EncoderConfig` | エンコーダー初期化設定 |
| `DecoderConfig` | デコーダー初期化設定（`threads`, `w`, `h`） |
| `EncodeOptions` | フレーム単位のオプション（`force_keyframe`） |
| `ImageFormat` / `ImageData` | 入力画像の形式とプレーンデータ |
| `EncodedFrame` | エンコード済みパケット（`data()`, `is_keyframe()`） |
| `DecodedFrame` | デコード済み画像（各プレーン・stride・`format()`） |
| `Error` | libavm エラー（`Display` で英語メッセージ） |
| `CodecInfo` | `supported_codecs()` の戻り値 |

### `EncoderConfig::new` のデフォルト

`EncoderConfig::new(width, height, image_format)` は必須 3 引数のみ指定し、それ以外は libavm のデフォルトに近い値で初期化します。

- `g_usage`: `Usage::GoodQuality`
- `rc_end_usage`: `RateControlMode::Vbr`
- `rc_target_bitrate`: `2000`（kbps）
- `g_timebase`: `{ num: 1, den: 30 }`
- 各 `Option` フィールド: `None`（libavm デフォルトを使用）

### `Usage` と速度調整

AVM v1.0.0 時点の libavm が文書化している usage は `AVM_USAGE_GOOD_QUALITY` のみです。

| `Usage` | 挙動 |
|---|---|
| `GoodQuality` | サポートされる |
| `Realtime` | `Encoder::new` がエラーを返す |
| `AllIntra` | `Encoder::new` がエラーを返す |

エンコード速度の調整には `cpu_used`（`0` = 最遅・最高品質 〜 `9` = 最速）と `g_threads` を使います。`AVM_USAGE_GOOD_QUALITY` では `7`〜`9` は `6` と同等扱いになる点に注意してください（libavm 仕様）。

### 未対応の `EncoderConfig` フィールド

次のフィールドに `Some(...)` や `true` を指定すると、`Encoder::new` は暗黙無視せず `Error` を返します。

- `rc_2pass_vbr_bias_pct`
- `rc_superres_mode`, `rc_superres_denominator`, `rc_superres_kf_denominator`, `rc_superres_qthresh`, `rc_superres_kf_qthresh`
- `large_scale_tile`, `save_as_annexb`
- `enable_obmc`, `enable_filter_intra`, `loopfilter_control`, `enable_ab_partitions`, `enable_dual_filter`, `enable_superres`
- `enable_psnr`（`true` の場合）

### ビルドメタデータ

ビルドに使用した AVM のリポジトリとバージョン（libavm のソースタグ）:

```rust
println!("repository: {}", shiguredo_avm::BUILD_REPOSITORY);
println!("version: {}", shiguredo_avm::BUILD_VERSION);
```

## テスト

```bash
# メイン crate
cargo test

# 例のみ
cargo run --example encoder

# Fuzzing（任意。nightly + cargo-fuzz。ワークスペース外の fuzz/ で実行）
make fuzzing-list
make fuzzing
```

統合テスト `tests/encode_decode.rs` は小解像度のエンコード→デコード→PSNR 検証です。エンコードには `cpu_used` 等で速度を抑えた設定を使っています。

## AVM ライセンス

<https://github.com/AOMediaCodec/avm/blob/main/LICENSE>

```text
BSD 3-Clause Clear License The Clear BSD License

Copyright (c) 2021, Alliance for Open Media

All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted (subject to the limitations in the disclaimer below) provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in
the documentation and/or other materials provided with the distribution.

3. Neither the name of the Alliance for Open Media nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.


NO EXPRESS OR IMPLIED LICENSES TO ANY PARTY'S PATENT RIGHTS ARE GRANTED BY THIS LICENSE.
THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY
EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL
THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT
OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## ライセンス

Apache License 2.0

```text
Copyright 2026-2026, Shiguredo Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
