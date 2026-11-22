//! Montagem do post-mortem (`src/lib/thanatos.ts:139-247`): seções do corpo das PRs, linha do tempo, tempo até a
//! correção e lacunas.

use std::cmp::Ordering;
use std::collections::HashSet;

use indexmap::IndexMap;
use serde_json::Map;

use crate::input::id_of;
use crate::jsdate::{iso, iso_z, parse};
use crate::jsstr::{cmp_utf16, flat, trim};
use crate::model::{
    Basis, CommitFact, FixData, FixPr, Gathered, Input, JournalFact, Postmortem, PrFact,
    SourceResult, SourceStatus, SourceSummary, THANATOS_VERSION, TimeToFix, TimelineEntry,
    TimelineSource,
};

const SHORT_SHA: usize = 12;

/// `SECTION_NAMES.impact` (`src/lib/thanatos.ts:15`).
pub const IMPACT_NAMES: &[&str] = &["impacto", "impact", "impacto no usuário", "user impact"];
/// `SECTION_NAMES.cause` (`src/lib/thanatos.ts:16`).
pub const CAUSE_NAMES: &[&str] = &["causa", "causa raiz", "causa-raiz", "cause", "root cause"];
/// `SECTION_NAMES.fix` (`src/lib/thanatos.ts:17`).
pub const FIX_NAMES: &[&str] = &[
    "correção",
    "como foi corrigido",
    "solução",
    "fix",
    "solution",
    "resolution",
];

const GAP_CAUSE: &str = "causa não declarada nas PRs (seção \"Causa\" ausente): preencher à mão";
const GAP_IMPACT: &str =
    "impacto não declarado nas PRs (seção \"Impacto\" ausente): preencher à mão";

fn ms(text: &str) -> Option<i64> {
    parse(text)
}

