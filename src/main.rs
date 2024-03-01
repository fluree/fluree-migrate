use cli::local_directory::LocalDirectory;
use cli::source::Migrate;
use structopt::StructOpt;

mod cli;
mod console;
mod fluree;
mod functions;

use cli::opt::Opt;
use fluree::FlureeInstance;

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let opt = Opt::from_args();

    if opt.input.is_some() {
        let mut source_directory = LocalDirectory::new(&opt);
        source_directory.migrate().await;
    } else {
        let mut source_instance = FlureeInstance::new_source(&opt);
        source_instance.migrate().await;
    }

    Ok(())
}
