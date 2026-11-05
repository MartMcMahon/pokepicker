//! Local sprite cache and colour index for reverse colour lookup.
//!
//! Layout on disk (under the OS cache dir, e.g. `~/Library/Caches/pokepicker`):
//!
//! ```text
//! sprites/{id}.png   raw sprites, downloaded once
//! index.json         per-sprite colour palettes, rebuilt from the sprites
//! ```

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::color::{Lab, Rgb};

#[cfg(not(target_arch = "wasm32"))]
pub use cache::{BuildReport, Progress, Store};

/// Bump when the palette algorithm changes so stale indexes get rebuilt.
const FORMAT: u32 = 1;
/// Pixels with less alpha than this are background.
const ALPHA_CUTOFF: u8 = 128;
/// Palette entries kept per sprite. Classic sprites use ~16 colours; this
/// keeps newer, softer-shaded ones bounded.
const MAX_SWATCHES: usize = 64;
/// Matches covering less of the sprite than this are dropped as noise.
const MIN_SCORE: f32 = 0.02;

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

#[derive(Serialize, Deserialize)]
pub struct Index {
    format: u32,
    pub entries: Vec<Entry>,
    /// Each palette colour converted to Lab, parallel to `entries[i].palette`.
    /// Derived data: computed once on creation/load rather than on every search.
    #[serde(skip)]
    labs: Vec<Vec<Lab>>,
}

impl Index {
    pub fn new(entries: Vec<Entry>) -> Self {
        let mut index = Index {
            format: FORMAT,
            entries,
            labs: Vec::new(),
        };
        index.prepare();
        index
    }

    /// Parse a saved index, or `None` if it's from an older format.
    pub fn from_json(bytes: &[u8]) -> Result<Option<Self>> {
        let mut index: Index = serde_json::from_slice(bytes).context("corrupt index.json")?;
        if index.format != FORMAT {
            return Ok(None);
        }
        index.prepare();
        Ok(Some(index))
    }

    fn prepare(&mut self) {
        self.labs = self
            .entries
            .iter()
            .map(|e| e.palette.iter().map(|s| Lab::from_rgb(s.rgb)).collect())
            .collect();
    }

