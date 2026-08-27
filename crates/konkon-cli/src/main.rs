fn main() -> anyhow::Result<()> {
    println!("konkon {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
