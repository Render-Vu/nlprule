use clap::{Arg, Command};
use nlprule::tokenizer::Tokenizer;

fn main() {
    env_logger::init();
    let matches = Command::new("test_disambiguation")
        .about("Runs the examples of the disambiguation rules as tests.")
        .arg(Arg::new("stop-at-error").long("stop-at-error"))
        .arg(
            Arg::new("tokenizer")
                .long("tokenizer")
                .short('t')
                .takes_value(true)
                .required(true),
        )
        .get_matches();
    let stop_at_error = matches.is_present("stop-at-error");

    let tokenizer = Tokenizer::new(matches.value_of("tokenizer").unwrap()).unwrap();
    let rules = tokenizer.rules();

    println!("Last ID: {}", rules[rules.len() - 1].id());
    println!("Runnable rules: {}", rules.len());

    let mut passes = 0;
    for rule in rules {
        if rule.test(&tokenizer) {
            passes += 1;
        } else if stop_at_error {
            break;
        }
    }

    println!("Rules passing tests: {}", passes);
    if passes == rules.len() {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}
