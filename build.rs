use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

// 依存ライブラリの名前（git clone 先ディレクトリ名および Cargo.toml の external-dependencies キー）
const LIB_NAME: &str = "avm";
// rustc に渡すリンク名（Unix: libavm.a → -lavm）
const LINK_NAME: &str = "avm";

// シンボル書き換え用のプレフィックス
//
// prebuilt で配布する際、他のライブラリが同じ libavm シンボル (avm_codec_encode 等) を
// 使っていると衝突する。この定数のプレフィックスを付けることで回避する。
//
// 変換例:
//   avm_codec_encode → shiguredo_avm_codec_encode (avm_ を shiguredo_avm_ に置換)
//   av2_internal_func → shiguredo_avm_av2_internal_func (内部シンボルは単純にプレフィックス付与)
const SYMBOL_PREFIX: &str = "shiguredo_avm";

/// prebuilt 取得処理の失敗理由
#[derive(Debug)]
enum PrebuiltError {
    Download { url: String },
    ChecksumDownload { url: String },
    ChecksumMismatch { expected: String, actual: String },
    Extract,
    CopyLibrary { path: String },
    CopyBindings,
    ReadLibDir,
}

impl std::fmt::Display for PrebuiltError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Download { url } => {
                write!(f, "prebuilt download failed: step=download, url={url}")
            }
            Self::ChecksumDownload { url } => {
                write!(
                    f,
                    "prebuilt download failed: step=checksum_download, url={url}"
                )
            }
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "prebuilt download failed: step=checksum_verify, expected={expected}, actual={actual}"
                )
            }
            Self::Extract => {
                write!(f, "prebuilt download failed: step=extract")
            }
            Self::CopyLibrary { path } => {
                write!(
                    f,
                    "prebuilt download failed: step=copy_library, path={path}"
                )
            }
            Self::CopyBindings => {
                write!(f, "prebuilt download failed: step=copy_bindings")
            }
            Self::ReadLibDir => {
                write!(f, "prebuilt download failed: step=read_lib_dir")
            }
        }
    }
}

fn main() {
    // Cargo.toml か build.rs が更新されたら、依存ライブラリを再ビルドする
    println!("cargo::rerun-if-changed=Cargo.toml");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=CARGO_FEATURE_SOURCE_BUILD");
    println!("cargo::rerun-if-env-changed=LIBAVM_TARGET");

    // 各種変数やビルドディレクトリのセットアップ
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("infallible"));
    let output_metadata_path = out_dir.join("metadata.rs");
    let output_bindings_path = out_dir.join("bindings.rs");

    // 各種メタデータを書き込む
    let (git_url, version) = get_git_url_and_version();
    fs::write(
        output_metadata_path,
        format!(
            concat!(
                "pub const BUILD_METADATA_REPOSITORY: &str={:?};\n",
                "pub const BUILD_METADATA_VERSION: &str={:?};\n",
            ),
            git_url, version
        ),
    )
    .expect("failed to write metadata file");

    if env::var("DOCS_RS").is_ok() {
        // Docs.rs 向けのビルドでは git clone ができないので build.rs の処理はスキップして、
        // 代わりに、ドキュメント生成時に最低限必要な定義だけをダミーで出力している。
        //
        // See also: https://docs.rs/about/builds
        fs::write(
            output_bindings_path,
            concat!(
                "pub struct avm_codec_iface;",
                "pub struct avm_codec_cx_pkt;",
                "pub struct avm_codec_cx_pkt__bindgen_ty_1__bindgen_ty_1 {",
                "    pub buf: *mut std::ffi::c_void,",
                "    pub sz: usize,",
                "    pub flags: u32,",
                "}",
                "pub struct avm_codec_enc_cfg;",
                "pub struct avm_image;",
                "pub type avm_codec_iter_t = *const std::ffi::c_void;",
                "pub struct avm_codec_ctx;",
                "pub enum avm_codec_err_t {}",
                "pub const AVM_FRAME_IS_KEY: u32 = 1;",
            ),
        )
        .expect("write file error");
        return;
    }

    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() == Some("windows") {
        panic!("unsupported target: Windows is not supported");
    }

    let output_lib_dir = if should_use_prebuilt() {
        match download_prebuilt(&out_dir) {
            Ok(lib_dir) => lib_dir,
            Err(err) => {
                eprintln!("{err}");
                eprintln!("falling back to source build for libavm");
                build_from_source(&out_dir, &output_bindings_path)
            }
        }
    } else {
        build_from_source(&out_dir, &output_bindings_path)
    };

    println!("cargo::rustc-link-search={}", output_lib_dir.display());
    println!("cargo::rustc-link-lib=static={LINK_NAME}");

    // TfLite 依存ライブラリをリンクする
    link_tflite_deps(&output_lib_dir);

    // libavm は C++ の標準ライブラリに依存する場合がある
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        println!("cargo::rustc-link-lib=c++");
    } else if target_os == "linux" {
        println!("cargo::rustc-link-lib=stdc++");
        println!("cargo::rustc-link-lib=m");
        println!("cargo::rustc-link-lib=pthread");
    }
}

