# algo-vis Rust crate

`algo_vis::record!` は通常の Rust 値を型付きの観測フレームとして送信する。専用の Array・Set・Map 型は不要。span ID の配列、任意の `from` 参照、任意個の値を受け取る。

```rust
use algo_vis::record;

let mut left = 0;
let right = a.len();
let root = record!([], a, left, right);
for i in 0..3 {
    left += 1;
    record!([i], from: root, a, left, right);
}
```

ローカルの Cargo プロジェクトには path dependency として追加する。

```toml
[features]
default = ["viz"]
viz = ["dep:algo-vis"]

[dependencies]
algo-vis = { path = "/home/mana/programs/algo-visualizer/sdk/rust", optional = true }
```

AtCoder に単体ファイルで提出するソースでは、feature がないときだけ空マクロを定義する。この場合、可視化用引数は評価されないため、記録式にアルゴリズム本体の副作用を含めないこと。

```rust
#[cfg(feature = "viz")]
use algo_vis::record;
#[cfg(not(feature = "viz"))]
macro_rules! record { ($($tokens:tt)*) => { () }; }
```

ビューワと受信サーバーは言語共通の CLI で起動する。リポジトリで `npm install && npm run build` を済ませ、別のターミナルで `npm run serve`（`npm link` 済みなら `algo-vis serve`）を実行する。その後、競プロコードを通常の `cargo run --bin <name>` で実行する。`record!` は起動済みサーバーへ送信し、`http://127.0.0.1:4317/` で履歴を確認できる。SDK はサーバーやブラウザを起動しない。サーバーがない場合は記録を無効化してプログラム本体を続行し、標準出力は変更しない。

`VIZ_PORT` で送信先ポート、`VIZ_TRACE_PATH` でファイル出力先、`VIZ_RUN_ID` で実行 ID を指定できる。crate は現在ローカル path dependency で、crates.io には未公開。
