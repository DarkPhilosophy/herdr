/// JSON [`crate::protocol::endpoint::SessionEnvPayload`] handed from the `--remote` launcher to
/// its spawned client process.
pub(crate) const SESSION_ENV_ENV_VAR: &str = "HERDR_REMOTE_SESSION_ENV";
pub(crate) const REATTACH_COMMAND_ENV_VAR: &str = "HERDR_REATTACH_COMMAND";
pub(crate) const REMOTE_KEYBINDINGS_ENV_VAR: &str = "HERDR_REMOTE_KEYBINDINGS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteKeybindings {
    Local,
    Server,
}

impl RemoteKeybindings {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "local" => Ok(Self::Local),
            "server" => Ok(Self::Server),
            _ => Err("--remote-keybindings must be 'local' or 'server'".to_string()),
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Server => "server",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteLaunch {
    pub(crate) target: String,
    pub(crate) keybindings: RemoteKeybindings,
    pub(crate) live_handoff: bool,
    /// Session-scoped `--token`/`--env` values forwarded to the remote server for this
    /// connection only.
    pub(crate) session_env: crate::protocol::endpoint::SessionEnvPayload,
}

pub(crate) fn extract_remote_args(
    args: &[String],
) -> Result<(Vec<String>, Option<RemoteLaunch>), String> {
    let mut cleaned = Vec::with_capacity(args.len());
    if let Some(program) = args.first() {
        cleaned.push(program.clone());
    }

    let mut remote_target = None;
    let mut keybindings = RemoteKeybindings::Local;
    let mut keybindings_seen = false;
    let mut live_handoff = false;
    // `--token`/`--env` are remote-launch options only when `--remote` is present; subcommands
    // such as `pane run`, `tab create`, `workspace create` and `plugin` own their own flags.
    let remote_requested = args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--remote" || arg.starts_with("--remote="));
    let mut tokens: Vec<(String, String)> = Vec::new();
    let mut env: Vec<(String, String)> = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            cleaned.extend_from_slice(&args[index..]);
            break;
        }
        if arg == "--handoff" {
            live_handoff = true;
            index += 1;
            continue;
        }
        if arg == "--remote" {
            if remote_target.is_some() {
                return Err("--remote can only be specified once".to_string());
            }
            let Some(value) = args.get(index + 1) else {
                return Err("missing value for --remote".to_string());
            };
            remote_target = Some(validate_remote_target(value)?.to_owned());
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--remote=") {
            if remote_target.is_some() {
                return Err("--remote can only be specified once".to_string());
            }
            remote_target = Some(validate_remote_target(value)?.to_owned());
            index += 1;
            continue;
        }
        if arg == "--remote-keybindings" {
            if keybindings_seen {
                return Err("--remote-keybindings can only be specified once".to_string());
            }
            let Some(value) = args.get(index + 1) else {
                return Err("missing value for --remote-keybindings".to_string());
            };
            keybindings = RemoteKeybindings::parse(value)?;
            keybindings_seen = true;
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--remote-keybindings=") {
            if keybindings_seen {
                return Err("--remote-keybindings can only be specified once".to_string());
            }
            keybindings = RemoteKeybindings::parse(value)?;
            keybindings_seen = true;
            index += 1;
            continue;
        }
        if remote_requested {
            let token_inline = arg.strip_prefix("--token=");
            if arg == "--token" || token_inline.is_some() {
                let value = match token_inline {
                    Some(value) => value,
                    None => args
                        .get(index + 1)
                        .map(String::as_str)
                        .ok_or_else(|| "missing value for --token".to_string())?,
                };
                tokens.extend(parse_session_token_spec(value)?);
                index += if token_inline.is_some() { 1 } else { 2 };
                continue;
            }
            let env_inline = arg.strip_prefix("--env=");
            if arg == "--env" || env_inline.is_some() {
                let value = match env_inline {
                    Some(value) => value,
                    None => args
                        .get(index + 1)
                        .map(String::as_str)
                        .ok_or_else(|| "missing value for --env".to_string())?,
                };
                env.push(parse_session_assignment("--env", value)?);
                index += if env_inline.is_some() { 1 } else { 2 };
                continue;
            }
        }

        cleaned.push(arg.clone());
        index += 1;
    }

    let remote = remote_target.map(|target| RemoteLaunch {
        target,
        keybindings,
        live_handoff,
        session_env: crate::protocol::endpoint::SessionEnvPayload {
            tokens: tokens
                .into_iter()
                .map(|(name, value)| (name, crate::protocol::SecretString::new(value)))
                .collect(),
            env,
        },
    });
    if remote.is_none() && keybindings_seen {
        return Err("--remote-keybindings requires --remote".to_string());
    }
    if remote.is_none() && live_handoff {
        cleaned.push("--handoff".to_string());
    }

    Ok((cleaned, remote))
}

/// `--token` accepts a literal `NAME=VALUE`, or a path to a file with one `NAME=VALUE` per line
/// (blank and `#` lines ignored).
fn parse_session_token_spec(value: &str) -> Result<Vec<(String, String)>, String> {
    if value.is_empty() {
        return Err("missing value for --token".to_string());
    }
    if value.contains('=') {
        return Ok(vec![parse_session_assignment("--token", value)?]);
    }
    let content = std::fs::read_to_string(value).map_err(|err| {
        format!("--token '{value}' is neither NAME=VALUE nor a readable file: {err}")
    })?;
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            parse_session_assignment("--token", line)
                .map_err(|_| format!("--token file '{value}' has an invalid NAME=VALUE line"))
        })
        .collect()
}

fn parse_session_assignment(flag: &str, value: &str) -> Result<(String, String), String> {
    let Some((name, value)) = value.split_once('=') else {
        return Err(format!("{flag} must use NAME=VALUE"));
    };
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("{flag} name must not be empty"));
    }
    Ok((name.to_string(), value.to_string()))
}

pub(crate) fn validate_remote_target(target: &str) -> Result<&str, String> {
    if target.is_empty() {
        return Err("missing value for --remote".to_string());
    }
    if target.starts_with('-') {
        return Err("--remote target must not start with '-'".to_string());
    }
    Ok(target)
}
