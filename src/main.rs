mod analyzer;
mod args;
mod cli;
mod processor;
mod rbsort;
mod report;
mod scanner;
mod updater;

use anyhow::Result;

fn main() -> Result<()> {
    cli::run()
}
