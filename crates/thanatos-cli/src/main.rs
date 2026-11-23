//! `thanatos`: argumentos, contexto e impressão; a regra mora no `thanatos-core` (`src/commands/thanatos.ts`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use hades_core::Env;
use hades_core::clock::now_ms;
use hades_core::errors::render_error;
use hades_core::fsx::read_text;
use hades_core::jsonfmt::json_compact;
use hades_core::output::{HadesError, exit, out, warn};
use hades_core::paths::{find_state_dir, work_root};
use hades_core::plugins::env_dir;
use hades_core::profile::load_profile;
use hades_core::project::{pack_of, resolve_project};
use hades_core::work::{WorkOptions, load_work_env};
use serde_json::{Value, json};
use thanatos_core::build::build_postmortem;
use thanatos_core::gather::{
    Exec, GatherError, GatherOptions, SystemExec, gather, journal_facts, window_of,
};
use thanatos_core::input::{id_of, parse_input};
use thanatos_core::jsstr::trim;
use thanatos_core::model::{SourceResult, SourceStatus};
use thanatos_core::render::format_duration;
use thanatos_core::settings::{self, Settings};
use thanatos_core::source::{SourceDeps, fetch_source};
use thanatos_core::store::{
    cited_files, exists, list_postmortems, read_markdown, read_raw, save_postmortem,
    search_postmortems,
};

/// `USAGE` (`src/commands/thanatos.ts:21`).
const USAGE: &str = "Post-mortem de incidente e bug em produção do projeto (reference/thanatos.md).
Uso: hades thanatos new <card|PR|<iso>..<iso>> [--force] [--json]
     hades thanatos list [--json]
     hades thanatos show <id> [--json]
     hades thanatos search <termo> [--json]
     hades thanatos cited [--base <ref> | --pr <n> | <arquivo>…] [--json]
  new     monta o post-mortem de git + PRs + reviews (+ diário e fonte externa do plugin, quando há) e grava em
          <work>/<owner>__<repo>/postmortems/; recusa sobrescrever sem --force
  cited   avisa quando os arquivos tocados já foram citados em post-mortem anterior (deliver e review; sempre sai 0)";

/// `ThanatosContext` (`src/commands/thanatos.ts:36`).
struct Context {
    now_ms: i64,
    exec: Box<dyn Exec>,
    dir: PathBuf,
    slug: String,
    repo: String,
    card_pattern: Option<String>,
    plugin_env: Option<PathBuf>,
    credentials: Option<String>,
    journal_root: PathBuf,
    settings: Settings,
    env: Env,
}

/// `Options` (`src/commands/thanatos.ts:48`).
#[derive(Default)]
struct Options {
    force: bool,
    json: bool,
    base: Option<String>,
    pr: Option<String>,
}

/// `renderCli` do `cli.ts:146`: o TS imprime com o prefixo `hades`, e o binário o substitui no corte.
const ERROR_PREFIX: &str = "hades";

fn fail(message: impl Into<String>) -> HadesError {
    HadesError::with_code(message, exit::FAIL)
}

/// `parseOptions` (`src/commands/thanatos.ts:50`): as opções valem para todo verbo; `--base` e `--pr` levam o próximo
/// argumento, qualquer que seja.
fn parse_options(args: &[String]) -> Result<(Vec<String>, Options), HadesError> {
    let mut options = Options::default();
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--json" => options.json = true,
            "--force" => options.force = true,
            "--base" | "--pr" => {
                let Some(value) = args.get(i + 1) else {
                    return Err(HadesError::new(format!("{arg} exige um valor\n{USAGE}")));
                };
                if arg == "--base" {
                    options.base = Some(value.clone());
                } else {
                    options.pr = Some(value.clone());
                }
                i += 1;
            }
            _ if arg.starts_with("--") => {
                return Err(HadesError::new(format!(
                    "opção desconhecida: {arg}\n{USAGE}"
                )));
            }
            _ => positional.push(arg.to_owned()),
        }
        i += 1;
    }
    Ok((positional, options))
}

fn object<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| v.is_object())
}

/// Conta do gh do perfil sem trocar a conta ativa (`useProjectToken`, `src/commands/thanatos.ts:75`): `GH_TOKEN` e
/// `GITHUB_TOKEN` do ambiente vencem. Devolve o token que vale só no filho do `gh`.
fn project_token(profile: &Value, env: &Env, exec: &dyn Exec) -> Option<String> {
    if env.get("GH_TOKEN").is_some() || env.get("GITHUB_TOKEN").is_some() {
        return None;
    }
    let gh = object(profile, "identity").and_then(|i| object(i, "gh"))?;
    if let Some(token) = gh
        .get("tokenEnv")
        .and_then(Value::as_str)
        .and_then(|name| env.get(name))
    {
        return Some(token.to_owned());
    }
    let user = gh.get("user").and_then(Value::as_str)?;
    let result = exec.run("gh", &["auth", "token", "--user", user]);
    let token = trim(&result.stdout);
    (result.status == 0 && !token.is_empty()).then(|| token.to_owned())
}