// source-build feature が有効でなければ prebuilt を使う
fn should_use_prebuilt() -> bool {
    if env::var("CARGO_FEATURE_SOURCE_BUILD").is_ok() {
        return false;
    }
    true
}

// prebuilt バイナリをダウンロードして展開する
fn download_prebuilt(out_dir: &Path) -> Result<PathBuf, PrebuiltError> {
    let target = get_target_platform();
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is not set");
    let base_url = format!(
        "https://github.com/shiguredo/avm-rs/releases/download/{}",
        version
    );
    let archive_name = format!("libavm-{}.tar.gz", target);
    let archive_url = format!("{}/{}", base_url, archive_name);
    let sha256_url = format!("{}/{}.sha256", base_url, archive_name);

    let archive_path = out_dir.join("prebuilt.tar.gz");
    let sha256_path = out_dir.join("prebuilt.sha256");
    let prebuilt_dir = out_dir.join("prebuilt");
    fs::create_dir_all(&prebuilt_dir).expect("failed to create prebuilt directory");

    // curl でアーカイブをダウンロード
    eprintln!("downloading prebuilt library: {archive_url}");
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&archive_path)
        .arg(&archive_url)
        .status()
        .expect("failed to execute curl; ensure curl is installed");
    if !status.success() {
        return Err(PrebuiltError::Download { url: archive_url });
    }

    // curl で SHA256 チェックサムをダウンロード
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&sha256_path)
        .arg(&sha256_url)
        .status()
        .expect("failed to execute curl; ensure curl is installed");
    if !status.success() {
        return Err(PrebuiltError::ChecksumDownload { url: sha256_url });
    }

    // SHA256 を検証
    verify_sha256(&archive_path, &sha256_path)?;

    // tar で展開
    let status = Command::new("tar")
        .args(["xzf"])
        .arg(&archive_path)
        .arg("-C")
        .arg(&prebuilt_dir)
        .status()
        .expect("failed to execute tar; ensure tar is installed");
    if !status.success() {
        return Err(PrebuiltError::Extract);
    }

    // ライブラリファイルを OUT_DIR/lib/ にコピー
    //
    // prebuilt アーカイブにはリリースビルド時にシンボル書き換え済みのライブラリと
    // #[link_name] 属性付きの bindings.rs が含まれているため、そのままコピーするだけでよい。
    //
    // TfLite 依存ライブラリもアーカイブに含まれているため、全てコピーする。
    let lib_dir = out_dir.join("lib");
    fs::create_dir_all(&lib_dir).expect("failed to create lib directory");
    let prebuilt_lib_dir = prebuilt_dir.join("lib");
    for entry in fs::read_dir(&prebuilt_lib_dir).map_err(|_| PrebuiltError::ReadLibDir)? {
        let entry = entry.expect("failed to read prebuilt lib directory entry");
        let src = entry.path();
        if src
            .extension()
            .is_some_and(|ext| ext == "a" || ext == "lib")
        {
            let dst = lib_dir.join(
                src.file_name()
                    .expect("prebuilt library path must have a file name"),
            );
            fs::copy(&src, &dst).map_err(|_| PrebuiltError::CopyLibrary {
                path: src.display().to_string(),
            })?;
        }
    }

    // bindings.rs を OUT_DIR/ にコピー
    fs::copy(
        prebuilt_dir.join("bindings.rs"),
        out_dir.join("bindings.rs"),
    )
    .map_err(|_| PrebuiltError::CopyBindings)?;

    Ok(lib_dir)
}

