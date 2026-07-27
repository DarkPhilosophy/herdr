const API_SCHEMA_JSON: &str = include_str!("../../docs/next/api/herdr-api.schema.json");

use crate::api::schema::{EmptyParams, Method, Request};

pub(super) fn run_api_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        print_api_help();
        return Ok(2);
    };

    match subcommand {
        "schema" => api_schema(&args[1..]),
        "snapshot" => api_snapshot(&args[1..]),
        "session-env" => api_session_env(&args[1..]),
        "help" | "--help" | "-h" => {
            print_api_help();
            Ok(0)
        }
        _ => {
            print_api_help();
            Ok(2)
        }
    }
}

fn api_schema(args: &[String]) -> std::io::Result<i32> {
    match args {
        [] => {
            print!("{}", schema_summary_text()?);
        }
        [flag] if flag == "--json" => {
            print!("{API_SCHEMA_JSON}");
        }
        [flag, path] if flag == "--output" => {
            write_schema_file(std::path::Path::new(path))?;
            println!("wrote API schema to {path}");
        }
        [flag] if flag == "--output" => {
            eprintln!("missing value for --output");
            return Ok(2);
        }
        [flag] if matches!(flag.as_str(), "help" | "--help" | "-h") => {
            print_api_schema_help();
        }
        [other] if other.starts_with('-') => {
            eprintln!("unknown option: {other}");
            return Ok(2);
        }
        _ => {
            print_api_schema_help();
            return Ok(2);
        }
    }
    Ok(0)
}

fn api_snapshot(args: &[String]) -> std::io::Result<i32> {
    if !args.is_empty() {
        eprintln!("usage: herdr api snapshot");
        return Ok(2);
    }

    super::print_response(&super::send_request(&Request {
        id: "cli:api:snapshot".into(),
        method: Method::SessionSnapshot(EmptyParams::default()),
    })?)
}

fn write_schema_file(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::write(path, API_SCHEMA_JSON)
}

fn schema_summary_text() -> std::io::Result<String> {
    let value: serde_json::Value = serde_json::from_str(API_SCHEMA_JSON)?;
    let protocol = value
        .get("protocol")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| std::io::Error::other("API schema is missing protocol"))?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| std::io::Error::other("API schema is missing schema_version"))?;
    let mut schemas: Vec<&str> = value
        .get("schemas")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| std::io::Error::other("API schema is missing schemas"))?
        .keys()
        .map(String::as_str)
        .collect();
    schemas.sort();

    Ok(format!(
        "Herdr API schema\nprotocol: {}\nschema_version: {}\nschemas: {}\n\nUse `herdr api schema --json` to print the full schema.\nUse `herdr api schema --output PATH` to write it to a file.\n",
        protocol,
        schema_version,
        schemas.join(", ")
    ))
}

fn print_api_help() {
    eprintln!("herdr api commands:");
    eprintln!("  herdr api snapshot");
    eprintln!("  herdr api schema [--json | --output PATH]");
}

fn print_api_schema_help() {
    eprintln!("usage: herdr api schema [--json | --output PATH]");
}

#[cfg(test)]
mod tests {
    #[test]
    fn schema_summary_text_stays_human_sized() {
        let text = super::schema_summary_text().unwrap();
        assert!(text.contains("Herdr API schema"));
        assert!(text.contains("Use `herdr api schema --json`"));
        assert!(text.len() < 400);
    }
}

/// `herdr api session-env` — returns the session-scoped tokens/env forwarded by
/// the most recently active client (see `--token`/`--env` on `herdr --remote`).
/// This is the controlled egress plugins use to read forwarded secrets (e.g.
/// a decryption key) without those secrets ever touching disk or logs.
///
/// Default output is line-oriented for easy parsing by plugins:
///   `TOKEN NAME=VALUE` for each forwarded secret
///   `ENV NAME=VALUE`   for each plain env override
/// Pass `--json` (or `--format=json`) to emit the raw envelope instead.
fn api_session_env(args: &[String]) -> std::io::Result<i32> {
    let response = super::send_request(&Request {
        id: "cli:api:session-env".into(),
        method: Method::SessionEnv(EmptyParams::default()),
    })?;
    let as_json = args.iter().any(|a| a == "--json" || a == "--format=json");
    if as_json || response.get("error").is_some() {
        return super::print_response(&response);
    }
    let Some(result) = response.get("result") else {
        return super::print_response(&response);
    };
    if let Some(tokens) = result.get("tokens").and_then(|v| v.as_array()) {
        for pair in tokens {
            let key = pair.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let val = pair.get(1).and_then(|v| v.as_str()).unwrap_or("");
            println!("TOKEN {key}={val}");
        }
    }
    if let Some(env) = result.get("env").and_then(|v| v.as_array()) {
        for pair in env {
            let key = pair.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let val = pair.get(1).and_then(|v| v.as_str()).unwrap_or("");
            println!("ENV {key}={val}");
        }
    }
    Ok(0)
}
