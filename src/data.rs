//! Where the COLOR MATCH tab gets its colour index and sprites.
//!
//! - Desktop: the local sprite cache (built on demand, see `index::Store`).
//! - Web: the pre-built bundle in `web-data/`, fetched once at startup.

use std::borrow::Cow;

use pokepicker::sprite_pack::SpritePack;

pub enum Sprites {
    #[cfg(not(target_arch = "wasm32"))]
    Cache(pokepicker::index::Store),
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Pack { table: SpritePack, bytes: Vec<u8> },
}

impl Sprites {
    /// The PNG bytes of one sprite, if available.
    pub fn png(&self, id: u32) -> Option<Cow<'_, [u8]>> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Sprites::Cache(store) => std::fs::read(store.sprite_path(id)).ok().map(Cow::Owned),
            Sprites::Pack { table, bytes } => table.get(bytes, id).map(Cow::Borrowed),
        }
    }
}

/// The desktop cache, plus its saved index if one exists.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_cache() -> (Option<pokepicker::index::Index>, Option<Sprites>) {
    use pokepicker::index::{Index, Store};
    let Ok(store) = Store::default_location() else {
        return (None, None);
    };
    let index = Index::load(&store).ok().flatten();
    (index, Some(Sprites::Cache(store)))
}

/// Fetch `web-data/` (served next to the app) and call `done` with the result.
#[cfg(target_arch = "wasm32")]
pub fn load_bundle(
    done: impl FnOnce(anyhow::Result<(pokepicker::index::Index, Sprites)>) + Send + 'static,
) {
    use anyhow::{Context, anyhow};
    use pokepicker::index::Index;

    fn get(name: &'static str, then: impl FnOnce(anyhow::Result<Vec<u8>>) + Send + 'static) {
        ehttp::fetch(
            ehttp::Request::get(format!("web-data/{name}")),
            move |res| {
                then(match res {
                    Ok(r) if r.ok => Ok(r.bytes),
                    Ok(r) => Err(anyhow!("HTTP {} loading {name}", r.status)),
                    Err(e) => Err(anyhow!("network error loading {name}: {e}")),
                })
            },
        );
    }

    // Three small requests in sequence; simplest, and the bin dominates anyway.
    get("index.json", move |index| {
        get("sprites.json", move |table| {
            get("sprites.bin", move |bytes| {
                done((|| {
                    let index = Index::from_json(&index?)?
                        .context("web-data/index.json is from an older format; re-run bundle")?;
                    let table: SpritePack = serde_json::from_slice(&table?)?;
                    Ok((
                        index,
                        Sprites::Pack {
                            table,
                            bytes: bytes?,
                        },
                    ))
                })())
            })
        })
    });
}