// SHA256 チェックサムを検証する
fn verify_sha256(file_path: &Path, sha256_path: &Path) -> Result<(), PrebuiltError> {
    let expected = fs::read_to_string(sha256_path)
        .expect("failed to read SHA256 checksum file")
        .split_whitespace()
        .next()
        .expect("SHA256 checksum file must not be empty")
        .to_lowercase();

    let actual = compute_sha256(file_path);
    if actual != expected {
        return Err(PrebuiltError::ChecksumMismatch { expected, actual });
    }
    eprintln!("SHA256 checksum verified: {actual}");
    Ok(())
}

// ファイルの SHA256 ハッシュを計算する
fn compute_sha256(path: &Path) -> String {
    let output = if cfg!(target_os = "macos") {
        // macOS: shasum を使用
        Command::new("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
            .expect("failed to execute shasum. Ensure shasum is installed")
    } else if cfg!(target_os = "windows") {
        // Windows: certutil を使用
        Command::new("certutil")
            .args(["-hashfile"])
            .arg(path)
            .arg("SHA256")
            .output()
            .expect("failed to execute certutil")
    } else {
        // Linux: sha256sum を使用
        Command::new("sha256sum")
            .arg(path)
            .output()
            .expect("failed to execute sha256sum. Ensure coreutils is installed")
    };

    if !output.status.success() {
        panic!("failed to compute SHA256 checksum");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if cfg!(target_os = "windows") {
        // certutil 出力形式:
        // SHA256 hash of <file>:
        // <hash>
        // CertUtil: -hashfile command completed successfully.
        stdout
            .lines()
            .nth(1)
            .expect("unexpected certutil output format")
            .trim()
            .to_lowercase()
    } else {
        // shasum / sha256sum 出力形式: <hash>  <filename>
        stdout
            .split_whitespace()
            .next()
            .expect("unexpected shasum/sha256sum output format")
            .to_lowercase()
    }
}

// ソースからビルドする
fn build_from_source(out_dir: &Path, output_bindings_path: &Path) -> PathBuf {
    let out_build_dir = out_dir.join("build/");
    let _ = fs::remove_dir_all(&out_build_dir);
    fs::create_dir(&out_build_dir).expect("failed to create build directory");

    // 依存ライブラリのリポジトリを取得する
    git_clone_external_lib(&out_build_dir);

    // CMAKE 環境変数を設定
    shiguredo_cmake::set_cmake_env();

    // CMake でソースからビルドする
    let src_dir = out_build_dir.join(LIB_NAME);
    let install_dir = shiguredo_cmake::Config::new(&src_dir)
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("ENABLE_TESTS", "OFF")
        .define("ENABLE_EXAMPLES", "OFF")
        .define("ENABLE_TOOLS", "OFF")
        .define("ENABLE_DOCS", "OFF")
        .profile("Release")
        .build();

    let input_header_dir = install_dir.join("include/avm/");
    let output_lib_dir = install_dir.join("lib/");

    // TfLite 依存ライブラリをインストールディレクトリにコピーする
    copy_tflite_deps(&out_build_dir.join(LIB_NAME), &output_lib_dir);

    // 静的ライブラリのシンボルを書き換える
    let callbacks = rewrite_symbols(&output_lib_dir, out_dir);

    // バインディングを生成する
    //
    // parse_callbacks にシンボル書き換え用の ParseCallbacks を渡すことで、
    // 生成されるバインディングに #[link_name = "書き換え後のシンボル名"] が自動付与される。
    bindgen::Builder::default()
        .header(input_header_dir.join("avm_codec.h").display().to_string())
        .header(input_header_dir.join("avm_encoder.h").display().to_string())
        .header(input_header_dir.join("avm_decoder.h").display().to_string())
        .header(input_header_dir.join("avmcx.h").display().to_string())
        .header(input_header_dir.join("avmdx.h").display().to_string())
        .clang_arg(format!("-I{}", install_dir.join("include").display()))
        .parse_callbacks(Box::new(callbacks))
        .generate()
        .expect("failed to generate bindings")
        .write_to_file(output_bindings_path)
        .expect("failed to write bindings");

    output_lib_dir
}

// --- シンボル書き換え ---
//
// 他のライブラリとのシンボル衝突を回避するため、静的ライブラリ内の全シンボルに
// プレフィックスを付与する仕組み。
//
// llvm-nm / llvm-objcopy は rustup の llvm-tools コンポーネントに含まれるものを使用する。
// rust-toolchain.toml に components = ["llvm-tools"] の記載が必要。
//
// プラットフォームごとのシンボル形式の違い:
//   - macOS (Mach-O): シンボル先頭に `_` が付く (例: _avm_codec_encode)
//   - Linux (ELF): 先頭 `_` なし (例: avm_codec_encode)
//   - Windows x64 (COFF): 先頭 `_` なし (例: avm_codec_encode)
//
// bindgen の generated_link_name_override は返した文字列に \u{1} プレフィックスを
// 自動付加する。\u{1} はコンパイラに「この名前をそのまま使え（マングリングするな）」と
// 指示するため、プラットフォーム固有のシンボル名（macOS なら _shiguredo_avm_codec_encode）を
// そのまま返す必要がある。

/// llvm-nm / llvm-objcopy のパスを保持する
struct LlvmTools {
    nm: PathBuf,
    objcopy: PathBuf,
}

/// objcopy 用と bindgen 用の 2 つのリネームマップを保持する
///
/// 2 つのマップが必要な理由:
///   - objcopy_map: ライブラリ内の実シンボル名を書き換えるため、プラットフォーム依存の名前を使う
///   - bindgen_map: Rust コードからリンクする際の名前を指定するため、C シンボル名をキーにする
struct SymbolRenameMaps {
    /// llvm-objcopy の --redefine-syms 用マップ
    ///
    /// キー: 元のシンボル名 (例: macOS なら _avm_codec_encode、Linux なら avm_codec_encode)
    /// 値: 書き換え後のシンボル名 (例: macOS なら _shiguredo_avm_codec_encode)
    objcopy_map: HashMap<String, String>,

    /// bindgen の #[link_name] 用マップ
    ///
    /// キー: C シンボル名 (プラットフォーム非依存、例: avm_codec_encode)
    /// 値: 書き換え後のシンボル名 (プラットフォーム依存、例: macOS なら _shiguredo_avm_codec_encode)
    ///
    /// bindgen は \u{1} プレフィックスを付加してマングリングを抑制するため、
    /// 値にはプラットフォーム固有のシンボル名を格納する必要がある。
    bindgen_map: HashMap<String, String>,
}

/// bindgen の ParseCallbacks 実装
///
/// バインディング生成時に、書き換え後のシンボル名を `#[link_name = "..."]` として付与する。
/// これにより lib.rs 側のコード変更なしでシンボル書き換えが透過的に動作する。
#[derive(Debug)]
struct SymbolLinkNameCallbacks {
    /// C シンボル名 → 書き換え後シンボル名のマップ
    rename_map: HashMap<String, String>,
}

impl bindgen::callbacks::ParseCallbacks for SymbolLinkNameCallbacks {
    /// bindgen がバインディングを生成する際に呼ばれるコールバック
    ///
    /// 戻り値が Some の場合、bindgen は #[link_name = "\u{1}<戻り値>"] を生成する。
    /// \u{1} プレフィックスによりコンパイラのシンボルマングリングが抑制されるため、
    /// 戻り値にはプラットフォーム固有のシンボル名を返す必要がある。
    fn generated_link_name_override(
        &self,
        item_info: bindgen::callbacks::ItemInfo<'_>,
    ) -> Option<String> {
        self.rename_map.get(item_info.name).cloned()
    }
}

/// 静的ライブラリのシンボルを書き換え、bindgen 用の ParseCallbacks を返す
///
/// 処理の流れ:
///   1. rustup の sysroot から llvm-nm / llvm-objcopy を探す
///   2. llvm-nm で静的ライブラリの定義済み外部シンボルを収集する
///   3. 収集したシンボルに対してリネームマップを生成する
///   4. マップファイルを書き出し、llvm-objcopy でライブラリ内のシンボルを書き換える
///   5. bindgen 用の ParseCallbacks を返す
fn rewrite_symbols(lib_dir: &Path, out_dir: &Path) -> SymbolLinkNameCallbacks {
    let tools = discover_llvm_tools();
    let lib_path = find_static_library(lib_dir);

    // macOS の Mach-O ではシンボル先頭に `_` が付くため、
    // プラットフォーム判定してリネームマップの生成時に考慮する
    let is_macos = env::var("CARGO_CFG_TARGET_OS")
        .map(|v| v == "macos")
        .unwrap_or(false);

    // シンボル名の変換ルール
    //
    // avm_ プレフィックスを持つシンボル (公開 API) は avm_ を SYMBOL_PREFIX_ に置換する。
    //   例: avm_codec_encode → shiguredo_avm_codec_encode
    //
    // それ以外のシンボル (av2_* 等の内部シンボル) は先頭に SYMBOL_PREFIX_ を付与する。
    //   例: av2_internal_func → shiguredo_avm_av2_internal_func
    let rename_symbol = |name: &str| -> Option<String> {
        if let Some(rest) = name.strip_prefix("avm_") {
            Some(format!("{SYMBOL_PREFIX}_{rest}"))
        } else {
            Some(format!("{SYMBOL_PREFIX}_{name}"))
        }
    };

    // 全定義済み外部シンボルを収集してリネームマップを生成する
    let symbols = collect_defined_external_symbols(&tools.nm, &lib_path);
    let maps = build_symbol_rename_maps(&symbols, is_macos, &rename_symbol);

    // マップファイルを書き出してシンボルを書き換える
    let map_file = out_dir.join("symbol_rename_map.txt");
    write_objcopy_rename_map(&maps.objcopy_map, &map_file);
    rewrite_archive_symbols(&tools.objcopy, &lib_path, &map_file);

    SymbolLinkNameCallbacks {
        rename_map: maps.bindgen_map,
    }
}

/// 静的ライブラリのパスを探す
///
/// ビルド結果は Unix 系では libavm.a、Windows では avm.lib として出力される。
fn find_static_library(lib_dir: &Path) -> PathBuf {
    let unix_path = lib_dir.join("libavm.a");
    if unix_path.exists() {
        return unix_path;
    }
    let win_path = lib_dir.join("avm.lib");
    if win_path.exists() {
        return win_path;
    }
    panic!("static library not found in {}", lib_dir.display());
}

/// rustc --print sysroot の結果を取得する
///
/// llvm-tools は rustup が管理する sysroot 配下にインストールされるため、
/// sysroot のパスを取得して llvm-nm / llvm-objcopy の探索に使用する。
fn get_rustc_sysroot() -> PathBuf {
    let output = Command::new("rustc")
        .arg("--print")
        .arg("sysroot")
        .output()
        .expect("failed to run rustc --print sysroot");
    if !output.status.success() {
        panic!("rustc --print sysroot failed");
    }
    PathBuf::from(
        String::from_utf8(output.stdout)
            .expect("invalid UTF-8")
            .trim(),
    )
}

/// Windows 対応の実行ファイル名を生成する
///
/// Windows では実行ファイルに .exe 拡張子が必要。
fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// rustup の sysroot から llvm-nm / llvm-objcopy を探す
///
/// llvm-tools コンポーネントのバイナリは以下のパスに配置される:
///   <sysroot>/lib/rustlib/<host>/bin/llvm-nm
///   <sysroot>/lib/rustlib/<host>/bin/llvm-objcopy
///
/// rust-toolchain.toml に llvm-tools コンポーネントの記載が必要。
///
/// llvm-nm / llvm-objcopy はホスト上で実行するツールなので、クロスコンパイル時は
/// TARGET ではなく HOST のパスから探す必要がある。
/// 例: Windows CI でホストが x86_64-pc-windows-msvc、ターゲットが x86_64-pc-windows-gnu の場合、
/// llvm-tools は msvc 側にのみインストールされている。
fn discover_llvm_tools() -> LlvmTools {
    let sysroot = get_rustc_sysroot();
    // llvm-tools はホスト上で動作するため HOST を使う。
    // クロスコンパイル時に TARGET を使うと、ホスト側にインストールされた
    // llvm-tools が見つからない。
    let host = env::var("HOST").expect("HOST environment variable not set");
    let tools_dir = sysroot.join("lib/rustlib").join(host).join("bin");

    let nm = tools_dir.join(exe_name("llvm-nm"));
    let objcopy = tools_dir.join(exe_name("llvm-objcopy"));

    if !nm.exists() {
        panic!(
            "llvm-nm not found at {}. Run: rustup component add llvm-tools",
            nm.display()
        );
    }
    if !objcopy.exists() {
        panic!(
            "llvm-objcopy not found at {}. Run: rustup component add llvm-tools",
            objcopy.display()
        );
    }

    LlvmTools { nm, objcopy }
}

/// llvm-nm で静的ライブラリから定義済み外部シンボルを収集する
///
/// llvm-nm のオプション:
///   --defined-only: 定義済みシンボルのみ (未定義シンボルを除外)
///   --extern-only: 外部シンボルのみ (ローカルシンボルを除外)
///   --format=just-symbols: シンボル名のみ出力 (アドレスやタイプを省略)
///
/// 出力にはオブジェクトファイル名 (例: av1_cx_iface.c.o:) も含まれるため、
/// is_c_identifier() でフィルタリングして純粋なシンボル名のみを抽出する。
fn collect_defined_external_symbols(nm_path: &Path, lib_path: &Path) -> Vec<String> {
    let output = Command::new(nm_path)
        .arg("--defined-only")
        .arg("--extern-only")
        .arg("--format=just-symbols")
        .arg(lib_path)
        .output()
        .expect("failed to run llvm-nm");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("llvm-nm failed: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout).expect("llvm-nm output is not valid UTF-8");
    let mut symbols: Vec<String> = stdout
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|s| !s.is_empty() && is_c_identifier(s))
        .collect();
    symbols.sort();
    symbols.dedup();
    symbols
}

/// C 識別子として有効かどうかを判定する
///
/// llvm-nm の --format=just-symbols 出力にはオブジェクトファイル名 (av1_cx_iface.c.o: 等) も
/// 含まれるため、この関数で C 識別子のみをフィルタリングする。
///
/// macOS の Mach-O ではシンボル先頭に `_` が付くため、`_` で始まる文字列も受け入れる。
///
/// libavm の NASM ソースは cglobal_label を使用しておらず、ドット付きの
/// extern シンボルは生成されないため、`.` は許可しない。
fn is_c_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// objcopy 用と bindgen 用のリネームマップを生成する
///
/// 2 つのマップを生成する理由:
///
/// objcopy_map: ライブラリバイナリ内の実シンボル名を書き換えるためのマップ。
///   macOS では _avm_codec_encode → _shiguredo_avm_codec_encode のようにプラットフォーム固有の
///   `_` プレフィックスを含む形で管理する。
///
/// bindgen_map: Rust バインディングの #[link_name] に使うマップ。
///   キーは C シンボル名 (avm_codec_encode)、値はプラットフォーム固有のシンボル名
///   (_shiguredo_avm_codec_encode) を格納する。
///   bindgen は generated_link_name_override の戻り値に \u{1} を付加してマングリングを
///   抑制するため、プラットフォーム固有の名前を直接返す必要がある。
fn build_symbol_rename_maps(
    symbols: &[String],
    is_macos: bool,
    rename_symbol: &dyn Fn(&str) -> Option<String>,
) -> SymbolRenameMaps {
    let mut objcopy_map = HashMap::new();
    let mut bindgen_map = HashMap::new();

    for sym in symbols {
        // プラットフォーム固有のプレフィックスを除去して C シンボル名を取得する
        //   macOS: _avm_codec_encode → avm_codec_encode
        //   Linux/Windows: avm_codec_encode → avm_codec_encode (変化なし)
        let c_name = if is_macos {
            sym.strip_prefix('_').unwrap_or(sym)
        } else {
            sym.as_str()
        };

        if let Some(new_c_name) = rename_symbol(c_name) {
            // objcopy 用: プラットフォーム固有のプレフィックスを再付与する
            //   macOS: shiguredo_avm_codec_encode → _shiguredo_avm_codec_encode
            //   Linux/Windows: shiguredo_avm_codec_encode → shiguredo_avm_codec_encode (変化なし)
            let new_sym = if is_macos {
                format!("_{new_c_name}")
            } else {
                new_c_name.clone()
            };
            objcopy_map.insert(sym.clone(), new_sym.clone());

            // bindgen 用: generated_link_name_override は \u{1} プレフィックスを付加して
            // シンボル名をそのまま使うため、プラットフォーム固有のシンボル名で管理する
            bindgen_map.insert(c_name.to_string(), new_sym);
        }
    }

    SymbolRenameMaps {
        objcopy_map,
        bindgen_map,
    }
}

/// --redefine-syms 用のマップファイルを書き出す
///
/// ファイル形式は 1 行に "旧シンボル名 新シンボル名" を空白区切りで記述する。
/// llvm-objcopy の --redefine-syms オプションで使用される。
fn write_objcopy_rename_map(map: &HashMap<String, String>, path: &Path) {
    let mut lines: Vec<String> = map
        .iter()
        .map(|(old, new)| format!("{old} {new}"))
        .collect();
    // 出力を決定的にするためソートする
    lines.sort();
    fs::write(path, lines.join("\n")).expect("failed to write symbol rename map");
}

/// llvm-objcopy でアーカイブ内のシンボルを書き換える
///
/// --redefine-syms はマップファイルに従ってシンボル名を一括置換する。
/// ライブラリファイルはインプレースで更新される。
fn rewrite_archive_symbols(objcopy_path: &Path, lib_path: &Path, map_file: &Path) {
    let status = Command::new(objcopy_path)
        .arg("--redefine-syms")
        .arg(map_file)
        .arg(lib_path)
        .status()
        .expect("failed to run llvm-objcopy");
    if !status.success() {
        panic!("llvm-objcopy failed");
    }
}

// TfLite 依存ライブラリをインストールディレクトリにコピーする
//
// AVM は TfLite を有効にすると、ビルドディレクトリに TfLite の依存ライブラリを生成するが、
// インストール時にこれらのライブラリを含めないため、明示的にコピーする必要がある。
fn copy_tflite_deps(avm_build_dir: &Path, output_lib_dir: &Path) {
    // avm_build_dir は <out_dir>/build/avm なので、ビルドディレクトリは <out_dir>/build
    let build_dir = avm_build_dir
        .parent()
        .expect("avm build directory must have a parent path");

    // TfLite 関連のライブラリを検索してコピーする
    let tflite_libs = ["libtensorflow-lite.a", "libtensorflowlite_c.a"];

    for lib_name in &tflite_libs {
        let src_path = find_library_recursive(build_dir, lib_name);
        if let Some(src_path) = src_path {
            let dst_path = output_lib_dir.join(lib_name);
            fs::copy(&src_path, &dst_path)
                .unwrap_or_else(|e| panic!("failed to copy {}: {}", src_path.display(), e));
        }
    }

    // TfLite の依存ライブラリを検索してコピーする
    let dep_libs = [
        "libXNNPACK.a",
        "libcpuinfo.a",
        "libpthreadpool.a",
        "libruy_*.a",
        "libflatbuffers.a",
        "libfarmhash.a",
        "libgemmlowp.a",
        "libfp16.a",
        "libml_dtypes.a",
        "libfft2d_*.a",
        "libabsl_*.a",
    ];

    for pattern in &dep_libs {
        let libs = find_libraries_by_pattern(build_dir, pattern);
        for src_path in libs {
            let lib_name = src_path
                .file_name()
                .expect("library path must have a file name")
                .to_str()
                .expect("library file name must be valid UTF-8");
            let dst_path = output_lib_dir.join(lib_name);
            fs::copy(&src_path, &dst_path)
                .unwrap_or_else(|e| panic!("failed to copy {}: {}", src_path.display(), e));
        }
    }
}

// ディレクトリを再帰的に検索してライブラリを見つける
fn find_library_recursive(dir: &Path, name: &str) -> Option<PathBuf> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = find_library_recursive(&path, name) {
                    return Some(found);
                }
            } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(path);
            }
        }
    }
    None
}

