use clap::Parser;

/// Thanatos: ferramenta do Hades, reescrita em Rust.
#[derive(Parser)]
#[command(name = "thanatos", version, about)]
struct Cli {}

fn main() {
    Cli::parse();
}
