//! Reverse colour lookup from the command line. Builds the index on first run.
//!
//!     cargo run --example color -- '#FFD733' [tolerance]

use pokepicker::api::Client;
use pokepicker::color::{parse_hex, to_hex};
use pokepicker::index::{Index, Progress, Store};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let hex = args.next().unwrap_or_else(|| "#FFD733".into());
    let target = parse_hex(&hex).ok_or_else(|| anyhow::anyhow!("bad hex colour {hex:?}"))?;
    let tolerance: f32 = args.next().map(|t| t.parse()).transpose()?.unwrap_or(20.0);

    let store = Store::default_location()?;
    let index = match Index::load(&store)? {
        Some(index) => index,
        None => {
            eprintln!("Building colour index (first run only)...");
            let report = Index::build(&store, &Client::new()?, &Progress::default())?;
            eprintln!(
                "Indexed {} sprites, {} failed",
                report.index.entries.len(),
                report.failed
            );
            report.index
        }
    };

    for m in index.search(target, tolerance).iter().take(15) {
        println!(
            "#{:<5} {:<28} score {:>5.1}%  coverage {:>5.1}%  closest {}",
            m.id,
            m.name,
            m.score * 100.0,
            m.coverage * 100.0,
            to_hex(m.closest)
        );
    }
    Ok(())
}