/// `realContext` (`src/commands/thanatos.ts:92`).
fn real_context(env: &Env, cwd: &Path) -> Result<Context, HadesError> {
    let work = load_work_env(
        &WorkOptions {
            cwd: Some(cwd.to_path_buf()),
            ..WorkOptions::default()
        },
        env,
    )?;
    let resolution = resolve_project(env, &work.repo_top);
    let profile = if resolution.code == 0 {
        load_profile(&resolution)
    } else {
        json!({})
    };
    let pack = pack_of(&resolution);
    let settings = settings::load(env, cwd);
    let timeout = Duration::from_secs(u64::from(settings.io_timeout_s));
    let plain = SystemExec {
        cwd: work.repo_top.clone(),
        timeout,
        gh_token: None,
    };
    let gh_token = project_token(&profile, env, &plain);
    let card = object(&profile, "card");
    let card_pattern = card
        .filter(|c| c.get("provider").and_then(Value::as_str) != Some("none"))
        .and_then(|c| c.get("idPattern").and_then(Value::as_str))
        .map(str::to_owned);
    let credentials =
        find_state_dir(env, cwd).and_then(|state| read_text(&state.join("credenciais.md")));
    let now = now_ms(env).map_err(|e| fail(e.to_string()))?;
    Ok(Context {
        now_ms: now,
        exec: Box::new(SystemExec { gh_token, ..plain }),
        dir: work.work.join("postmortems"),
        repo: work.slug.replacen("__", "/", 1),
        slug: work.slug,
        card_pattern,
        plugin_env: pack.and_then(|p| env_dir(&p, &profile)),
        credentials,
        journal_root: work_root(env),
        settings,
        env: env.clone(),
    })
}

/// `newPostmortem` (`src/commands/thanatos.ts:119`).
fn new_postmortem(ctx: &Context, text: &str, options: &Options) -> Result<i32, HadesError> {
    let input = parse_input(text, ctx.card_pattern.as_deref())?;
    let id = id_of(&input);
    if exists(&ctx.dir, &id)? && !options.force {
        return Err(fail(format!(
            "post-mortem {id} já existe em {}; use --force para regerar (a edição à mão se perde)",
            ctx.dir.display()
        )));
    }
    let gathered = gather(
        &input,
        &*ctx.exec,
        &GatherOptions {
            repo: ctx.repo.clone(),
            max_prs: ctx.settings.max_prs,
            journal_max: ctx.settings.journal_max,
        },
    )
    .map_err(|GatherError(message)| fail(message))?;
    let window = window_of(&input, &gathered);
    let journal = window.as_ref().map_or_else(Vec::new, |(from, to)| {
        journal_facts(
            &ctx.slug,
            from,
            to,
            ctx.settings.journal_max as usize,
            &ctx.journal_root,
        )
    });
    let source = match &window {
        Some((from, to)) => fetch_source(
            from,
            to,
            &SourceDeps {
                env_dir: ctx.plugin_env.as_deref(),
                credentials: ctx.credentials.as_deref(),
                timeout_s: ctx.settings.source_timeout_s,
                env: &ctx.env,
            },
        ),
        None => SourceResult {
            status: SourceStatus::Absent,
            note: "sem janela de tempo (nenhuma PR mergeada) para consultar a fonte externa".into(),
            events: Vec::new(),
            metrics: Vec::new(),
            start: None,
        },
    };
    let pm = build_postmortem(&input, &gathered, &source, &journal, ctx.now_ms);
    let (md, json_file) = save_postmortem(&ctx.dir, &pm)?;
    if options.json {
        let value = serde_json::to_value(&pm).map_err(|e| fail(e.to_string()))?;
        out(&json_compact(&value));
    } else {
        out(&format!("post-mortem {} gravado", pm.id));
        out(&format!("  {}", md.display()));
        out(&format!("  {}", json_file.display()));
        let ttf = pm
            .fix_data
            .time_to_fix
            .as_ref()
            .map_or_else(|| "n/d".to_owned(), |t| format_duration(t.ms));
        out(&format!(
            "  {} PR(s), {} evento(s), tempo até a correção: {ttf}",
            pm.fix_data.prs.len(),
            pm.timeline.len()
        ));
        for gap in &pm.gaps {
            out(&format!("  lacuna: {gap}"));
        }
    }
    Ok(exit::OK)
}

/// `showOne` (`src/commands/thanatos.ts:150`).
fn show_one(ctx: &Context, id: &str, options: &Options) -> Result<i32, HadesError> {
    let raw = read_raw(&ctx.dir, id)?;
    let md = read_markdown(&ctx.dir, id)?;
    let (Some(raw), Some(md)) = (raw, md) else {
        return Err(fail(format!("post-mortem não encontrado: {id}")));
    };
    if options.json {
        out(&json_compact(&raw));
    } else {
        out(md.strip_suffix('\n').unwrap_or(&md));
    }
    Ok(exit::OK)
}

fn last_line(stderr: &str) -> &str {
    trim(stderr).rsplit('\n').next().unwrap_or("falhou")
}

