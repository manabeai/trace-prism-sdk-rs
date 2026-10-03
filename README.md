# TracePrism Rust crate

`traceprism::record!` は通常の Rust 値を型付きの観測フレームとして送信する。専用の Array・Set・Map 型は不要。span ID の配列、任意の `from` 参照、任意個の値を受け取る。

```rust
use traceprism::record;

let adjacency = vec![vec![1, 2], vec![], vec![]];
let mut seen = vec![false; 3];
record!([0], adjacency, seen, v = 0);
for &v in &adjacency[0] {
    seen[v] = true;
    record!([v], from: 0, adjacency, seen, v);
}
```

`from: 0` は親のspan ID `[0]` を指す。SDKはこれを型付きの `fromId` として送る。`record!([i, j], from: [pi, pj], ...)` のようにIDのパス全体も指定できる。同じIDの記録が複数ある場合は、対象より前にある最新の記録へ結ぶ。従来の `let parent = record!(...); record!(..., from: parent, ...)` も使用でき、この場合の `FrameRef` は従来のseq参照 `from` として送る。

ローカルの Cargo プロジェクトには path dependency として追加する。

```toml
[features]
default = ["viz"]
viz = ["dep:traceprism"]

[dependencies]
traceprism = { path = "/home/mana/programs/algo-visualizer/sdk/rust", optional = true }
```

AtCoder に単体ファイルで提出するソースでは、feature がないときだけ空マクロを定義する。この場合、可視化用引数は評価されないため、記録式にアルゴリズム本体の副作用を含めないこと。

```rust
#[cfg(feature = "viz")]
use traceprism::record;
#[cfg(not(feature = "viz"))]
macro_rules! record { ($($tokens:tt)*) => { () }; }
```

ビューワと受信サーバーは言語共通の CLI で起動する。リポジトリで `npm install && npm run build` を済ませ、別のターミナルで `npm run serve`（`npm link` 済みなら `traceprism serve`）を実行する。その後、競プロコードを通常の `cargo run --bin <name>` で実行する。`record!` は起動済みサーバーへ送信し、`http://127.0.0.1:4317/` で履歴を確認できる。SDK はサーバーやブラウザを起動しない。サーバーがない場合は記録を無効化してプログラム本体を続行し、標準出力は変更しない。

`VIZ_PORT` で送信先ポート、`VIZ_TRACE_PATH` でファイル出力先、`VIZ_RUN_ID` で実行 ID を指定できる。crate は現在ローカル path dependency で、crates.io には未公開。
