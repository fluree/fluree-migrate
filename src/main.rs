use clap::Parser;
use cli::local_directory::LocalDirectory;
use cli::source::Migrate;

mod cli;
mod console;
mod fluree;
mod functions;

use cli::opt::Opt;
use fluree::FlureeInstance;

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    env_logger::init();
    let opt = Opt::parse();

    if opt.input.is_some() {
        let mut source_directory = LocalDirectory::new(&opt);
        source_directory.migrate().await;
    } else {
        let mut source_instance = FlureeInstance::new_source(&opt);
        source_instance.migrate().await;
    }

    Ok(())
}
