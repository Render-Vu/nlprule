use clap::{Arg, Command};
use nlprule::{rules::Rules, tokenizer::Tokenizer};

fn main() {
    env_logger::init();
    let matches = Command::new("test")
        .about("Runs the examples of the grammar rules as tests.")
        .arg(
            Arg::new("tokenizer")
                .long("tokenizer")
                .short('t')
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("rules")
                .long("rules")
                .short('r')
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("ids")
                .long("ids")
                .short('i')
                .takes_value(true)
                .multiple_occurrences(true)
                .help("Only test these rule ids (full ids, e.g. GRAMMAR/WAS_BEEN/1)."),
        )
        .get_matches();
    let ids: Vec<String> = matches
        .values_of("ids")
        .map(|v| v.map(str::to_string).collect())
        .unwrap_or_default();

    let tokenizer = Tokenizer::new(matches.value_of("tokenizer").unwrap()).unwrap();
    let rules_container = Rules::new(matches.value_of("rules").unwrap()).unwrap();
    let rules = rules_container.rules();

    println!("Runnable rules: {}", rules.len());

    let mut passes = 0;
    for rule in rules {
        if ids.is_empty() || ids.contains(&rule.id().to_string()) {
            passes += rule.test(&tokenizer) as usize;
        }
    }

    println!("Rules passing tests: {}", passes);
    if passes == rules.len() {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}
