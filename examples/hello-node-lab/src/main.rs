//! `hello-node-lab` standalone — the node graph lab in a window of its own.
//!
//! ★ R1724 — the screen itself is this package's **library** (`src/lib.rs`), so
//! the same code is both this binary and the page the analysis-tool shell
//! mounts at its Catalog destination. One source, two ways to reach it: the
//! demo sweep keeps a process per screen and the application keeps its
//! sections in one window.

fn main() {
    hello_node_lab::run();
}
