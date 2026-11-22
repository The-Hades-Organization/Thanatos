//! Leitura que o app faz sobre o `thanatos-core`; nada aqui roda na thread da interface.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hades_core::Env;
use hades_core::output::HadesError;
use hades_core::work::{WorkOptions, load_work_env};
use serde_json::Value;
use thanatos_core::build::build_postmortem;
use thanatos_core::gather::{GatherOptions, SystemExec, gather, journal_facts, window_of};
use thanatos_core::input::{id_of, parse_input};
use thanatos_core::model::{IndexEntry, Postmortem, SourceResult, SourceStatus};
use thanatos_core::proc::{Outcome, SpawnOptions, spawn};
use thanatos_core::settings::{self, Settings};
use thanatos_core::source::{SourceDeps, fetch_source};
use thanatos_core::store::{
    exists, list_postmortems, read_markdown, read_postmortem, save_postmortem,
};

pub fn listar(dir: &Path) -> Vec<IndexEntry> {
    list_postmortems(dir)
}

/// O documento aberto: o `.json` tipado e o `.md` (que pode faltar, ou ter sido removido à mão).
pub struct Documento {
    pub postmortem: Postmortem,
    pub markdown: Option<String>,
    pub caminho_md: PathBuf,
}

pub fn abrir(dir: &Path, id: &str) -> Result<Option<Documento>, HadesError> {
    let Some(postmortem) = read_postmortem(dir, id)? else {
        return Ok(None);
    };
    let markdown = read_markdown(dir, id)?;
    Ok(Some(Documento {
        postmortem,
        markdown,
        caminho_md: dir.join(format!("{id}.md")),
    }))
}

/// O que `thanatos new` precisa do projeto ativo (`realContext`, `src/commands/thanatos.ts:90`); montado uma vez,
/// fora da thread da interface.
#[derive(Clone, Debug)]
pub struct Contexto {
    pub dir: PathBuf,
    pub slug: String,
    pub repo: String,
    pub repo_top: PathBuf,
    pub card_pattern: Option<String>,
    pub plugin_env: Option<PathBuf>,
    pub credenciais: Option<String>,
    pub journal_root: PathBuf,
    pub ajustes: Settings,
    pub gh_token: Option<String>,
}

fn objeto<'a>(v: &'a Value, chave: &str) -> Option<&'a Value> {
    v.get(chave).filter(|o| o.is_object())
}

/// Conta do `gh` do perfil sem trocar a conta ativa: `GH_TOKEN`/`GITHUB_TOKEN` do ambiente vencem
/// (`useProjectToken`, `src/commands/thanatos.ts:75`).
fn token_do_perfil(env: &Env, perfil: &Value, cwd: &Path, timeout: Duration) -> Option<String> {
    if env.get_raw("GH_TOKEN").is_some() || env.get_raw("GITHUB_TOKEN").is_some() {
        return None;
    }
    let gh = objeto(perfil, "identity").and_then(|i| objeto(i, "gh"))?;
    if let Some(nome) = gh.get("tokenEnv").and_then(Value::as_str)
        && let Some(valor) = env.get_raw(nome).filter(|v| !v.is_empty())
    {
        return Some(valor.to_owned());
    }
    let usuario = gh.get("user").and_then(Value::as_str)?;
    let opcoes = SpawnOptions {
        cwd: Some(cwd.to_path_buf()),
        timeout: Some(timeout),
        ..SpawnOptions::default()
    };
    match spawn("gh", &["auth", "token", "--user", usuario], &opcoes) {
        Outcome::Done {
            status: 0, stdout, ..
        } if !stdout.trim().is_empty() => Some(stdout.trim().to_owned()),
        _ => None,
    }
}

