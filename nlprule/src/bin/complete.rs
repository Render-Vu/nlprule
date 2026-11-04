//! Interactive test tool for grammar-aware completion.
//!
//! ```bash
//! cargo run --release --features bin --bin complete -- --tokenizer storage/es_tokenizer.bin "Los niños jue"
//! ```
//!
//! Without a text, reads one text per line from stdin.

use clap::{Arg, Command};
use nlprule::{
    complete::{Completer, CompletionOptions, Frequencies},
    Rules, Tokenizer,
};
use std::{
    io::{self, BufRead, Write},
    time::Instant,
};

struct Opts {
    text: Option<String>,
    tokenizer: String,
    rules: Option<String>,
    freq: Option<String>,
    tag: Option<String>,
    no_context: bool,
    validate: bool,
    limit: usize,
    json: bool,
}

impl Opts {
    fn parse() -> Self {
        let matches = Command::new("complete")
            .about("Completes the last (partial) word of a text.")
            .arg(Arg::new("text").help(
                "The text up to the cursor. If omitted, texts are read line by line from stdin.",
            ))
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
                    .help("Rules binary, needed for --validate."),
            )
            .arg(
                Arg::new("freq")
                    .long("freq")
                    .short('f')
                    .takes_value(true)
                    .help("Word frequency list: one `word count` pair per line."),
            )
            .arg(Arg::new("tag").long("tag").takes_value(true).help(
                "Only return forms with this tag prefix, e.g. VMIP3P (`?` is a wildcard). Disables context inference.",
            ))
            .arg(
                Arg::new("no-context")
                    .long("no-context")
                    .help("Do not infer the expected tags from the context."),
            )
            .arg(Arg::new("validate").long("validate").help(
                "Discard candidates for which the rules report an error (requires --rules).",
            ))
            .arg(
                Arg::new("limit")
                    .long("limit")
                    .short('l')
                    .takes_value(true)
                    .default_value("10"),
            )
            .arg(Arg::new("json").long("json").help("Print the result as JSON."))
            .get_matches();

        let string = |name: &str| matches.value_of(name).map(str::to_string);

        Opts {
            text: string("text"),
            tokenizer: string("tokenizer").expect("required"),
            rules: string("rules"),
            freq: string("freq"),
            tag: string("tag"),
            no_context: matches.is_present("no-context"),
            validate: matches.is_present("validate"),
            limit: matches
                .value_of("limit")
                .and_then(|l| l.parse().ok())
                .expect("--limit must be a number"),
            json: matches.is_present("json"),
        }
    }
}

fn main() {
    env_logger::init();
    let opts = Opts::parse();

    let start = Instant::now();
    let tokenizer = Tokenizer::new(&opts.tokenizer).expect("can load tokenizer");
    let rules = opts
        .rules
        .as_ref()
        .map(|path| Rules::new(path).expect("can load rules"));
    let freq = opts
        .freq
        .as_ref()
        .map(|path| Frequencies::from_path(path).expect("can load frequencies"));
    eprintln!("loaded in {:.2?}", start.elapsed());

    if opts.validate && rules.is_none() {
        eprintln!("--validate requires --rules");
        std::process::exit(2);
    }

    let mut completer = Completer::new(&tokenizer);
    if let Some(rules) = &rules {
        completer = completer.with_rules(rules);
    }
    if let Some(freq) = &freq {
        completer = completer.with_frequencies(freq);
    }

    let options = CompletionOptions {
        limit: opts.limit,
        tag_filter: opts.tag.clone(),
        use_context: !opts.no_context,
        validate: opts.validate,
        ..CompletionOptions::default()
    };

    let run = |text: &str| {
        let start = Instant::now();
        let result = completer.complete(text, &options);
        let elapsed = start.elapsed();

        if opts.json {
            println!("{}", serde_json::to_string(&result).expect("serializable"));
            return;
        }

        println!(
            "prefix {:?} | expected {:?} | boosted {:?} | {:.2?}",
            result.prefix, result.context.expected, result.context.boosted, elapsed
        );
        for item in &result.items {
            println!(
                "  {:<20} {:<30} {}{}",
                item.word,
                item.tags.join(","),
                if item.matches_context { "✓ " } else { "  " },
                if item.frequency > 0 {
                    format!("freq={}", item.frequency)
                } else {
                    String::new()
                }
            );
        }
    };

    match &opts.text {
        Some(text) => run(text),
        None => {
            let stdin = io::stdin();
            print!("> ");
            io::stdout().flush().ok();
            for line in stdin.lock().lines() {
                run(&line.expect("can read stdin"));
                print!("> ");
                io::stdout().flush().ok();
            }
        }
    }
}
