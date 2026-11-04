//! HTTP server exposing nlprule as
//! - `/v2/check`: a subset of the LanguageTool HTTP API, so LanguageTool clients (e.g. flycheck-languagetool,
//!   langtool.el) can use nlprule by changing only the server URL;
//! - `/complete`: grammar-aware word completion.

mod check;

use clap::{Arg, Command};
use log::{error, info};
use nlprule::{
    complete::{Completer, CompletionOptions, Frequencies},
    Rules, Tokenizer,
};
use serde_json::{json, Value};
use std::{collections::HashMap, io::Read, sync::Arc, time::Instant};
use tiny_http::{Header, Method, Request, Response, Server};

/// The loaded resources, shared by all worker threads.
pub struct State {
    pub tokenizer: Tokenizer,
    pub rules: Rules,
    pub frequencies: Option<Frequencies>,
    pub language: String,
}

type Params = HashMap<String, String>;

struct HttpError(u16, String);

fn bad_request(message: impl Into<String>) -> HttpError {
    HttpError(400, message.into())
}

/// Collects the query string and (for POST) form-encoded body parameters.
fn read_params(request: &mut Request) -> Result<Params, HttpError> {
    let mut params = Params::new();

    if let Some((_, query)) = request.url().split_once('?') {
        params.extend(form_urlencoded::parse(query.as_bytes()).into_owned());
    }

    if *request.method() == Method::Post {
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .map_err(|e| bad_request(format!("cannot read body: {}", e)))?;

        let is_json = request.headers().iter().any(|h| {
            h.field.equiv("Content-Type") && h.value.as_str().starts_with("application/json")
        });

        if is_json {
            let value: Value = serde_json::from_str(&body)
                .map_err(|e| bad_request(format!("invalid JSON body: {}", e)))?;
            if let Value::Object(map) = value {
                for (key, value) in map {
                    let value = match value {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    params.insert(key, value);
                }
            }
        } else {
            params.extend(form_urlencoded::parse(body.as_bytes()).into_owned());
        }
    }

    Ok(params)
}

fn flag(params: &Params, name: &str, default: bool) -> Result<bool, HttpError> {
    match params.get(name).map(String::as_str) {
        None => Ok(default),
        Some("true") | Some("1") | Some("yes") => Ok(true),
        Some("false") | Some("0") | Some("no") => Ok(false),
        Some(other) => Err(bad_request(format!(
            "invalid value for {}: {}",
            name, other
        ))),
    }
}

fn complete(state: &State, params: &Params) -> Result<Value, HttpError> {
    let text = params
        .get("text")
        .ok_or_else(|| bad_request("missing parameter: text"))?;

    // optional cursor position (in characters); the text after it is ignored
    let text: String = match params.get("cursor") {
        Some(cursor) => {
            let cursor: usize = cursor
                .parse()
                .map_err(|_| bad_request("cursor must be a number"))?;
            text.chars().take(cursor).collect()
        }
        None => text.clone(),
    };

    let mut options = CompletionOptions::default();
    if let Some(limit) = params.get("limit") {
        options.limit = limit
            .parse()
            .map_err(|_| bad_request("limit must be a number"))?;
    }
    options.tag_filter = params.get("tag").filter(|t| !t.is_empty()).cloned();
    options.use_context = flag(params, "context", true)?;
    options.validate = flag(params, "validate", false)?;

    let mut completer = Completer::new(&state.tokenizer).with_rules(&state.rules);
    if let Some(freq) = &state.frequencies {
        completer = completer.with_frequencies(freq);
    }

    serde_json::to_value(completer.complete(&text, &options))
        .map_err(|e| HttpError(500, e.to_string()))
}

fn route(state: &State, request: &mut Request) -> Result<Value, HttpError> {
    let path = request.url().split('?').next().unwrap_or("").to_string();

    match (request.method().clone(), path.as_str()) {
        (Method::Get, "/health") => Ok(json!({ "status": "ok" })),
        (Method::Get, "/v2/languages") => Ok(json!([{
            "name": check::language_name(&state.language),
            "code": state.language,
            "longCode": state.language,
        }])),
        (Method::Get, "/v2/check") | (Method::Post, "/v2/check") => {
            let params = read_params(request)?;
            check::check(state, &params).map_err(bad_request)
        }
        (Method::Get, "/complete") | (Method::Post, "/complete") => {
            let params = read_params(request)?;
            complete(state, &params)
        }
        (_, "/health") | (_, "/v2/languages") | (_, "/v2/check") | (_, "/complete") => {
            Err(HttpError(405, "method not allowed".into()))
        }
        _ => Err(HttpError(404, format!("not found: {}", path))),
    }
}

fn handle(state: &State, mut request: Request) {
    let start = Instant::now();
    let (status, body) = match route(state, &mut request) {
        Ok(value) => (200, value),
        Err(HttpError(status, message)) => (status, json!({ "error": message })),
    };

    info!(
        "{} {} -> {} in {:.2?}",
        request.method(),
        request.url().split('?').next().unwrap_or(""),
        status,
        start.elapsed()
    );

    let response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .expect("valid header"),
        )
        .with_header(
            Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..])
                .expect("valid header"),
        );

    if let Err(e) = request.respond(response) {
        error!("failed to respond: {}", e);
    }
}

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("nlprule_server=info"),
    )
    .init();

    let matches = Command::new("nlprule-server")
        .about("LanguageTool-compatible checking and grammar-aware completion over HTTP.")
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
            Arg::new("freq")
                .long("freq")
                .short('f')
                .takes_value(true)
                .help(
                    "Word frequency list for ranking completions: one `word count` pair per line.",
                ),
        )
        .arg(
            Arg::new("language")
                .long("language")
                .takes_value(true)
                .default_value("es")
                .help("Language code of the binaries."),
        )
        .arg(
            Arg::new("host")
                .long("host")
                .takes_value(true)
                .default_value("127.0.0.1"),
        )
        .arg(
            Arg::new("port")
                .long("port")
                .short('p')
                .takes_value(true)
                .default_value("8011"),
        )
        .arg(
            Arg::new("threads")
                .long("threads")
                .takes_value(true)
                .default_value("4"),
        )
        .get_matches();

    let value = |name: &str| matches.value_of(name).expect("has default or is required");
    let threads: usize = value("threads")
        .parse()
        .expect("--threads must be a number");

    let start = Instant::now();
    let state = Arc::new(State {
        tokenizer: Tokenizer::new(value("tokenizer")).expect("can load tokenizer"),
        rules: Rules::new(value("rules")).expect("can load rules"),
        frequencies: matches
            .value_of("freq")
            .map(|path| Frequencies::from_path(path).expect("can load frequencies")),
        language: value("language").to_string(),
    });
    info!("loaded in {:.2?}", start.elapsed());

    let address = format!("{}:{}", value("host"), value("port"));
    let server = Arc::new(Server::http(&address).expect("can bind address"));
    info!("listening on http://{}", address);

    let workers: Vec<_> = (0..threads.max(1))
        .map(|_| {
            let server = Arc::clone(&server);
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                for request in server.incoming_requests() {
                    handle(&state, request);
                }
            })
        })
        .collect();

    for worker in workers {
        worker.join().ok();
    }
}