/// Monta o contexto do projeto de `cwd`: pasta, perfil, padrão de card, plugin, credenciais e ajustes.
pub fn contexto(env: &Env, cwd: &Path) -> Result<Contexto, HadesError> {
    use hades_core::{paths, plugins, profile, project};
    let options = WorkOptions {
        cwd: Some(cwd.to_path_buf()),
        ..WorkOptions::default()
    };
    let work = load_work_env(&options, env)?;
    let resolucao = project::resolve_project(env, &work.repo_top);
    let perfil = if resolucao.code == 0 {
        profile::load_profile(&resolucao)
    } else {
        Value::Object(Default::default())
    };
    let pack = project::pack_of(&resolucao);
    let ajustes = settings::load(env, cwd);
    let card = objeto(&perfil, "card");
    let card_pattern = card
        .filter(|c| c.get("provider").and_then(Value::as_str) != Some("none"))
        .and_then(|c| c.get("idPattern").and_then(Value::as_str))
        .map(str::to_owned);
    let estado = paths::find_state_dir(env, cwd);
    let timeout = Duration::from_secs(u64::from(ajustes.io_timeout_s));
    let gh_token = token_do_perfil(env, &perfil, &work.repo_top, timeout);
    Ok(Contexto {
        dir: work.work.join("postmortems"),
        repo: work.slug.replace("__", "/"),
        slug: work.slug.clone(),
        repo_top: work.repo_top.clone(),
        card_pattern,
        plugin_env: pack.and_then(|p| plugins::env_dir(&p, &perfil)),
        credenciais: estado.and_then(|d| hades_core::fsx::read_text(&d.join("credenciais.md"))),
        journal_root: paths::work_root(env),
        ajustes,
        gh_token,
    })
}

/// O que `thanatos new` gravou: os dois arquivos e o resumo que a CLI imprime.
#[derive(Clone, Debug, PartialEq)]
pub struct Criado {
    pub id: String,
    pub md: PathBuf,
    pub json: PathBuf,
    pub prs: usize,
    pub eventos: usize,
    pub tempo_ate_a_correcao: String,
    pub lacunas: Vec<String>,
}

/// `newPostmortem` (`src/commands/thanatos.ts:120`): interpreta, recusa sobrescrever sem `force`, coleta, consulta a
/// fonte, monta e grava. Bloqueia: só roda no executor de fundo.
pub fn criar(env: &Env, ctx: &Contexto, texto: &str, force: bool) -> Result<Criado, String> {
    let input = parse_input(texto, ctx.card_pattern.as_deref()).map_err(|e| e.to_string())?;
    let id = id_of(&input);
    if exists(&ctx.dir, &id).map_err(|e| e.to_string())? && !force {
        return Err(format!(
            "post-mortem {id} já existe em {}; use Refazer para regerar (a edição à mão se perde)",
            ctx.dir.display()
        ));
    }
    let exec = SystemExec {
        cwd: ctx.repo_top.clone(),
        timeout: Duration::from_secs(u64::from(ctx.ajustes.io_timeout_s)),
        gh_token: ctx.gh_token.clone(),
    };
    let opcoes = GatherOptions {
        repo: ctx.repo.clone(),
        max_prs: ctx.ajustes.max_prs,
        journal_max: ctx.ajustes.journal_max,
    };
    let colhido = gather(&input, &exec, &opcoes).map_err(|e| e.to_string())?;
    let janela = window_of(&input, &colhido);
    let diario = janela.as_ref().map_or_else(Vec::new, |(de, ate)| {
        journal_facts(
            &ctx.slug,
            de,
            ate,
            ctx.ajustes.journal_max as usize,
            &ctx.journal_root,
        )
    });
    let fonte = match &janela {
        Some((de, ate)) => fetch_source(
            de,
            ate,
            &SourceDeps {
                env_dir: ctx.plugin_env.as_deref(),
                credentials: ctx.credenciais.as_deref(),
                timeout_s: ctx.ajustes.source_timeout_s,
                env,
            },
        ),
        None => SourceResult {
            status: SourceStatus::Absent,
            note: "sem janela de tempo (nenhuma PR mergeada) para consultar a fonte externa"
                .to_owned(),
            events: Vec::new(),
            metrics: Vec::new(),
            start: None,
        },
    };
    let agora = hades_core::clock::now_ms(env).map_err(|e| e.to_string())?;
    let pm = build_postmortem(&input, &colhido, &fonte, &diario, agora);
    let (md, json) = save_postmortem(&ctx.dir, &pm).map_err(|e| e.to_string())?;
    Ok(Criado {
        id: pm.id.clone(),
        md,
        json,
        prs: pm.fix_data.prs.len(),
        eventos: pm.timeline.len(),
        tempo_ate_a_correcao: crate::vm::tempo_ate_a_correcao(&pm),
        lacunas: pm.gaps.clone(),
    })
}

/// Abre o `.md` no editor do sistema (`xdg-open`); bloqueia só até o `xdg-open` sair, no executor de fundo.
pub fn abrir_no_editor(caminho: &Path) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(caminho)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("xdg-open: {e}"))
        .and_then(|st| {
            if st.success() {
                Ok(())
            } else {
                Err(format!("xdg-open saiu com {st}"))
            }
        })
}
