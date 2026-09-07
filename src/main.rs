use std::process::ExitCode;

use clap::Parser;

use gh_pr_digest::cli::{Cli, OutputTarget, PrTarget};
use gh_pr_digest::digest::Digest;
use gh_pr_digest::error::Result;
use gh_pr_digest::filter::Filter;
use gh_pr_digest::github::{auth, client::Client, fetch};
use gh_pr_digest::output;
use gh_pr_digest::render::markdown;
use gh_pr_digest::repo::{self, Repo};

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Combinations clap cannot express are still usage errors, so they share
    // clap's exit code 2 rather than getting one of our own.
    if let Err(message) = cli.validate() {
        output::error(&message);
        return ExitCode::from(2);
    }

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            output::error(&error.to_string());
            ExitCode::from(error.exit_code())
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    let number = cli.target.number();
    let (repo, explicit_host) = resolve_repo(cli)?;
    let host = auth::resolve_host(explicit_host.as_deref(), Some(&repo.host));

    let token = auth::resolve_token(cli.token.as_deref(), &host)?;
    let client = Client::new(auth::graphql_endpoint(&host), token);

    output::info(&format!("fetching {repo}#{number} from {host}"));

    let pull_request = fetch::pull_request(&client, &repo, number)?;
    let digest = Digest::from_wire(pull_request);

    let (status, outdated) = cli.filters();
    let document = markdown::render(&digest, Filter::new(status, outdated));

    let target = cli
        .output
        .clone()
        .unwrap_or_else(|| OutputTarget::File(output::default_path(number)));

    if let Some(path) = output::write(&target, &document)? {
        output::success(&format!("wrote {}", path.display()));
    }

    Ok(())
}

/// Work out the repository, and whether the user named its host out loud.
fn resolve_repo(cli: &Cli) -> Result<(Repo, Option<String>)> {
    if let PrTarget::Url {
        host, owner, repo, ..
    } = &cli.target
    {
        let repo = Repo {
            host: host.clone(),
            owner: owner.clone(),
            name: repo.clone(),
        };
        return Ok((repo, Some(host.clone())));
    }

    if let Some(spec) = &cli.repo {
        let explicit = spec.host.clone().or_else(|| cli.hostname.clone());
        let host = auth::resolve_host(explicit.as_deref(), None);
        let repo = Repo {
            host,
            owner: spec.owner.clone(),
            name: spec.name.clone(),
        };
        let named = repo.host.clone();
        return Ok((repo, Some(named)));
    }

    // Nothing named a repository, so the host we detect alongside it is
    // inferred rather than explicit -- `--hostname` still overrides it.
    let detected = repo::detect()?;
    Ok((detected, cli.hostname.clone()))
}
