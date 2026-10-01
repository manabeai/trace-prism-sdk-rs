# algo-vis Rust crate

`algo_vis::record!` sends ordinary Rust values to a local algo-vis server when one is running. No custom Array, Set, or Map wrapper is needed. The macro records a typed span ID path, an optional `from` reference, and any number of named values.

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

Add the crate to a local Cargo project with a path dependency:

```toml
[features]
default = ["viz"]
viz = ["dep:algo-vis"]

[dependencies]
algo-vis = { path = "/home/mana/programs/algo-visualizer/sdk/rust", optional = true }
```

For a source that is also submitted as a standalone AtCoder file, gate the import and provide a no-op fallback. The fallback does not evaluate visualization arguments, so recording expressions should not have algorithmic side effects.

```rust
#[cfg(feature = "viz")]
use algo_vis::record;
#[cfg(not(feature = "viz"))]
macro_rules! record { ($($tokens:tt)*) => { () }; }
```

Run `npm run build && npm run serve` in the algo-vis repository, then run the contest project with `cargo run --bin <name>`. Open `http://127.0.0.1:4317/`. Each run is saved separately; when the server is absent, the contest program continues without recording.

The crate emits `viz.trace/v2` snapshot/patch records. This is a local path dependency for the MVP; it has not been published to crates.io.
