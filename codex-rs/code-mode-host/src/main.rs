use clap::Parser;

#[derive(Debug, Parser)]
struct Cli {
    /// Transport URL: `stdio` or a loopback `grpc://IP:PORT` URL.
    #[arg(long, default_value = "stdio", value_name = "URL")]
    listen: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    codex_code_mode_host::run_transport(&cli.listen).await
}
