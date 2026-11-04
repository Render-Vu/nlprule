use clap::{Arg, Command};
use nlprule::{rules::Rules, tokenizer::Tokenizer};

fn main() {
    env_logger::init();
    let matches = Command::new("run")
        .about("Prints the tokens and suggestions for a text.")
        .arg(Arg::new("text").required(true))
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
        .get_matches();
    let text = matches.value_of("text").unwrap();

    let tokenizer = Tokenizer::new(matches.value_of("tokenizer").unwrap()).unwrap();
    let rules = Rules::new(matches.value_of("rules").unwrap()).unwrap();

    let tokens = tokenizer.pipe(text);

    println!("Tokens: {:#?}", tokens.collect::<Vec<_>>());
    println!("Suggestions: {:#?}", rules.suggest(text, &tokenizer));
}