/// `ms(a) - ms(b)` como comparador: com `NaN` o resultado do JS é 0 (iguais).
fn cmp_ms(a: &str, b: &str) -> Ordering {
    match (ms(a), ms(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => Ordering::Equal,
    }
}

/// `[...new Set(items.filter(i => i !== ""))].sort()`.
fn unique(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out: Vec<String> = items
        .into_iter()
        .filter(|i| !i.is_empty() && seen.insert(i.clone()))
        .collect();
    out.sort_by(|a, b| cmp_utf16(a, b));
    out
}

fn header_regex() -> regress::Regex {
    regress::Regex::new(r"^#{1,6}\s+(.+?)\s*#*\s*$").expect("regex fixa")
}

fn any_header_regex() -> regress::Regex {
    regress::Regex::new(r"^#{1,6}\s").expect("regex fixa")
}

/// `sectionOf` (`src/lib/thanatos.ts:144`): corpo da seção cujo cabeçalho (`#` a `######`) é um dos nomes, até o próximo
/// cabeçalho; vazio se não há.
pub fn section_of(body: &str, names: &[&str]) -> String {
    let lines: Vec<&str> = body.split('\n').collect();
    let wanted: HashSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let header = header_regex();
    let start = lines.iter().position(|line| {
        header
            .find(line)
            .and_then(|m| m.group(1).map(|r| &line[r]))
            .is_some_and(|title| {
                let cleaned: String = title
                    .to_lowercase()
                    .chars()
                    .filter(|c| !matches!(c, ':' | '*' | '_' | '`'))
                    .collect();
                wanted.contains(trim(&cleaned))
            })
    });
    let Some(start) = start else {
        return String::new();
    };
    let rest = &lines[start + 1..];
    let next = any_header_regex();
    let end = rest.iter().position(|l| next.find(l).is_some());
    trim(&rest[..end.unwrap_or(rest.len())].join("\n")).to_owned()
}

fn timeline_of(
    gathered: &Gathered,
    source: &SourceResult,
    journal: &[JournalFact],
) -> Vec<TimelineEntry> {
    let mut entries: Vec<TimelineEntry> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let short = |sha: &str| sha.chars().take(SHORT_SHA).collect::<String>();
    let mut commit = |entries: &mut Vec<TimelineEntry>, c: &CommitFact| {
        if !seen.insert(short(&c.sha)) {
            return;
        }
        entries.push(TimelineEntry {
            ts: c.ts.clone(),
            source: TimelineSource::Git,
            text: format!(
                "commit {} ({}): {}",
                short(&c.sha),
                c.author,
                flat(&c.subject)
            ),
        });
    };
    for pr in &gathered.prs {
        entries.push(TimelineEntry {
            ts: pr.created_at.clone(),
            source: TimelineSource::Gh,
            text: format!(
                "PR #{} aberta por {}: {}",
                pr.number,
                pr.author,
                flat(&pr.title)
            ),
        });
        for c in &pr.commits {
            commit(&mut entries, c);
        }
        for r in &pr.reviews {
            entries.push(TimelineEntry {
                ts: r.ts.clone(),
                source: TimelineSource::Gh,
                text: format!("review em #{} de {}: {}", pr.number, r.author, r.state),
            });
        }
        if let Some(merged) = pr.merged_at.as_deref().filter(|m| !m.is_empty()) {
            entries.push(TimelineEntry {
                ts: merged.to_owned(),
                source: TimelineSource::Gh,
                text: format!("PR #{} mergeada", pr.number),
            });
        }
    }
    for c in &gathered.commits {
        commit(&mut entries, c);
    }
    for e in journal {
        let agent = if e.agent.is_empty() { "-" } else { &e.agent };
        entries.push(TimelineEntry {
            ts: e.ts.clone(),
            source: TimelineSource::Diario,
            text: format!("{agent} {}: {}", e.kind, flat(&e.summary)),
        });
    }
    for e in &source.events {
        let text = match e.level.as_deref().filter(|l| !l.is_empty()) {
            Some(level) => format!("[{level}] {}", flat(&e.text)),
            None => flat(&e.text),
        };
        entries.push(TimelineEntry {
            ts: e.ts.clone(),
            source: TimelineSource::Fonte,
            text,
        });
    }
    let order = |s: TimelineSource| match s {
        TimelineSource::Fonte => 0,
        TimelineSource::Git => 1,
        TimelineSource::Gh => 2,
        TimelineSource::Diario => 3,
    };
    let mut dated: Vec<(i64, TimelineEntry)> = entries
        .into_iter()
        .filter_map(|e| {
            let at = ms(&e.ts)?;
            Some((at, TimelineEntry { ts: iso_z(at), ..e }))
        })
        .collect();
    dated.sort_by(|(a_at, a), (b_at, b)| {
        a_at.cmp(b_at)
            .then_with(|| order(a.source).cmp(&order(b.source)))
            .then_with(|| cmp_utf16(&a.text, &b.text))
    });
    dated.into_iter().map(|(_, e)| e).collect()
}

fn time_to_fix_of(input: &Input, gathered: &Gathered, source: &SourceResult) -> Option<TimeToFix> {
    let mut merged: Vec<&str> = gathered
        .prs
        .iter()
        .filter_map(|p| p.merged_at.as_deref())
        .collect();
    merged.sort_by(|a, b| cmp_ms(a, b));
    let to = *merged.last()?;
    let mut commits: Vec<&str> = gathered
        .prs
        .iter()
        .flat_map(|p| p.commits.iter())
        .chain(gathered.commits.iter())
        .map(|c| c.ts.as_str())
        .collect();
    commits.sort_by(|a, b| cmp_ms(a, b));
    let mut opened: Vec<&str> = gathered.prs.iter().map(|p| p.created_at.as_str()).collect();
    opened.sort_by(|a, b| cmp_ms(a, b));
    let candidates: [(Option<&str>, Basis); 4] = [
        (source.start.as_deref(), Basis::Fonte),
        (
            match input {
                Input::Range { from, .. } => Some(from.as_str()),
                _ => None,
            },
            Basis::Intervalo,
        ),
        (commits.first().copied(), Basis::PrimeiroCommit),
        (opened.first().copied(), Basis::AberturaDaPr),
    ];
    let (start, basis) = candidates
        .into_iter()
        .find_map(|(at, basis)| Some((ms(at?)?, basis)))?;
    let elapsed = match ms(to) {
        Some(end) => (end - start).max(0) as f64,
        None => f64::NAN,
    };
    Some(TimeToFix {
        ms: elapsed,
        from: iso_z(start),
        to: to.to_owned(),
        basis,
    })
}

fn from_prs(prs: &[PrFact], names: &[&str], fallback: String) -> String {
    let found: Vec<String> = prs
        .iter()
        .map(|p| section_of(&p.body, names))
        .filter(|s| !s.is_empty())
        .collect();
    if found.is_empty() {
        fallback
    } else {
        found.join("\n\n")
    }
}

fn or_default(text: String, fallback: &str) -> String {
    if text.is_empty() {
        fallback.to_owned()
    } else {
        text
    }
}

/// `buildPostmortem` (`src/lib/thanatos.ts:198`). `now_ms` é o instante do relógio (`HADES_NOW`); `createdAt` sai sem
/// milissegundos.
pub fn build_postmortem(
    input: &Input,
    gathered: &Gathered,
    source: &SourceResult,
    journal: &[JournalFact],
    now_ms: i64,
) -> Postmortem {
    let mut prs = gathered.prs.clone();
    prs.sort_by_key(|p| p.number);
    let mut gaps: Vec<String> = Vec::new();
    let impact_text = from_prs(&prs, IMPACT_NAMES, String::new());
    let cause_text = from_prs(&prs, CAUSE_NAMES, String::new());
    if prs.is_empty() {
        gaps.push(
            "nenhuma PR encontrada para a entrada: linha do tempo só com commits e diário"
                .to_owned(),
        );
    }
    if impact_text.is_empty() {
        gaps.push(GAP_IMPACT.to_owned());
    }
    if cause_text.is_empty() {
        gaps.push(GAP_CAUSE.to_owned());
    }
    if prs.iter().any(|p| p.merged_at.is_none()) {
        gaps.push(
            "há PR ainda não mergeada: o tempo até a correção só conta as mergeadas".to_owned(),
        );
    }
    if source.status != SourceStatus::Ok {
        gaps.push(format!("fonte externa: {}", source.note));
    }
    let time_to_fix = time_to_fix_of(input, gathered, source);
    if time_to_fix.is_none() {
        gaps.push("sem PR mergeada: tempo até a correção não calculado".to_owned());
    }
    let mut by_commit: IndexMap<String, &CommitFact> = IndexMap::new();
    for c in prs
        .iter()
        .flat_map(|p| p.commits.iter())
        .chain(gathered.commits.iter())
    {
        by_commit.insert(c.sha.chars().take(SHORT_SHA).collect(), c);
    }
    let mut fix_commits: Vec<CommitFact> = by_commit
        .into_iter()
        .map(|(short, c)| CommitFact {
            sha: short,
            ..c.clone()
        })
        .collect();
    fix_commits.sort_by(|a, b| cmp_ms(&a.ts, &b.ts));
    let id = id_of(input);
    let title = match (prs.as_slice(), input) {
        ([only], _) => only.title.clone(),
        (_, Input::Range { from, to }) => format!("Mudanças de {from} a {to}"),
        (_, Input::Card { id }) => format!("Card {id}"),
        (_, Input::Pr { number }) => format!("PR #{}", crate::jsstr::number_to_string(*number)),
    };
    let merged: Vec<&PrFact> = prs.iter().filter(|p| p.merged_at.is_some()).collect();
    let summary = if merged.is_empty() {
        format!("Sem PR mergeada para {id}.")
    } else {
        let list: Vec<String> = merged
            .iter()
            .map(|p| format!("#{} {}", p.number, flat(&p.title)))
            .collect();
        format!(
            "{} PR(s) de correção mergeada(s): {}.",
            merged.len(),
            list.join("; ")
        )
    };
    let fix_fallback = {
        let lines: Vec<String> = prs
            .iter()
            .map(|p| format!("#{} {}", p.number, flat(&p.title)))
            .collect();
        or_default(lines.join("\n"), "(sem PR)")
    };
    let authors = unique(
        prs.iter()
            .map(|p| p.author.clone())
            .chain(fix_commits.iter().map(|c| c.author.clone())),
    );
    let created_at = {
        let text = iso(now_ms);
        format!("{}Z", &text[..text.len() - 5])
    };
    Postmortem {
        v: THANATOS_VERSION,
        id,
        input: input.clone(),
        created_at,
        title: flat(&title),
        summary,
        impact: or_default(impact_text, "(não declarado)"),
        timeline: timeline_of(gathered, source, journal),
        cause: or_default(cause_text, "(não declarada)"),
        fix: from_prs(&prs, FIX_NAMES, fix_fallback),
        fix_data: FixData {
            prs: prs
                .iter()
                .map(|p| FixPr {
                    number: p.number,
                    title: p.title.clone(),
                    author: p.author.clone(),
                    url: p.url.clone(),
                    branch: p.branch.clone(),
                    merged_at: p.merged_at.clone(),
                })
                .collect(),
            commits: fix_commits,
            files: unique(prs.iter().flat_map(|p| p.files.iter().cloned())),
            authors,
            time_to_fix,
        },
        source: SourceSummary {
            status: source.status,
            note: source.note.clone(),
            metrics: source.metrics.clone(),
        },
        gaps,
        extra: Map::new(),
    }
}

#[cfg(test)]
mod tests;