// パターンに一致するライブラリを検索する
fn find_libraries_by_pattern(dir: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir).ok().into_iter().flatten() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if path.is_dir() {
                result.extend(find_libraries_by_pattern(&path, pattern));
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && matches_pattern(name, pattern)
            {
                result.push(path);
            }
        }
    }
    result
}

// 簡易的なワイルドカードパターンマッチ
fn matches_pattern(name: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("*.a") {
        name.starts_with(prefix) && name.ends_with(".a")
    } else {
        name == pattern
    }
}

// TfLite 依存ライブラリをリンクする
//
// AVM は TfLite を有効にすると、ビルドディレクトリに TfLite の依存ライブラリを生成するが、
// インストール時にこれらのライブラリを含めないため、明示的にリンクする必要がある。
fn link_tflite_deps(output_lib_dir: &Path) {
    let tflite_libs = [
        "tensorflow-lite",
        "tensorflowlite_c",
        "XNNPACK",
        "cpuinfo",
        "pthreadpool",
        "flatbuffers",
        "farmhash",
        "gemmlowp",
        "fp16",
        "ml_dtypes",
    ];

    for lib_name in &tflite_libs {
        let lib_path = output_lib_dir.join(format!("lib{lib_name}.a"));
        if lib_path.exists() {
            println!("cargo::rustc-link-lib=static={lib_name}");
        }
    }

    // ruy と absl は複数のサブライブラリがあるため、パターンで検索する
    let ruy_libs = find_libraries_by_pattern(output_lib_dir, "libruy_*.a");
    for lib_path in ruy_libs {
        if let Some(name) = lib_path.file_stem().and_then(|n| n.to_str()) {
            // "libruy_*.a" -> "ruy_*"
            if let Some(lib_name) = name.strip_prefix("lib") {
                println!("cargo::rustc-link-lib=static={lib_name}");
            }
        }
    }

    let absl_libs = find_libraries_by_pattern(output_lib_dir, "libabsl_*.a");
    for lib_path in absl_libs {
        if let Some(name) = lib_path.file_stem().and_then(|n| n.to_str())
            && let Some(lib_name) = name.strip_prefix("lib")
        {
            println!("cargo::rustc-link-lib=static={lib_name}");
        }
    }

    let fft2d_libs = find_libraries_by_pattern(output_lib_dir, "libfft2d_*.a");
    for lib_path in fft2d_libs {
        if let Some(name) = lib_path.file_stem().and_then(|n| n.to_str())
            && let Some(lib_name) = name.strip_prefix("lib")
        {
            println!("cargo::rustc-link-lib=static={lib_name}");
        }
    }
}

