# Pokepicker

Look up Pokémon from [PokeAPI](https://pokeapi.co), or pick a colour and find
the Pokémon whose sprites wear it. Rust + [egui](https://github.com/emilk/egui),
running as a desktop app or in the browser.

## Desktop

```sh
cargo run
```

The first COLOR MATCH search downloads every sprite (~1.4 MB) into the OS
cache folder (`~/Library/Caches/pokepicker` on macOS) and builds a colour
index there. DATA → REBUILD COLOR INDEX refreshes it.

## Web

The web build is static files: the app (WebAssembly + a small JS loader),
`index.html`, and the bundled colour index and sprites in `web-data/`. There
is no server-side code; any static file server works.

### Build

One-time setup:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
```

Then:

```sh
trunk build --release --cargo-profile web
```

This writes the site to `dist/`. For development, `trunk serve` builds,
serves on http://localhost:8080 and rebuilds on save.

### Serve

Point the web server's document root at `dist/` (the site expects to be at
the root of its domain, e.g. https://poke.mart.pizza/). Recommended settings:

- **MIME type:** serve `.wasm` as `application/wasm`. Most servers already do;
  without it browsers fall back to a slower way of loading the app.
- **Compression:** enable gzip or brotli for `.wasm`, `.js` and `.json`. The
  app is ~3.6 MB raw but ~1.5 MB gzipped. `web-data/sprites.bin` is
  already-compressed PNGs, so compressing it gains nothing.
- **Caching:** the `.wasm` and `.js` filenames contain a content hash, so
  they can be cached for a long time. Keep `index.html` and `web-data/` on
  short or no caching so updates show up.

For example, with nginx:

```nginx
server {
    server_name poke.mart.pizza;
    root /path/to/pokepicker/dist;

    gzip on;
    gzip_types application/wasm application/javascript application/json;

    location ~* \.(wasm|js)$ {
        add_header Cache-Control "public, max-age=31536000, immutable";
    }
    location / {
        add_header Cache-Control "no-cache";
    }
}
```

### Update

```sh
git pull
trunk build --release --cargo-profile web
```

### Refreshing the bundled data

`web-data/` is committed, so building the site never needs to download
sprites. It's generated from the desktop cache, so to pick up new Pokémon
from PokeAPI:

1. In the desktop app, run DATA → REBUILD COLOR INDEX.
2. Regenerate the bundle and commit `web-data/`:

   ```sh
   cargo run --release --bin bundle
   ```
