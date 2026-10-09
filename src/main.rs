mod action;
mod cmdline;
mod combination;
mod config;
mod draw;
mod error;
mod keysym;
mod session;
mod wayland;

use structopt::StructOpt;

use crate::cmdline::Opt;
use crate::error::Error;
use crate::session::Session;

fn main_wrap() -> Result<(), Error> {
    let opt = Opt::from_args();
    if opt.example_config {
        print!("{}", serde_yaml::to_string(&config::example())?);
        return Ok(());
    }

    let mut session = Session::load(&opt)?;
    let root = session.resolve_root()?;
    let initial = session.inject(&root)?;
    wayland::run(session, initial)
}

fn main() {
    match main_wrap() {
        Ok(()) => {}
        Err(Error::ConfigError(e)) => {
            eprintln!("Error: {}", e);
            eprintln!("See a sample config file with --show-example-config");
            std::process::exit(-1);
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(-1);
        }
    }
}
