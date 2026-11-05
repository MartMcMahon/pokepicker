//! Every sprite in one file: the original PNGs back to back, plus a small
//! table of where each one starts. The web build downloads this instead of
//! ~1,300 separate files.
//!
//! Keeping the original PNG bytes (rather than repacking pixels into one
//! sheet) preserves PokeAPI's palette compression: most sprites are 16- or
//! 256-colour PNGs, which a combined full-colour sheet would double in size.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Byte range of one sprite's PNG within the packed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub offset: u32,
    pub len: u32,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpritePack {
    /// Keyed by pokemon id; a BTreeMap so the JSON is stable across runs.
    pub sprites: BTreeMap<u32, Span>,
}

impl SpritePack {
    /// Concatenate the sprites in id order, returning the table and the packed bytes.
    pub fn pack(sprites: impl IntoIterator<Item = (u32, Vec<u8>)>) -> (SpritePack, Vec<u8>) {
        let sorted: BTreeMap<u32, Vec<u8>> = sprites.into_iter().collect();
        let mut pack = SpritePack::default();
        let mut bytes = Vec::with_capacity(sorted.values().map(Vec::len).sum());
        for (id, png) in sorted {
            let span = Span {
                offset: bytes.len() as u32,
                len: png.len() as u32,
            };
            bytes.extend_from_slice(&png);
            pack.sprites.insert(id, span);
        }
        (pack, bytes)
    }

    /// The PNG bytes for `id`, or `None` if it isn't packed (or `bytes` is truncated).
    pub fn get<'a>(&self, bytes: &'a [u8], id: u32) -> Option<&'a [u8]> {
        let Span { offset, len } = *self.sprites.get(&id)?;
        bytes.get(offset as usize..(offset + len) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_png(id: u32) -> Vec<u8> {
        (0..10 + id * 7 % 50).map(|i| (i * id) as u8).collect()
    }

    #[test]
    fn sprites_round_trip() {
        let sprites: Vec<_> = (1..=50).map(|id| (id, fake_png(id))).collect();
        let (pack, bytes) = SpritePack::pack(sprites.clone());
        for (id, png) in &sprites {
            assert_eq!(pack.get(&bytes, *id), Some(png.as_slice()), "sprite {id}");
        }
        assert_eq!(pack.get(&bytes, 999), None);
        assert_eq!(
            bytes.len(),
            sprites.iter().map(|(_, p)| p.len()).sum::<usize>()
        );
    }

    #[test]
    fn packing_is_deterministic() {
        let sprites: Vec<_> = (1..=20).map(|id| (id, fake_png(id))).collect();
        let mut reversed = sprites.clone();
        reversed.reverse();
        assert_eq!(SpritePack::pack(sprites), SpritePack::pack(reversed));
    }

    #[test]
    fn truncated_file_is_none_not_a_panic() {
        let (pack, bytes) = SpritePack::pack([(1, fake_png(1)), (2, fake_png(2))]);
        let cut = &bytes[..bytes.len() - 1];
        assert!(pack.get(cut, 1).is_some());
        assert_eq!(pack.get(cut, 2), None);
    }
}
