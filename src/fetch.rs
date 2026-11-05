//! Non-blocking pokemon lookup for the GUI, via `ehttp`.
//!
//! `ehttp` runs requests on a background thread natively and on the browser's
//! event loop on the web, so this is the one network path both builds share.

use anyhow::{Context, Result, anyhow, bail};
use eframe::egui::ColorImage;
use pokepicker::api::{self, Pokemon};

pub type Lookup = Result<(Pokemon, ColorImage)>;

/// Fetch a pokemon and its sprite, then call `done` with the result.
///
/// `done` runs off the UI thread on desktop, so it should only hand the
/// result over (e.g. through a channel) and request a repaint.
pub fn lookup(name_or_id: &str, done: impl FnOnce(Lookup) + Send + 'static) {
    let key = api::pokemon_key(name_or_id);
    get(api::pokemon_url(&key), move |res| {
        let mon: Pokemon = match parse(&key, res) {
            Ok(mon) => mon,
            Err(e) => return done(Err(e)),
        };
        let Some(url) = mon.sprites.front_default.clone() else {
            return done(Err(anyhow!("{} has no sprite", mon.name)));
        };
        get(url, move |res| done(decode(res).map(|image| (mon, image))));
    });
}

fn get(url: String, on_done: impl FnOnce(ehttp::Result<ehttp::Response>) + Send + 'static) {
    #[allow(unused_mut)]
    let mut request = ehttp::Request::get(url);
    // Browsers don't allow scripts to set User-Agent; desktop identifies itself.
    #[cfg(not(target_arch = "wasm32"))]
    request.headers.insert("User-Agent", api::USER_AGENT);
    ehttp::fetch(request, on_done);
}

fn ok(res: ehttp::Result<ehttp::Response>) -> Result<ehttp::Response> {
    let res = res.map_err(|e| anyhow!("network error: {e}"))?;
    if !res.ok {
        bail!("HTTP {} {} for {}", res.status, res.status_text, res.url);
    }
    Ok(res)
}

fn parse(key: &str, res: ehttp::Result<ehttp::Response>) -> Result<Pokemon> {
    let res = ok(res).with_context(|| format!("no pokemon named {key:?}"))?;
    serde_json::from_slice(&res.bytes).context("unexpected response shape")
}

fn decode(res: ehttp::Result<ehttp::Response>) -> Result<ColorImage> {
    let res = ok(res)?;
    let rgba = image::load_from_memory(&res.bytes)?.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    fn run(query: &str) -> Lookup {
        let (tx, rx) = channel();
        lookup(query, move |r| tx.send(r).unwrap());
        rx.recv_timeout(Duration::from_secs(30))
            .expect("lookup timed out")
    }

    #[test]
    #[ignore = "hits the live PokeAPI; run with --ignored"]
    fn looks_up_pokemon_and_sprite() {
        let (mon, image) = run("  Pikachu ").unwrap();
        assert_eq!((mon.id, mon.name.as_str()), (25, "pikachu"));
        assert_eq!(image.size, [96, 96]);
    }

    #[test]
    #[ignore = "hits the live PokeAPI; run with --ignored"]
    fn unknown_pokemon_is_a_friendly_error() {
        let err = run("notamon").unwrap_err();
        assert!(
            format!("{err:#}").starts_with("no pokemon named \"notamon\""),
            "{err:#}"
        );
    }
}
