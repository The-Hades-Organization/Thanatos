//! Armazenamento dos post-mortems (`src/lib/thanatos.ts:298-410`): `<dir>/<id>.md` e `<dir>/<id>.json`.

use std::fs;
use std::path::{Path, PathBuf};

use hades_core::atomic::write_file_atomic;
use hades_core::fsx::{is_file, read_text};
use hades_core::jsonfmt::json_indent;
use hades_core::output::{HadesError, exit};
use serde_json::Value;

use crate::jsstr::{cmp_utf16, slice_utf16, trim};
use crate::model::{Citation, IndexEntry, Postmortem, SearchHit};
use crate::render::render_markdown;

/// `SEARCH_SNIPPET_MAX` (`src/lib/thanatos.ts:15`).
const SEARCH_SNIPPET_MAX: usize = 160;
const ID_MAX: usize = 80;

/// `ID_PATTERN` (`src/lib/thanatos.ts:11`): `/^[A-Za-z0-9][\w.-]{0,79}$/`, sem expressão regular.
pub fn is_valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && id.chars().count() <= ID_MAX
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// `fileIn` (`src/lib/thanatos.ts:299`).
fn file_in(dir: &Path, id: &str, ext: &str) -> Result<PathBuf, HadesError> {
    if !is_valid_id(id) {
        return Err(HadesError::with_code(
            format!("id de post-mortem inválido: {id}"),
            exit::USAGE,
        ));
    }
    Ok(dir.join(format!("{id}.{ext}")))
}

fn io_error(path: &Path, e: &std::io::Error) -> HadesError {
    HadesError::new(format!("não gravou {}: {e}", path.display()))
}

/// `exists` (`src/lib/thanatos.ts:310`). Decisão do dono: o post-mortem existe com o `.json` **ou** o `.md` (o TS só
/// olhava o `.json` e deixava `--force` sobrescrever um `.md` órfão sem aviso).
pub fn exists(dir: &Path, id: &str) -> Result<bool, HadesError> {
    Ok(is_file(&file_in(dir, id, "json")?) || is_file(&file_in(dir, id, "md")?))
}

/// `savePostmortem` (`src/lib/thanatos.ts:314`): o `.md` primeiro, o `.json` depois, os dois de forma atômica.
pub fn save_postmortem(dir: &Path, pm: &Postmortem) -> Result<(PathBuf, PathBuf), HadesError> {
    fs::create_dir_all(dir).map_err(|e| io_error(dir, &e))?;
    let md = file_in(dir, &pm.id, "md")?;
    let json = file_in(dir, &pm.id, "json")?;
    write_file_atomic(&md, render_markdown(pm).as_bytes()).map_err(|e| io_error(&md, &e))?;
    let value = serde_json::to_value(pm).map_err(|e| HadesError::new(e.to_string()))?;
    let mut text = json_indent(&value, 2);
    text.push('\n');
    write_file_atomic(&json, text.as_bytes()).map_err(|e| io_error(&json, &e))?;
    Ok((md, json))
}

/// O documento como o arquivo o tem (campos de uma `v` maior ficam), ou `None` se ausente ou ilegível. A checagem é a do
/// TS: `id` texto, `v` número, `timeline` lista e `fixData` presente (`src/lib/thanatos.ts:327-332`).
pub fn read_raw(dir: &Path, id: &str) -> Result<Option<Value>, HadesError> {
    let Some(text) = read_text(&file_in(dir, id, "json")?) else {
        return Ok(None);
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return Ok(None);
    };
    let Some(map) = doc.as_object() else {
        return Ok(None);
    };
    let ok = map.get("id").is_some_and(Value::is_string)
        && map.get("v").is_some_and(Value::is_number)
        && map.get("timeline").is_some_and(Value::is_array)
        && map.contains_key("fixData");
    Ok(ok.then_some(doc))
}

/// `readPostmortem` (`src/lib/thanatos.ts:325`). Divergência: documento com tipos errados fora dessas quatro chaves
/// vira `None` (o TS devolvia o objeto e quebrava adiante).
pub fn read_postmortem(dir: &Path, id: &str) -> Result<Option<Postmortem>, HadesError> {
    Ok(read_raw(dir, id)?.and_then(|doc| serde_json::from_value(doc).ok()))
}

/// `readMarkdown` (`src/lib/thanatos.ts:337`).
pub fn read_markdown(dir: &Path, id: &str) -> Result<Option<String>, HadesError> {
    Ok(read_text(&file_in(dir, id, "md")?))
}

