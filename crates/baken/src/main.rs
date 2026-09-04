mod args;
mod cdjsafe;
mod cli;
mod progress;
mod rbsort;
mod report;
mod updater;

fn main() -> anyhow::Result<()> {
    cli::run()
}
