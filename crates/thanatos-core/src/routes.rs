//! Rotas do painel (`src/panel/thanatos.ts:17-29`): `GET /api/thanatos` e `GET /api/thanatos/:id`, só leitura sobre
//! `<work>/<owner>__<repo>/postmortems/`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::store::{is_valid_id, list_postmortems, read_markdown, read_raw};

/// Falha de `one`: o painel responde 400 (`BadRequest`, `src/panel/http.ts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadRequest(pub String);

/// `ThanatosRoutes` (`src/panel/thanatos.ts:12`).
#[derive(Clone, Debug)]
pub struct ThanatosRoutes {
    dir: Option<PathBuf>,
}

impl ThanatosRoutes {
    /// `createThanatosRoutes` (`src/panel/thanatos.ts:20`): a pasta é irmã da de locks; sem locks, não há pasta.
    pub fn new(locks: Option<&Path>) -> Self {
        Self {
            dir: locks.map(|l| l.parent().unwrap_or(Path::new("")).join("postmortems")),
        }
    }

    /// `list` (`src/panel/thanatos.ts:22`): `{ items }`.
    pub fn list(&self) -> Value {
        let items = self
            .dir
            .as_deref()
            .map(list_postmortems)
            .unwrap_or_default();
        json!({ "items": items })
    }

    /// `one` (`src/panel/thanatos.ts:23-28`): `Ok(None)` é 404; id fora do padrão é `BadRequest` (400). O documento é o
    /// lido do arquivo, com campos de uma `v` maior.
    pub fn one(&self, id: &str) -> Result<Option<Value>, BadRequest> {
        if !is_valid_id(id) {
            return Err(BadRequest(format!("id inválido: {id}")));
        }
        let Some(dir) = self.dir.as_deref() else {
            return Ok(None);
        };
        let postmortem = read_raw(dir, id).ok().flatten();
        let markdown = read_markdown(dir, id).ok().flatten();
        Ok(match (postmortem, markdown) {
            (Some(postmortem), Some(markdown)) => {
                Some(json!({ "id": id, "markdown": markdown, "postmortem": postmortem }))
            }
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::build_postmortem;
    use crate::model::{Gathered, Input, SourceResult, SourceStatus};
    use crate::store::save_postmortem;
    use std::fs;

    fn project() -> (tempfile::TempDir, ThanatosRoutes, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let locks = tmp.path().join("work/acme__fixer/locks");
        let dir = tmp.path().join("work/acme__fixer/postmortems");
        (tmp, ThanatosRoutes::new(Some(&locks)), dir)
    }

    fn write(dir: &Path, id: &str, extra: Option<(&str, &str)>) {
        let gathered = Gathered {
            prs: vec![],
            commits: vec![],
        };
        let source = SourceResult {
            status: SourceStatus::Absent,
            note: "sem plugin".into(),
            events: vec![],
            metrics: vec![],
            start: None,
        };
        let mut doc = build_postmortem(
            &Input::Pr { number: 12.0 },
            &gathered,
            &source,
            &[],
            1_772_452_800_000,
        );
        doc.id = id.to_owned();
        let (_, json) = save_postmortem(dir, &doc).unwrap();
        if let Some((key, value)) = extra {
            let mut value_doc: Value =
                serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
            value_doc[key] = json!(value);
            fs::write(&json, value_doc.to_string()).unwrap();
        }
    }

    #[test]
    fn lista_vazia_sem_pasta() {
        let (_tmp, routes, _) = project();
        assert_eq!(routes.list(), json!({ "items": [] }));
    }

    #[test]
    fn lista_e_leitura_vem_da_pasta_do_projeto() {
        let (_tmp, routes, dir) = project();
        write(&dir, "pr-12", None);
        let items = routes.list();
        assert_eq!(items["items"].as_array().unwrap().len(), 1);
        assert_eq!(items["items"][0]["id"], "pr-12");
        let one = routes.one("pr-12").unwrap().unwrap();
        assert_eq!(one["id"], "pr-12");
        assert!(one["markdown"].as_str().unwrap().contains("## Causa"));
        assert_eq!(one["postmortem"]["v"], 1);
    }

    #[test]
    fn inexistente_e_none_e_id_invalido_e_400() {
        let (_tmp, routes, dir) = project();
        write(&dir, "pr-12", None);
        assert_eq!(routes.one("pr-99"), Ok(None));
        for bad in ["../x", ".oculto", "a/b", ""] {
            assert!(routes.one(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn sem_md_ou_com_json_ilegivel_e_none() {
        let (_tmp, routes, dir) = project();
        write(&dir, "pr-1", None);
        fs::remove_file(dir.join("pr-1.md")).unwrap();
        fs::write(dir.join("pr-2.json"), "{").unwrap();
        fs::write(dir.join("pr-2.md"), "x").unwrap();
        assert_eq!(routes.one("pr-1"), Ok(None));
        assert_eq!(routes.one("pr-2"), Ok(None));
    }

    #[test]
    fn campos_de_versao_maior_ficam() {
        let (_tmp, routes, dir) = project();
        write(&dir, "pr-v2", Some(("extra", "sim")));
        let one = routes.one("pr-v2").unwrap().unwrap();
        assert_eq!(one["postmortem"]["extra"], "sim");
    }

    #[test]
    fn painel_sem_locks_lista_vazio() {
        let routes = ThanatosRoutes::new(None);
        assert_eq!(routes.list(), json!({ "items": [] }));
        assert_eq!(routes.one("pr-1"), Ok(None));
        assert!(routes.one("../x").is_err());
    }
}
