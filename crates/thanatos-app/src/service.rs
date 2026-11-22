//! Leitura que o app faz sobre o `thanatos-core`; nada aqui roda na thread da interface.

use std::path::{Path, PathBuf};

use hades_core::Env;
use hades_core::output::HadesError;
use hades_core::work::{WorkOptions, load_work_env};
use thanatos_core::model::{IndexEntry, Postmortem};
use thanatos_core::store::{list_postmortems, read_markdown, read_postmortem};

/// `<work>/<owner>__<repo>/postmortems` do repositório de `cwd` (`src/commands/thanatos.ts:109`).
pub fn pasta(env: &Env, cwd: &Path) -> Result<PathBuf, HadesError> {
    let options = WorkOptions {
        cwd: Some(cwd.to_path_buf()),
        ..WorkOptions::default()
    };
    Ok(load_work_env(&options, env)?.work.join("postmortems"))
}

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
