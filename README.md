# TracePrism Rust crate

このブランチでは記録先を実行ごとのNDJSONファイルに変更した。公開済みの `0.1.0-preview.1` は旧HTTP版で、この変更はまだ含まれない。ファイル連携を試す場合はCargo依存をこのブランチの `sdk/rust` への `path` に変更する。通常の `cargo run` で記録でき、受信サーバーは不要。I/Oに失敗しても標準エラーへ通知して本体の実行を続ける。保存先はルートREADMEのOS別表を参照。`TRACEPRISM_RUN_DIR` は絶対パスの保存ディレクトリ、`VIZ_TRACE_PATH` は個別ファイル、`VIZ_RUN_ID` は実行IDを指定する。

以下は公開済み旧HTTP版の説明である。

`traceprism::record!` は通常の Rust 値を型付きの観測フレームとして送信する。専用の Array・Set・Map 型は不要。span ID の配列、任意の `from` 参照、任意個の値を受け取る。

現在の公開版は `0.1.0-preview.1`。API と送信形式はプレリリース中に変更される可能性がある。

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

Cargo プロジェクトには次のように追加する。リポジトリ内で開発する場合は `version` の代わりに `path` を指定できる。

```toml
[features]
default = ["viz"]
viz = ["dep:traceprism"]

[dependencies]
traceprism = { version = "=0.1.0-preview.1", optional = true }
```

AtCoder に単体ファイルで提出するソースでは、feature がないときだけ空マクロを定義する。この場合、可視化用引数は評価されないため、記録式にアルゴリズム本体の副作用を含めないこと。

```rust
#[cfg(feature = "viz")]
use traceprism::record;
#[cfg(not(feature = "viz"))]
macro_rules! record { ($($tokens:tt)*) => { () }; }
```

ビューワと受信サーバーは言語共通の CLI で起動する。`npm install --global traceprism@preview` の後に `traceprism serve` を実行する。リポジトリでは `npm install && npm run build && npm run serve` でも起動できる。その後、競プロコードを通常の `cargo run --bin <name>` で実行する。`record!` は起動済みサーバーへ送信し、`http://127.0.0.1:4317/` で履歴を確認できる。SDK はサーバーやブラウザを起動しない。サーバーがない場合は記録を無効化してプログラム本体を続行し、標準出力は変更しない。

`VIZ_PORT` で送信先ポート、`VIZ_TRACE_PATH` でファイル出力先、`VIZ_RUN_ID` で実行 ID を指定できる。
