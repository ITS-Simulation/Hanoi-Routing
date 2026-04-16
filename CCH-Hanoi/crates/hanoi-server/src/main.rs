use clap::Parser;

use hanoi_server::app::args::Args;

#[tokio::main]
async fn main() {
    hanoi_server::app::bootstrap::run(Args::parse()).await;
}
