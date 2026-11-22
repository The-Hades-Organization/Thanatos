//! Fonte externa do Thanatos (`src/lib/thanatos-source.ts`; contrato em `reference/thanatos.md`): o plugin do projeto
//! declara em `plugin.json → commands[]` um comando `thanatos-source`; a Hades o chama com o intervalo e lê JSON (logs,
//! métricas). Sem plugin, sem comando, sem token ou com erro, o post-mortem sai sem a seção e a lacuna diz o que faltou.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hades_core::env::Env;
use hades_core::fsx::{is_file, read_text, realpath_missing_ok, resolve};
use hades_core::journal::mask_secrets;
use hades_core::proc::which;
use serde_json::{Map, Value};

use crate::jsdate::to_utc;
use crate::model::{SourceEvent, SourceMetric, SourceResult, SourceStatus};

/// `SOURCE_COMMAND` (`src/lib/thanatos-source.ts:13`).
pub const SOURCE_COMMAND: &str = "thanatos-source";
/// `SOURCE_SCHEMA_VERSION` (`src/lib/thanatos-source.ts:14`).
pub const SOURCE_SCHEMA_VERSION: f64 = 1.0;
/// `TOKEN_ENV` (`src/lib/thanatos-source.ts:15`).
pub const TOKEN_ENV: &str = "HADES_THANATOS_TOKEN";
/// Variável que escolhe o Bun que roda fonte `.ts` (no TS o runner é o próprio Bun em execução).
pub const BUN_ENV: &str = "HADES_BUN";
const EVENTS_MAX: usize = 500;
const METRICS_MAX: usize = 200;
const TEXT_MAX: usize = 500;
const LEVEL_MAX: usize = 16;
const NAME_MAX: usize = 80;
const UNIT_MAX: usize = 16;
const STDOUT_MAX_BYTES: usize = 4 * 1024 * 1024;
const NOTE_MAX: usize = 200;

/// `SourceCommand` (`src/lib/thanatos-source.ts:25`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCommand {
    pub script: PathBuf,
    pub token_key: Option<String>,
}

/// `clip` (`src/lib/thanatos-source.ts:30`): corta em `max` unidades UTF-16 com `…` no fim.
fn clip(text: &str, max: usize) -> String {
    if text.encode_utf16().count() <= max {
        return text.to_owned();
    }
    format!("{}…", crate::jsstr::slice_utf16(text, max - 1))
}

fn gap(status: SourceStatus, note: String) -> SourceResult {
    SourceResult {
        status,
        note,
        events: Vec::new(),
        metrics: Vec::new(),
        start: None,
    }
}

/// `script.slice(script.lastIndexOf("."))`: o trecho a partir do último ponto do caminho inteiro.
fn extension(script: &str) -> &str {
    script.rfind('.').map_or("", |i| &script[i..])
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Runner {
    Bash,
    Bun,
}

fn runner_of(script: &str) -> Option<Runner> {
    match extension(script) {
        ".sh" => Some(Runner::Bash),
        ".ts" => Some(Runner::Bun),
        _ => None,
    }
}

/// `/^[A-Z][A-Z0-9_]{1,63}$/`.
fn is_token_key(text: &str) -> bool {
    let b = text.as_bytes();
    (2..=64).contains(&b.len())
        && b[0].is_ascii_uppercase()
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

/// `declaredSource` (`src/lib/thanatos-source.ts:35`): o `thanatos-source` do `plugin.json` da pasta `env/` do pacote,
/// com o script resolvido dentro dela; ou o motivo da ausência.
pub fn declared_source(env_dir: Option<&Path>) -> Result<SourceCommand, String> {
    let Some(env_dir) = env_dir else {
        return Err("o projeto não tem plugin com pasta env/".to_owned());
    };
    let doc = read_text(&env_dir.join("plugin.json"))
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .ok_or_else(|| "o plugin não traz plugin.json legível".to_owned())?;
    let entry = doc
        .get("commands")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter().find_map(|c| {
                let c = c.as_object()?;
                (c.get("name").and_then(Value::as_str) == Some(SOURCE_COMMAND)).then_some(c)
            })
        })
        .ok_or_else(|| {
            format!("o plugin não declara o comando {SOURCE_COMMAND} em plugin.json → commands[]")
        })?;
    let rel = match entry.get("script").and_then(Value::as_str) {
        Some(rel) if !rel.is_empty() && !rel.starts_with('/') => rel,
        _ => {
            return Err(format!(
                "{SOURCE_COMMAND}: \"script\" precisa ser um caminho relativo ao env/"
            ));
        }
    };
    let script = realpath_missing_ok(&resolve(&env_dir.join(rel)));
    let root = realpath_missing_ok(env_dir);
    let inside = script
        .to_string_lossy()
        .starts_with(&format!("{}/", root.to_string_lossy()));
    if !inside || !is_file(&script) {
        return Err(format!(
            "{SOURCE_COMMAND}: script fora do env/ ou inexistente ({})",
            clip(rel, NOTE_MAX)
        ));
    }
    if runner_of(&script.to_string_lossy()).is_none() {
        return Err(format!("{SOURCE_COMMAND}: só scripts .sh e .ts"));
    }
    let token_key = entry
        .get("tokenKey")
        .and_then(Value::as_str)
        .filter(|k| is_token_key(k))
        .map(str::to_owned);
    Ok(SourceCommand { script, token_key })
}

