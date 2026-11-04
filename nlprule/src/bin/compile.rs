use clap::{Arg, Command};
use fs_err as fs;
use nlprule::compile::{compile, Error};
use std::io::BufWriter;
use std::path::PathBuf;

fn main() -> Result<(), Error> {
    env_logger::init();
    let matches = Command::new("compile")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Compiles an nlprule build directory into tokenizer and rules binaries.")
        .arg(
            Arg::new("build-dir")
                .long("build-dir")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("tokenizer-out")
                .long("tokenizer-out")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("rules-out")
                .long("rules-out")
                .takes_value(true)
                .required(true),
        )
        .get_matches();
    let path = |name: &str| PathBuf::from(matches.value_of(name).expect("required"));

    let tokenizer_sink = BufWriter::new(fs::File::create(path("tokenizer-out"))?);
    let rules_sink = BufWriter::new(fs::File::create(path("rules-out"))?);

    compile(path("build-dir"), rules_sink, tokenizer_sink)
}