// CARGO_CFG_TARGET_OS + CARGO_CFG_TARGET_ARCH からプラットフォーム名を生成する
fn get_target_platform() -> String {
    if let Ok(target) = env::var("LIBAVM_TARGET") {
        return target;
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    match (target_os.as_str(), target_arch.as_str()) {
        ("linux", "x86_64") => format!("{}_x86_64", detect_linux_distro()),
        ("linux", "aarch64") => {
            panic!(
                "unsupported target: Linux arm64 is not supported (os={}, arch={})",
                target_os, target_arch
            );
        }
        ("macos", "aarch64") => "macos_arm64".to_string(),
        ("windows", "x86_64") => {
            panic!(
                "unsupported target: Windows is not supported (os={}, arch={})",
                target_os, target_arch
            );
        }
        _ => panic!("unsupported target: os={}, arch={}", target_os, target_arch),
    }
}

// /etc/os-release から Ubuntu バージョンを検出する
fn detect_linux_distro() -> String {
    if let Ok(content) = fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(version) = line.strip_prefix("VERSION_ID=") {
                let version = version.trim_matches('"');
                if version == "24.04" {
                    return format!("ubuntu-{version}");
                }
            }
        }
    }
    panic!(
        "unsupported Linux distribution: only Ubuntu 24.04 is supported. \
         set LIBAVM_TARGET environment variable to specify the target explicitly"
    );
}