fn credential_line() -> regress::Regex {
    regress::Regex::new(r#"^\s*(?:[-*]\s*)?`?([A-Z][A-Z0-9_]*)`?\s*[=:]\s*(.*?)\s*$"#)
        .expect("regex fixa")
}

/// `tokenFromCredentials` (`src/lib/thanatos-source.ts:50`): `CHAVE=valor` (ou `CHAVE: valor`, com marcador de lista
/// opcional) de `credenciais.md`; nunca vai ao log nem ao post-mortem.
pub fn token_from_credentials(text: Option<&str>, key: &str) -> Option<String> {
    let rx = credential_line();
    for line in text.unwrap_or("").split('\n') {
        let Some(m) = rx.find(line) else { continue };
        let (Some(name), Some(value)) = (m.group(1), m.group(2)) else {
            continue;
        };
        if &line[name] == key && !line[value.clone()].is_empty() {
            let value = &line[value];
            let quote = |c: char| matches!(c, '"' | '\'' | '`');
            let value = value.strip_prefix(quote).unwrap_or(value);
            let value = value.strip_suffix(quote).unwrap_or(value);
            return Some(value.to_owned());
        }
    }
    None
}

/// O que a fonte devolveu, sem `status` e `note`.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceOutput {
    pub events: Vec<SourceEvent>,
    pub metrics: Vec<SourceMetric>,
    pub start: Option<String>,
}

fn string_of<'a>(m: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    m.get(key).and_then(Value::as_str)
}

/// `parseSourceOutput` (`src/lib/thanatos-source.ts:59`): valida a saída (`{v, events[], metrics[], start?}`); item
/// ruim é descartado; documento inteiro ruim vira `None`.
pub fn parse_source_output(stdout: &str) -> Option<SourceOutput> {
    let doc: Value = serde_json::from_str(stdout).ok()?;
    let doc = doc.as_object()?;
    let v = doc.get("v").filter(|v| v.is_number())?.as_f64()?;
    if v < SOURCE_SCHEMA_VERSION {
        return None;
    }
    let items = |key: &str| -> Vec<Value> {
        doc.get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let mut events = Vec::new();
    for e in items("events") {
        let Some(e) = e.as_object() else { continue };
        let (Some(ts), Some(text)) = (string_of(e, "ts"), string_of(e, "text")) else {
            continue;
        };
        if events.len() >= EVENTS_MAX {
            continue;
        }
        let Some(ts) = to_utc(ts) else { continue };
        events.push(SourceEvent {
            ts,
            text: mask_secrets(&clip(text, TEXT_MAX)),
            level: string_of(e, "level").map(|l| clip(l, LEVEL_MAX)),
        });
    }
    let mut metrics = Vec::new();
    for m in items("metrics") {
        let Some(m) = m.as_object() else { continue };
        let (Some(name), Some(value)) = (
            string_of(m, "name"),
            m.get("value")
                .filter(|v| v.is_number())
                .and_then(Value::as_f64),
        ) else {
            continue;
        };
        if !value.is_finite() || metrics.len() >= METRICS_MAX {
            continue;
        }
        metrics.push(SourceMetric {
            name: clip(name, NAME_MAX),
            value,
            unit: string_of(m, "unit").map(|u| clip(u, UNIT_MAX)),
            ts: string_of(m, "ts").and_then(to_utc),
        });
    }
    let start = string_of(doc, "start").and_then(to_utc);
    Some(SourceOutput {
        events,
        metrics,
        start,
    })
}

/// `SourceDeps` (`src/lib/thanatos-source.ts:92`).
pub struct SourceDeps<'a> {
    pub env_dir: Option<&'a Path>,
    /// Conteúdo de `$D/credenciais.md`, quando há.
    pub credentials: Option<&'a str>,
    pub timeout_s: u32,
    /// O ambiente do processo: `PATH`, `HOME` e `HADES_BUN`.
    pub env: &'a Env,
}

