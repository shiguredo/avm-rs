# 変更履歴

- CHANGE
  - 後方互換のない変更
- ADD
  - 後方互換がある追加
- UPDATE
  - 後方互換がある変更
- FIX
  - バグ修正

## 2026.0.0

**リリース日**: 2026-06-01

- [ADD] AVM v1.0.0（libavm）向け AV2 エンコーダー / デコーダーの Rust バインディング crate `shiguredo_avm` を公開する
  - @voluntas
- [ADD] GitHub Releases の libavm prebuilt 取得と AVM ソースからの libavm ビルドフォールバックを build.rs に実装する
  - @voluntas
- [ADD] `Encoder` / `Decoder`、`EncoderConfig`、`supported_codecs()`、I420 / I422 / I444 および 16-bit 入力フォーマットを公開 API として提供する
  - @voluntas
- [ADD] encoder / decoder の example、統合テスト、PBT（proptest）、fuzzing 基盤を追加する
  - @voluntas
- [UPDATE] README を shiguredo_avm / AV2 / AVM・libavm 用語に統一し、メンテナンス方針を記載する
  - @voluntas
- [FIX] FFI プレーンポインタ NULL 入力時にパニックせず `Error` を返す
  - @voluntas
- [FIX] 未対応 `EncoderConfig` フィールドと `Usage` を暗黙無視せず `Error` を返す
  - @voluntas
- [FIX] libavm prebuilt 取得失敗時に AVM ソースからの libavm ビルドへ自動フォールバックする
  - @voluntas
- [FIX] PSNR 統合テストの検証抜けと境界エラーパスを塞ぐ
  - @voluntas

### misc

- [ADD] CI / release workflow、canary スクリプト、Makefile を整備する
  - @voluntas