// 外部ライブラリのリポジトリを git clone する
fn git_clone_external_lib(build_dir: &Path) {
    let (git_url, version) = get_git_url_and_version();
    let success = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--branch")
        .arg(version)
        .arg(git_url)
        .current_dir(build_dir)
        .status()
        .is_ok_and(|status| status.success());
    if !success {
        panic!("failed to clone {LIB_NAME} repository");
    }
}

// Cargo.toml から依存ライブラリの Git URL とバージョンタグを取得する
fn get_git_url_and_version() -> (String, String) {
    let cargo_toml = shiguredo_toml::Value::Table(
        shiguredo_toml::from_str(include_str!("Cargo.toml")).expect("failed to parse Cargo.toml"),
    );
    if let Some((Some(git_url), Some(version))) = cargo_toml
        .get("package")
        .and_then(|v| v.get("metadata"))
        .and_then(|v| v.get("external-dependencies"))
        .and_then(|v| v.get(LIB_NAME))
        .map(|v| {
            (
                v.get("url").and_then(|s| s.as_str()),
                v.get("version").and_then(|s| s.as_str()),
            )
        })
    {
        (git_url.to_string(), version.to_string())
    } else {
        panic!(
            "Cargo.toml does not contain a valid [package.metadata.external-dependencies.{LIB_NAME}] table"
        );
    }
}