/// O Bun que roda fonte `.ts`: `HADES_BUN`, senão `bun` do `PATH`.
fn bun_of(env: &Env) -> Option<String> {
    if let Some(path) = env.get(BUN_ENV) {
        return Some(path.to_owned());
    }
    which("bun", env.get_raw("PATH").unwrap_or("")).map(|p| p.to_string_lossy().into_owned())
}

/// `fetchSource` (`src/lib/thanatos-source.ts:97`): roda o `thanatos-source` para o intervalo (UTC). Nunca falha: o
/// que faltou vira o `status` e a `note`.
pub fn fetch_source(from: &str, to: &str, deps: &SourceDeps<'_>) -> SourceResult {
    use crate::proc::{EnvMode, Outcome, SpawnOptions, spawn};

    let command = match declared_source(deps.env_dir) {
        Ok(command) => command,
        Err(why) => return gap(SourceStatus::Absent, why),
    };
    let SourceCommand { script, token_key } = command;
    let mut vars = vec![
        (
            "PATH".to_owned(),
            deps.env.get_raw("PATH").unwrap_or("").to_owned(),
        ),
        (
            "HOME".to_owned(),
            deps.env.get_raw("HOME").unwrap_or("").to_owned(),
        ),
        ("LANG".to_owned(), "C.UTF-8".to_owned()),
        ("TZ".to_owned(), "UTC".to_owned()),
    ];
    if let Some(key) = token_key {
        let Some(token) = token_from_credentials(deps.credentials, &key) else {
            return gap(
                SourceStatus::NoToken,
                format!("{SOURCE_COMMAND} pede {key} e ele não está em credenciais.md"),
            );
        };
        vars.push((TOKEN_ENV.to_owned(), token));
    }
    let script_text = script.to_string_lossy().into_owned();
    let (program, args): (String, Vec<&str>) = match runner_of(&script_text) {
        Some(Runner::Bash) => ("bash".to_owned(), vec![&script_text, from, to]),
        _ => match bun_of(deps.env) {
            Some(bun) => (bun, vec![&script_text, from, to]),
            None => {
                return gap(
                    SourceStatus::Error,
                    format!(
                        "{SOURCE_COMMAND} é um script .ts e o Bun não foi encontrado (PATH ou {BUN_ENV})"
                    ),
                );
            }
        },
    };
    let options = SpawnOptions {
        cwd: script.parent().map(Path::to_path_buf),
        timeout: Some(Duration::from_secs(u64::from(deps.timeout_s))),
        max_buffer: Some(STDOUT_MAX_BYTES),
        env: EnvMode::Clear(vars),
    };
    match spawn(&program, &args, &options) {
        Outcome::TimedOut => gap(
            SourceStatus::Error,
            format!("{SOURCE_COMMAND} passou de {}s", deps.timeout_s),
        ),
        Outcome::Overflow => gap(
            SourceStatus::Error,
            format!("{SOURCE_COMMAND} não executou: saída acima de 4 MiB"),
        ),
        Outcome::Failed(why) => gap(
            SourceStatus::Error,
            format!(
                "{SOURCE_COMMAND} não executou: {}",
                clip(&mask_secrets(&why), NOTE_MAX)
            ),
        ),
        Outcome::Done {
            status,
            stdout,
            stderr,
        } => {
            if status != 0 {
                let last = crate::jsstr::trim(&stderr)
                    .rsplit('\n')
                    .next()
                    .unwrap_or("");
                let why = mask_secrets(last);
                let tail = if why.is_empty() {
                    String::new()
                } else {
                    format!(": {}", clip(&why, NOTE_MAX))
                };
                return gap(
                    SourceStatus::Error,
                    format!("{SOURCE_COMMAND} saiu com {status}{tail}"),
                );
            }
            match parse_source_output(&stdout) {
                None => gap(
                    SourceStatus::Error,
                    format!(
                        "{SOURCE_COMMAND} não devolveu o JSON do contrato (v>={}, events[], metrics[])",
                        SOURCE_SCHEMA_VERSION as u32
                    ),
                ),
                Some(parsed) => SourceResult {
                    status: SourceStatus::Ok,
                    note: format!(
                        "{} evento(s) e {} métrica(s) da fonte",
                        parsed.events.len(),
                        parsed.metrics.len()
                    ),
                    events: parsed.events,
                    metrics: parsed.metrics,
                    start: parsed.start,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests;
