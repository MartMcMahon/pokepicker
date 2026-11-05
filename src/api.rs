//! Thin blocking client for https://pokeapi.co/api/v2/
//!
//! Blocking is fine here: the GUI runs every request on a worker thread.

use anyhow::{Context, Result};
use serde::Deserialize;

pub const BASE_URL: &str = "https://pokeapi.co/api/v2";

/// Where PokeAPI hosts its sprites; `front_default` always points here.
/// Building the URL from the id lets us skip one API call per pokemon.
pub const SPRITE_BASE_URL: &str =
    "https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon";

pub fn sprite_url(id: u32) -> String {
    format!("{SPRITE_BASE_URL}/{id}.png")
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pokemon {
    pub id: u32,
    pub name: String,
    /// Decimetres.
    pub height: u32,
    /// Hectograms.
    pub weight: u32,
    pub types: Vec<TypeSlot>,
    pub sprites: Sprites,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TypeSlot {
    pub slot: u8,
    #[serde(rename = "type")]
    pub kind: NamedResource,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NamedResource {
    pub name: String,
    pub url: String,
}

impl NamedResource {
    /// Resource URLs end in their numeric id, e.g. `.../pokemon/25/`.
    pub fn id(&self) -> Option<u32> {
        self.url
            .trim_end_matches('/')
            .rsplit('/')
            .next()?
            .parse()
            .ok()
    }
}

#[derive(Deserialize)]
struct Page {
    results: Vec<NamedResource>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Sprites {
    pub front_default: Option<String>,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::blocking::Client,
}

impl Client {
    pub fn new() -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("pokepicker/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { http })
    }

    /// GET /pokemon/{name_or_id}
    pub fn pokemon(&self, name_or_id: &str) -> Result<Pokemon> {
        let key = name_or_id.trim().to_lowercase();
        let url = format!("{BASE_URL}/pokemon/{key}");
        self.http
            .get(&url)
            .send()?
            .error_for_status()
            .with_context(|| format!("no pokemon named {key:?}"))?
            .json()
            .context("unexpected response shape")
    }

    /// Every pokemon (including alternate forms) as `(id, name)`, in one request.
    pub fn list_pokemon(&self) -> Result<Vec<(u32, String)>> {
        let page: Page = self
            .http
            .get(format!("{BASE_URL}/pokemon?limit=100000"))
            .send()?
            .error_for_status()?
            .json()
            .context("unexpected response shape")?;
        Ok(page
            .results
            .into_iter()
            .filter_map(|r| Some((r.id()?, r.name)))
            .collect())
    }

    /// Sprite bytes by id, or `None` if that pokemon has no sprite (some forms don't).
    pub fn sprite_by_id(&self, id: u32) -> Result<Option<Vec<u8>>> {
        let resp = self.http.get(sprite_url(id)).send()?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(resp.error_for_status()?.bytes()?.to_vec()))
    }

    /// Download the sprite for a pokemon and return the raw image bytes.
    ///
    /// Note: PokeAPI serves sprites as PNG, not JPG. The bytes are returned
    /// undecoded; `image::load_from_memory` sniffs the format either way.
    pub fn sprite(&self, pokemon: &Pokemon) -> Result<Vec<u8>> {
        let url = pokemon
            .sprites
            .front_default
            .as_deref()
            .with_context(|| format!("{} has no sprite", pokemon.name))?;
        let bytes = self.http.get(url).send()?.error_for_status()?.bytes()?;
        Ok(bytes.to_vec())
    }
}