/// `listPostmortems` (`src/lib/thanatos.ts:355`): do mais novo ao mais antigo; arquivo ilegível é pulado.
pub fn list_postmortems(dir: &Path) -> Vec<IndexEntry> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<IndexEntry> = Vec::new();
    for item in read.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if !is_valid_id(id) {
            continue;
        }
        let Ok(Some(pm)) = read_postmortem(dir, id) else {
            continue;
        };
        entries.push(IndexEntry {
            id: pm.id,
            title: pm.title,
            created_at: pm.created_at,
            kind: pm.input.kind().to_owned(),
            prs: pm.fix_data.prs.iter().map(|p| p.number).collect(),
            files: pm.fix_data.files.len(),
            gaps: pm.gaps.len(),
            time_to_fix_ms: pm.fix_data.time_to_fix.map(|t| t.ms),
        });
    }
    entries.sort_by(|a, b| {
        cmp_utf16(&b.created_at, &a.created_at).then_with(|| cmp_utf16(&a.id, &b.id))
    });
    entries
}

/// `searchPostmortems` (`src/lib/thanatos.ts:373`): trecho de texto, sem caixa, nunca como expressão regular.
pub fn search_postmortems(dir: &Path, term: &str) -> Result<Vec<SearchHit>, HadesError> {
    let needle = term.to_lowercase();
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for entry in list_postmortems(dir) {
        let markdown = read_markdown(dir, &entry.id)?.unwrap_or_default();
        if let Some(line) = markdown
            .split('\n')
            .find(|l| l.to_lowercase().contains(&needle))
        {
            hits.push(SearchHit {
                id: entry.id,
                title: entry.title,
                line: slice_utf16(trim(line), SEARCH_SNIPPET_MAX),
            });
        }
    }
    Ok(hits)
}