    /// Pokemon whose sprites contain colours near `target`, best first;
    /// equal scores are ordered by pokedex number.
    ///
    /// `tolerance` is the ΔE radius. A palette colour counts fully at distance 0
    /// and fades to nothing at `tolerance` along `1 - (d/tol)²`, so anything
    /// inside the radius still counts substantially and coverage dominates.
    ///
    /// Works in squared distances throughout: `(d/tol)²` is `d²/tol²`, so the
    /// square root in ΔE is never needed.
    pub fn search(&self, target: Rgb, tolerance: f32) -> Vec<Match> {
        let target = Lab::from_rgb(target);
        let tol2 = tolerance * tolerance;
        let mut matches: Vec<Match> = self
            .entries
            .iter()
            .zip(&self.labs)
            .filter_map(|(entry, labs)| {
                let (mut weighted, mut within) = (0.0, 0);
                let mut closest = (f32::MAX, [0; 3]);
                for (swatch, &lab) in entry.palette.iter().zip(labs) {
                    let d2 = target.delta_e_squared(lab);
                    if d2 < tol2 {
                        weighted += swatch.count as f32 * (1.0 - d2 / tol2);
                        within += swatch.count;
                    }
                    if d2 < closest.0 {
                        closest = (d2, swatch.rgb);
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
        matches.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        matches
    }
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

/// The desktop sprite cache and index builder. Browsers have no file system
/// or threads; the web build loads a pre-built bundle instead (see `sprite_pack`).
#[cfg(not(target_arch = "wasm32"))]
mod cache {
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use anyhow::{Context, Result};

    use super::{Entry, Index, palette};
    use crate::api::Client;

    /// Concurrent sprite downloads. Polite to the host, still finishes in well under a minute.
    const WORKERS: usize = 8;

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

    impl Index {
        /// The saved index, or `None` if there isn't one (or it's from an older format).
        pub fn load(store: &Store) -> Result<Option<Self>> {
            let Ok(bytes) = std::fs::read(store.index_path()) else {
                return Ok(None);
            };
            Index::from_json(&bytes)
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
            let index = Index::new(entries);
            index.save(store)?;
            Ok(BuildReport {
                index,
                failed: failed.into_inner(),
            })
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
        let index = Index::new(vec![
            entry(1, &[([40, 80, 200], 90), ([20, 20, 20], 10)]), // blue: no match
            entry(2, &[(yellow, 30), ([20, 20, 20], 70)]),        // a little yellow
            entry(3, &[(yellow, 80), ([20, 20, 20], 20)]),        // mostly yellow
            entry(4, &[([245, 200, 50], 80), ([20, 20, 20], 20)]), // mostly near-yellow
        ]);
        let ids: Vec<_> = index
            .search([255, 215, 30], 20.0)
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, [3, 4, 2]);
    }

    #[test]
    fn equal_scores_are_ordered_by_id() {
        let pal = [([250, 210, 40], 50), ([20, 20, 20], 50)];
        let index = Index::new(vec![entry(30, &pal), entry(10, &pal), entry(20, &pal)]);
        let ids: Vec<_> = index
            .search([250, 210, 40], 20.0)
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, [10, 20, 30]);
    }

    /// The original search: converts every swatch to Lab per search and uses
    /// true ΔE with a square root. Kept as the reference for `search`.
    fn reference_search(index: &Index, target: Rgb, tolerance: f32) -> Vec<Match> {
        let target = Lab::from_rgb(target);
        index
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
            .collect()
    }

    /// Sprite-shaped fixture: a dark outline plus 4-30 colours with skewed
    /// pixel counts, from a fixed-seed generator so the test is deterministic.
    /// Every 10th entry duplicates its predecessor's palette to create ties.
    fn fixture() -> Index {
        let mut seed = 0x2545_f491_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let mut entries: Vec<Entry> = Vec::new();
        for id in 1..=400 {
            let mut pal = vec![([16, 16, 24], 80 + next() % 120)];
            if id % 10 == 0 {
                let prev = entries.last().unwrap();
                pal = prev.palette.iter().map(|s| (s.rgb, s.count)).collect();
            } else {
                for i in 0..4 + next() % 27 {
                    let [r, g, b, _] = next().to_le_bytes();
                    pal.push(([r, g, b], 1 + next() % (600 / (i + 1))));
                }
            }
            entries.push(entry(id, &pal));
        }
        Index::new(entries)
    }

    #[test]
    fn search_matches_reference_implementation() {
        let index = fixture();
        let steps = [0u8, 51, 102, 153, 204, 255];
        for r in steps {
            for g in steps {
                for b in steps {
                    for tol in [5.0, 12.0, 20.0, 35.0, 50.0] {
                        let target = [r, g, b];
                        let fast = index.search(target, tol);
                        let reference = reference_search(&index, target, tol);

                        let mut fast_ids: Vec<_> = fast.iter().map(|m| m.id).collect();
                        let mut ref_ids: Vec<_> = reference.iter().map(|m| m.id).collect();
                        fast_ids.sort();
                        ref_ids.sort();
                        assert_eq!(fast_ids, ref_ids, "result set for {target:?} tol {tol}");

                        for m in &fast {
                            let r = reference.iter().find(|x| x.id == m.id).unwrap();
                            assert!((m.score - r.score).abs() < 1e-5, "score for {}", m.id);
                            assert_eq!(m.coverage, r.coverage);
                            assert_eq!(m.closest, r.closest);
                        }

                        // Best first, ties by id.
                        assert!(fast.windows(2).all(|w| {
                            w[0].score > w[1].score
                                || (w[0].score == w[1].score && w[0].id < w[1].id)
                        }));
                    }
                }
            }
        }
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
