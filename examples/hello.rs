//! Hello world against PokeAPI, no GUI.
//!
//!     cargo run --example hello -- pikachu

use pokepicker::api::Client;

fn main() -> anyhow::Result<()> {
    let name = std::env::args().nth(1).unwrap_or_else(|| "pikachu".into());
    let client = Client::new()?;

    let mon = client.pokemon(&name)?;
    let types: Vec<_> = mon.types.iter().map(|t| t.kind.name.as_str()).collect();
    println!("Hello, #{:03} {} [{}]", mon.id, mon.name, types.join("/"));

    let bytes = client.sprite(&mon)?;
    let path = format!("{}.png", mon.name);
    std::fs::write(&path, &bytes)?;
    println!("Saved sprite ({} bytes) to {path}", bytes.len());
    Ok(())
}