/// `citedFiles` (`src/lib/thanatos.ts:393`): post-mortems anteriores que citam algum dos arquivos tocados (caminho
/// exato do repo). Lê pelo `id` do documento, como o TS.
pub fn cited_files(dir: &Path, touched: &[String]) -> Result<Vec<Citation>, HadesError> {
    let wanted: Vec<&str> = touched
        .iter()
        .map(|f| trim(f))
        .filter(|f| !f.is_empty())
        .collect();
    let mut found: Vec<Citation> = Vec::new();
    for entry in list_postmortems(dir) {
        let Some(pm) = read_postmortem(dir, &entry.id)? else {
            continue;
        };
        let mut merged: Vec<&String> = pm
            .fix_data
            .prs
            .iter()
            .filter_map(|p| p.merged_at.as_ref())
            .collect();
        merged.sort_by(|a, b| cmp_utf16(a, b));
        let merged_at = merged.last().map(|m| (*m).clone());
        for file in &pm.fix_data.files {
            if wanted.contains(&file.as_str()) {
                found.push(Citation {
                    file: file.clone(),
                    id: pm.id.clone(),
                    title: pm.title.clone(),
                    merged_at: merged_at.clone(),
                });
            }
        }
    }
    found.sort_by(|a, b| cmp_utf16(&a.file, &b.file).then_with(|| cmp_utf16(&a.id, &b.id)));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::build_postmortem;
    use crate::model::{
        CommitFact, Gathered, Input, PrFact, ReviewFact, SourceResult, SourceStatus,
    };

    const NOW: i64 = 1_772_452_800_000;

    fn pr(number: u64) -> PrFact {
        PrFact {
            number,
            title: "fix: retry no gateway".into(),
            body: "## Causa\nTimeout.\n\n## Correção\nRetry com backoff.\n".into(),
            url: "https://example.invalid/pr/12".into(),
            author: "ana".into(),
            branch: "fix/gateway".into(),
            labels: vec![],
            created_at: "2026-03-01T15:00:00Z".into(),
            merged_at: Some("2026-03-01T17:00:00Z".into()),
            state: "MERGED".into(),
            commits: vec![CommitFact {
                sha: "a".repeat(40),
                subject: "fix: retry".into(),
                author: "ana".into(),
                ts: "2026-03-01T14:00:00Z".into(),
            }],
            files: vec!["src/gateway.ts".into(), "src/pay.ts".into()],
            reviews: vec![ReviewFact {
                author: "bia".into(),
                ts: "2026-03-01T16:00:00Z".into(),
                state: "APPROVED".into(),
            }],
        }
    }

    fn absent() -> SourceResult {
        SourceResult {
            status: SourceStatus::Absent,
            note: "sem plugin".into(),
            events: vec![],
            metrics: vec![],
            start: None,
        }
    }

    fn pm(number: u64, now: i64) -> Postmortem {
        let g = Gathered {
            prs: vec![pr(number)],
            commits: vec![],
        };
        build_postmortem(
            &Input::Pr {
                number: number as f64,
            },
            &g,
            &absent(),
            &[],
            now,
        )
    }

    #[test]
    fn id_so_vale_pelo_padrao() {
        assert!(is_valid_id("pr-12"));
        assert!(is_valid_id("range-20260301T120000Z-20260301T180000Z"));
        assert!(is_valid_id(&"a".repeat(80)));
        assert!(!is_valid_id(&"a".repeat(81)));
        for bad in ["", "../x", ".oculto", "a/b", "-x", "a b", "á"] {
            assert!(!is_valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn id_invalido_e_erro_de_uso() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_markdown(dir.path(), "../x").unwrap_err();
        assert_eq!(err.to_string(), "id de post-mortem inválido: ../x");
        assert_eq!(err.code, exit::USAGE);
        assert!(exists(dir.path(), "../x").is_err());
    }

    #[test]
    fn grava_md_e_json_le_de_volta_e_exists_olha_os_dois() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("postmortems");
        assert!(!exists(&dir, "pr-12").unwrap());
        let doc = pm(12, NOW);
        let (md, json) = save_postmortem(&dir, &doc).unwrap();
        assert!(md.ends_with("pr-12.md") && json.ends_with("pr-12.json"));
        let text = fs::read_to_string(&json).unwrap();
        assert!(text.starts_with("{\n  \"v\": 1,\n  \"id\": \"pr-12\",\n  \"input\": {\n    \"kind\": \"pr\",\n    \"number\": 12\n  },"));
        assert!(text.ends_with("}\n"));
        assert_eq!(read_postmortem(&dir, "pr-12").unwrap(), Some(doc));
        assert!(
            read_markdown(&dir, "pr-12")
                .unwrap()
                .unwrap()
                .contains("## Causa")
        );
        assert!(exists(&dir, "pr-12").unwrap());
        fs::remove_file(&json).unwrap();
        assert!(exists(&dir, "pr-12").unwrap());
        fs::remove_file(&md).unwrap();
        assert!(!exists(&dir, "pr-12").unwrap());
    }

    #[test]
    fn documento_ilegivel_ou_sem_as_chaves_vira_none() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("a.json"), "{ nao json").unwrap();
        fs::write(dir.join("b.json"), "[1]").unwrap();
        fs::write(dir.join("c.json"), r#"{"id":"c","v":1,"timeline":[]}"#).unwrap();
        for id in ["a", "b", "c", "nada"] {
            assert!(read_raw(dir, id).unwrap().is_none(), "{id}");
        }
        assert!(list_postmortems(dir).is_empty());
    }

    #[test]
    fn list_ordena_do_mais_novo_ao_mais_antigo() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        save_postmortem(dir, &pm(12, NOW)).unwrap();
        save_postmortem(dir, &pm(13, NOW + 60_000)).unwrap();
        save_postmortem(dir, &pm(11, NOW)).unwrap();
        let ids: Vec<String> = list_postmortems(dir).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["pr-13", "pr-11", "pr-12"]);
        let first = &list_postmortems(dir)[0];
        assert_eq!(first.kind, "pr");
        assert_eq!(first.prs, [13]);
        assert_eq!((first.files, first.gaps), (2, 2));
        assert_eq!(first.time_to_fix_ms, Some(3.0 * 3_600_000.0));
    }

    #[test]
    fn list_sem_pasta_e_vazio() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list_postmortems(&tmp.path().join("nada/postmortems")).is_empty());
    }

    #[test]
    fn search_acha_sem_caixa_e_nunca_como_regex() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        save_postmortem(dir, &pm(12, NOW)).unwrap();
        let hits = search_postmortems(dir, "BACKOFF").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "pr-12");
        assert_eq!(hits[0].line, "Retry com backoff.");
        assert!(search_postmortems(dir, ".*").unwrap().is_empty());
        assert!(search_postmortems(dir, "").unwrap().is_empty());
    }

    #[test]
    fn search_corta_a_linha_em_160_unidades_utf16() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let mut doc = pm(12, NOW);
        doc.title = format!("{}needle", "x".repeat(200));
        save_postmortem(dir, &doc).unwrap();
        let hits = search_postmortems(dir, "needle").unwrap();
        assert_eq!(hits[0].line.chars().count(), 160);
    }

    #[test]
    fn cited_so_por_caminho_exato() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        save_postmortem(dir, &pm(12, NOW)).unwrap();
        let touched: Vec<String> = ["src/gateway.ts", "src/outro.ts", "gateway.ts", "  "]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            cited_files(dir, &touched).unwrap(),
            [Citation {
                file: "src/gateway.ts".into(),
                id: "pr-12".into(),
                title: "fix: retry no gateway".into(),
                merged_at: Some("2026-03-01T17:00:00Z".into()),
            }]
        );
        assert!(
            cited_files(dir, &["src/livre.ts".to_string()])
                .unwrap()
                .is_empty()
        );
    }
}
