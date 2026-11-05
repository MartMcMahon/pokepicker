//! Package the colour index and every sprite for the web build.
//!
//!     cargo run --release --bin bundle [out_dir]
//!
//! Writes `index.json`, `sprites.json` and `sprites.bin` to `out_dir`
//! (default `web-data/`). Uses the desktop sprite cache, building it first if
//! needed, so it only downloads what's missing.

use std::path::PathBuf;

use anyhow::Context;
use pokepicker::api::Client;
use pokepicker::index::{Index, Progress, Store};
use pokepicker::sprite_pack::SpritePack;

fn main() -> anyhow::Result<()> {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "web-data".into()));
    std::fs::create_dir_all(&out)?;

    let store = Store::default_location()?;
    let index = match Index::load(&store)? {
        Some(index) => index,
        None => {
            eprintln!("Building colour index first...");
            Index::build(&store, &Client::new()?, &Progress::default())?.index
        }
    };

    let mut sprites = Vec::with_capacity(index.entries.len());
    for entry in &index.entries {
        let path = store.sprite_path(entry.id);
        let png = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        sprites.push((entry.id, png));
    }
    let (pack, bin) = SpritePack::pack(sprites);

    let files = [
        ("index.json", serde_json::to_vec(&index)?),
        ("sprites.json", serde_json::to_vec(&pack)?),
        ("sprites.bin", bin),
    ];
    for (name, bytes) in &files {
        std::fs::write(out.join(name), bytes)?;
        println!("{:<12} {:>7.0} KB", name, bytes.len() as f64 / 1024.0);
    }
    println!("{} sprites packed", pack.sprites.len());
    Ok(())
}
