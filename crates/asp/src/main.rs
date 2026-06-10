use clap::Parser;

#[derive(Parser)]
#[command(
    name = "asp",
    version,
    about = "agentspaces: instant, disposable, fully-reviewable forks of your real working directory for AI agents"
)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
    println!(
        "asp {} — engine {}",
        env!("CARGO_PKG_VERSION"),
        asp_core::version()
    );
}
