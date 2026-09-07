use std::process::ExitCode;

use clap::Parser;

use gh_pr_digest::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Combinations clap cannot express are still usage errors, so they share
    // clap's exit code 2 rather than getting one of our own.
    if let Err(message) = cli.validate() {
        eprintln!("error: {message}");
        return ExitCode::from(2);
    }

    let (status, outdated) = cli.filters();
    eprintln!(
        "target={:?} status={status:?} outdated={outdated:?}",
        cli.target
    );

    ExitCode::SUCCESS
}
