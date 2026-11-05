//! Local sprite cache and colour index for reverse colour lookup.
//!
//! Layout on disk (under the OS cache dir, e.g. `~/Library/Caches/pokepicker`):
//!
//! ```text
//! sprites/{id}.png   raw sprites, downloaded once
//! index.json         per-sprite colour palettes, rebuilt from the sprites
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::api::Client;
use crate::color::{Lab, Rgb};

/// Bump when the palette algorithm changes so stale indexes get rebuilt.
const FORMAT: u32 = 1;
/// Concurrent sprite downloads. Polite to the host, still finishes in well under a minute.
const WORKERS: usize = 8;
/// Pixels with less alpha than this are background.
const ALPHA_CUTOFF: u8 = 128;
/// Palette entries kept per sprite. Classic sprites use ~16 colours; this
/// keeps newer, softer-shaded ones bounded.
const MAX_SWATCHES: usize = 64;
/// Matches covering less of the sprite than this are dropped as noise.
const MIN_SCORE: f32 = 0.02;

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn default_location() -> Result<Self> {
        let root = dirs::cache_dir()
            .context("no cache directory on this OS")?
            .join("pokepicker");
        Ok(Self { root })
    }

    pub fn sprite_path(&self, id: u32) -> PathBuf {
        self.root.join("sprites").join(format!("{id}.png"))
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    /// Cached sprite bytes, downloading on a miss. `None` if the pokemon has no sprite.
    fn sprite(&self, client: &Client, id: u32) -> Result<Option<Vec<u8>>> {
        let path = self.sprite_path(id);
        if let Ok(bytes) = std::fs::read(&path) {
            return Ok(Some(bytes));
        }
        let Some(bytes) = client.sprite_by_id(id)? else {
            return Ok(None);
        };
        write_atomic(&path, &bytes)?;
        Ok(Some(bytes))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Swatch {
    pub rgb: Rgb,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: u32,
    pub name: String,
    /// Opaque pixel count; the denominator for coverage.
    pub pixels: u32,
    /// Most common colours first.
    pub palette: Vec<Swatch>,
}

#[derive(Debug, Clone)]
pub struct Match {
    pub id: u32,
    pub name: String,
    /// Ranking score: coverage weighted by closeness, 0..=1.
    pub score: f32,
    /// Share of the sprite within tolerance of the target, 0..=1.
    pub coverage: f32,
    /// The sprite's colour closest to the target.
    pub closest: Rgb,
}

#[derive(Default)]
pub struct Progress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
}

pub struct BuildReport {
    pub index: Index,
    /// Pokemon whose sprite couldn't be fetched or decoded; a rebuild retries them.
    pub failed: usize,
}

#[derive(Serialize, Deserialize)]
pub struct Index {
    format: u32,
    pub entries: Vec<Entry>,
}

impl Index {
    /// The saved index, or `None` if there isn't one (or it's from an older format).
    pub fn load(store: &Store) -> Result<Option<Self>> {
        let Ok(bytes) = std::fs::read(store.index_path()) else {
            return Ok(None);
        };
        let index: Index = serde_json::from_slice(&bytes).context("corrupt index.json")?;
        Ok((index.format == FORMAT).then_some(index))
    }

    fn save(&self, store: &Store) -> Result<()> {
        write_atomic(&store.index_path(), &serde_json::to_vec(self)?)
    }

    /// Download any missing sprites, extract every palette, and save the index.
    pub fn build(store: &Store, client: &Client, progress: &Progress) -> Result<BuildReport> {
        let all = client.list_pokemon()?;
        progress.total.store(all.len(), Ordering::Relaxed);

        let next = AtomicUsize::new(0);
        let entries = Mutex::new(Vec::with_capacity(all.len()));
        let failed = AtomicUsize::new(0);

        std::thread::scope(|s| {
            for _ in 0..WORKERS {
                s.spawn(|| {
                    while let Some((id, name)) = all.get(next.fetch_add(1, Ordering::Relaxed)) {
                        match index_one(store, client, *id, name) {
                            Ok(Some(entry)) => entries.lock().unwrap().push(entry),
                            Ok(None) => {}
                            Err(_) => _ = failed.fetch_add(1, Ordering::Relaxed),
                        }
                        progress.done.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });

        let mut entries = entries.into_inner().unwrap();
        entries.sort_by_key(|e| e.id);
        let index = Index {
            format: FORMAT,
            entries,
        };
        index.save(store)?;
        Ok(BuildReport {
            index,
            failed: failed.into_inner(),
        })
    }

    /// Pokemon whose sprites contain colours near `target`, best first.
    ///
    /// `tolerance` is the ΔE radius. A palette colour counts fully at distance 0
    /// and fades to nothing at `tolerance` along `1 - (d/tol)²`, so anything
    /// inside the radius still counts substantially and coverage dominates.
    pub fn search(&self, target: Rgb, tolerance: f32) -> Vec<Match> {
        let target = Lab::from_rgb(target);
        let mut matches: Vec<Match> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (mut weighted, mut within) = (0.0, 0);
                let mut closest = (f32::MAX, [0; 3]);
                for swatch in &entry.palette {
                    let d = target.delta_e(Lab::from_rgb(swatch.rgb));
                    if d < tolerance {
                        weighted += swatch.count as f32 * (1.0 - (d / tolerance).powi(2));
                        within += swatch.count;
                    }
                    if d < closest.0 {
                        closest = (d, swatch.rgb);
                    }
                }
                let score = weighted / entry.pixels as f32;
                (score >= MIN_SCORE).then(|| Match {
                    id: entry.id,
                    name: entry.name.clone(),
                    score,
                    coverage: within as f32 / entry.pixels as f32,
                    closest: closest.1,
                })
            })
            .collect();
        matches.sort_by(|a, b| b.score.total_cmp(&a.score));
        matches
    }
}

fn index_one(store: &Store, client: &Client, id: u32, name: &str) -> Result<Option<Entry>> {
    let Some(bytes) = store.sprite(client, id)? else {
        return Ok(None);
    };
    let (pixels, palette) = palette(&bytes)?;
    Ok((pixels > 0).then(|| Entry {
        id,
        name: name.to_owned(),
        pixels,
        palette,
    }))
}

/// Opaque pixel count and the sprite's colours, most common first.
///
/// Near-identical colours (same top 5 bits per channel) are merged and
/// represented by their average, which keeps anti-aliased sprites compact.
pub fn palette(png: &[u8]) -> Result<(u32, Vec<Swatch>)> {
    let img = image::load_from_memory(png)?.to_rgba8();
    let mut buckets: HashMap<[u8; 3], ([u32; 3], u32)> = HashMap::new();
    let mut pixels = 0;
    for &image::Rgba([r, g, b, a]) in img.pixels() {
        if a < ALPHA_CUTOFF {
            continue;
        }
        pixels += 1;
        let (sum, count) = buckets.entry([r >> 3, g >> 3, b >> 3]).or_default();
        sum[0] += r as u32;
        sum[1] += g as u32;
        sum[2] += b as u32;
        *count += 1;
    }
    let mut palette: Vec<Swatch> = buckets
        .into_values()
        .map(|(sum, count)| Swatch {
            rgb: sum.map(|c| (c / count) as u8),
            count,
        })
        .collect();
    palette.sort_by_key(|s| std::cmp::Reverse(s.count));
    palette.truncate(MAX_SWATCHES);
    Ok((pixels, palette))
}

/// Write via a temp file so an interrupted build never leaves a truncated file.
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, palette: &[(Rgb, u32)]) -> Entry {
        let palette: Vec<_> = palette
            .iter()
            .map(|&(rgb, count)| Swatch { rgb, count })
            .collect();
        Entry {
            id,
            name: format!("mon{id}"),
            pixels: palette.iter().map(|s| s.count).sum(),
            palette,
        }
    }

    #[test]
    fn search_ranks_by_closeness_and_coverage() {
        let yellow = [250, 210, 40];
        let index = Index {
            format: FORMAT,
            entries: vec![
                entry(1, &[([40, 80, 200], 90), ([20, 20, 20], 10)]), // blue: no match
                entry(2, &[(yellow, 30), ([20, 20, 20], 70)]),        // a little yellow
                entry(3, &[(yellow, 80), ([20, 20, 20], 20)]),        // mostly yellow
                entry(4, &[([245, 200, 50], 80), ([20, 20, 20], 20)]), // mostly near-yellow
            ],
        };
        let ids: Vec<_> = index
            .search([255, 215, 30], 20.0)
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, [3, 4, 2]);
    }

    #[test]
    fn palette_ignores_transparent_pixels_and_merges_near_colours() {
        let mut img = image::RgbaImage::new(4, 1);
        img.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
        img.put_pixel(1, 0, image::Rgba([200, 100, 50, 255]));
        img.put_pixel(2, 0, image::Rgba([202, 102, 52, 255]));
        img.put_pixel(3, 0, image::Rgba([10, 10, 10, 255]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        let (pixels, palette) = palette(&png).unwrap();
        assert_eq!(pixels, 3);
        assert_eq!(palette.len(), 2);
        assert_eq!(palette[0].count, 2);
        assert_eq!(palette[0].rgb, [201, 101, 51]);
    }
}
