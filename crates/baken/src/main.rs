mod args;
mod cdjsafe;
mod cli;
#[cfg(feature = "expressport")]
mod expressport;
mod progress;
mod rbsort;
mod report;
mod updater;

fn main() -> anyhow::Result<()> {
    cli::run()
}
