.PHONY: test cover fuzz fuzzing fuzzing-list check clippy fmt clean

# 全テストを実行する
test:
	cargo test

# 全テストカバレッジ付きで実行する
cover:
	cargo llvm-cov --tests

# Fuzzing を全ターゲットで 30 秒ずつ実行する（fuzz/ はワークスペース外）
fuzzing:
	@for target in $$(cd fuzz && cargo fuzz list); do \
		echo "=== Fuzzing $$target ==="; \
		cd fuzz && cargo +nightly fuzz run $$target -- -max_total_time=30 || exit 1; \
	done

# fuzzing のエイリアス
fuzz: fuzzing

# Fuzzing ターゲット一覧を表示する
fuzzing-list:
	cd fuzz && cargo fuzz list

# cargo check を実行する
check:
	cargo check

# cargo clippy を実行する
clippy:
	cargo clippy -- -D warnings

# cargo fmt を実行する
fmt:
	cargo fmt --all

# ビルド成果物を削除する
clean:
	cargo clean
