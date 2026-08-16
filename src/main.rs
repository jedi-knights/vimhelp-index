// Entry point. All real work lives in the library (src/lib.rs) so tests
// can call it without depending on the binary target.

fn main() -> anyhow::Result<()> {
    vimhelp_index::cli::run()
}