fn lines(stdout: &str) -> Vec<String> {
    stdout
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `touchedFiles` (`src/commands/thanatos.ts:159`).
fn touched_files(
    ctx: &Context,
    positional: &[String],
    options: &Options,
) -> Result<Vec<String>, HadesError> {
    if let Some(base) = &options.base {
        let range = format!("{base}...HEAD");
        let r = ctx.exec.run("git", &["diff", "--name-only", &range]);
        if r.status != 0 {
            return Err(fail(format!("git diff {range}: {}", last_line(&r.stderr))));
        }
        return Ok(lines(&r.stdout));
    }
    if let Some(pr) = &options.pr {
        if pr.is_empty() || !pr.bytes().all(|b| b.is_ascii_digit()) {
            return Err(HadesError::new(format!("--pr é o número da PR: {pr}")));
        }
        let args = [
            "pr",
            "view",
            pr,
            "-R",
            &ctx.repo,
            "--json",
            "files",
            "--jq",
            ".files[].path",
        ];
        let r = ctx.exec.run("gh", &args);
        if r.status != 0 {
            return Err(fail(format!("gh pr view {pr}: {}", last_line(&r.stderr))));
        }
        return Ok(lines(&r.stdout));
    }
    Ok(positional.to_vec())
}

/// `cited` (`src/commands/thanatos.ts:175`).
fn cited(ctx: &Context, positional: &[String], options: &Options) -> Result<i32, HadesError> {
    let touched = touched_files(ctx, positional, options)?;
    let found = cited_files(&ctx.dir, &touched)?;
    if options.json {
        let value = serde_json::to_value(&found).map_err(|e| fail(e.to_string()))?;
        out(&json_compact(&value));
    } else {
        for c in &found {
            let fix = c
                .merged_at
                .as_ref()
                .map_or_else(String::new, |m| format!(", correção em {m}"));
            out(&format!(
                "AVISO {} já foi citado no post-mortem {} ({}{fix}): leia antes de mexer (hades thanatos show {})",
                c.file, c.id, c.title, c.id
            ));
        }
    }
    Ok(exit::OK)
}

fn list(ctx: &Context, options: &Options) -> Result<i32, HadesError> {
    let items = list_postmortems(&ctx.dir);
    if options.json {
        let value = serde_json::to_value(&items).map_err(|e| fail(e.to_string()))?;
        out(&json_compact(&value));
    } else {
        for i in &items {
            let prs = if i.prs.is_empty() {
                "-".to_owned()
            } else {
                i.prs
                    .iter()
                    .map(|n| format!("#{n}"))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            out(&format!(
                "{}\t{}\t{}\t{prs}\t{}",
                i.id, i.created_at, i.kind, i.title
            ));
        }
    }
    Ok(exit::OK)
}

fn search(ctx: &Context, positional: &[String], options: &Options) -> Result<i32, HadesError> {
    let joined = positional.join(" ");
    let term = trim(&joined);
    if term.is_empty() {
        return Err(HadesError::new(format!(
            "hades thanatos search <termo>\n{USAGE}"
        )));
    }
    let hits = search_postmortems(&ctx.dir, term)?;
    if options.json {
        let value = serde_json::to_value(&hits).map_err(|e| fail(e.to_string()))?;
        out(&json_compact(&value));
    } else {
        for h in &hits {
            out(&format!("{}\t{}\t{}", h.id, h.title, h.line));
        }
    }
    Ok(exit::OK)
}

/// `runThanatos` (`src/commands/thanatos.ts:187`): o contexto só é montado nos cinco verbos, depois do parse.
fn run(args: &[String], ctx: &dyn Fn() -> Result<Context, HadesError>) -> Result<i32, HadesError> {
    let verb = args.first().map(String::as_str);
    let (positional, options) = parse_options(args.get(1..).unwrap_or_default())?;
    match verb {
        Some("new") => match positional.as_slice() {
            [text] => new_postmortem(&ctx()?, text, &options),
            _ => Err(HadesError::new(format!(
                "hades thanatos new <card|PR|<iso>..<iso>>\n{USAGE}"
            ))),
        },
        Some("list") => list(&ctx()?, &options),
        Some("show") => match positional.as_slice() {
            [id] => show_one(&ctx()?, id, &options),
            _ => Err(HadesError::new(format!(
                "hades thanatos show <id>\n{USAGE}"
            ))),
        },
        Some("search") => search(&ctx()?, &positional, &options),
        Some("cited") => cited(&ctx()?, &positional, &options),
        Some("-h" | "--help") | None => {
            out(USAGE);
            Ok(if verb.is_none() {
                exit::USAGE
            } else {
                exit::OK
            })
        }
        Some(other) => Err(HadesError::new(format!(
            "subcomando desconhecido: {other}\n{USAGE}"
        ))),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let env = Env::from_process();
    match run(&args, &|| real_context(&env, &cwd)) {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            warn(&render_error(&error, ERROR_PREFIX));
            ExitCode::from(error.code as u8)
        }
    }
}

#[cfg(test)]
mod tests;
