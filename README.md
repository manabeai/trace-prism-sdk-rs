# TracePrism Rust crate

`traceprism::record!` は通常の Rust 値を型付きの観測フレームとして記録する。`0.1.0-preview.2` では実行ごとのNDJSONファイルへ直接書き込むため、受信サーバーは不要。旧版 `0.1.0-preview.1` はHTTP送信を使用する。

```toml
[features]
default = ["viz"]
viz = ["dep:traceprism"]

[dependencies]
traceprism = { version = "=0.1.0-preview.2", optional = true }
```

```rust
#[cfg(feature = "viz")]
use traceprism::record;
#[cfg(not(feature = "viz"))]
macro_rules! record { ($($tokens:tt)*) => { () }; }

let adjacency = vec![vec![1, 2], vec![], vec![]];
let mut seen = vec![false; 3];
let origin = record!([0], adjacency, seen, v = 0);
for &v in &adjacency[0] {
    seen[v] = true;
    record!([v], from: origin, adjacency, seen, v);
}
```

`record!` の第1引数はspan IDの配列。`from:` には先行フレームの `FrameRef` または親のspan IDを渡せる。配列・整数・Set・Mapなどの通常の値をそのまま記録する。featureを外した提出用ビルドでは、空マクロが可視化用引数を評価しない。

保存先は次のとおり。`TRACEPRISM_RUN_DIR` に絶対パスを指定すると保存ディレクトリを変更できる。`VIZ_TRACE_PATH` は個別のファイル出力先、`VIZ_RUN_ID` は実行IDを指定する。

| OS | 既定の保存先 |
| --- | --- |
| Linux | `$XDG_DATA_HOME/traceprism/runs`、未設定なら `~/.local/share/traceprism/runs` |
| macOS | `~/Library/Application Support/traceprism/runs` |
| Windows | `%APPDATA%\traceprism\runs`、未設定なら `%LOCALAPPDATA%\traceprism\runs` |

各ファイルは `viz.trace/v2` のsnapshotとpatchを改行区切りで保持する。ファイルを開けない、または書き込みに失敗した場合は標準エラーへ通知して記録を停止し、プログラム本体の実行を続ける。標準出力は変更しない。

ビューワの起動方法は[リポジトリのREADME](https://github.com/manabeai/algo-vis/tree/feat/tauri-file-transport#tauriデスクトップ版開発ブランチ)を参照。
