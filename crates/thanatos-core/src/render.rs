//! Markdown do post-mortem (`src/lib/thanatos.ts:251-296`).

use crate::jsstr::number_to_string;
use crate::model::Postmortem;

/// `formatDuration` (`src/lib/thanatos.ts:251`). `NaN` dá `NaNmin`, como o JS.
pub fn format_duration(total_ms: f64) -> String {
    let minutes = (total_ms / 60_000.0).floor();
    let days = (minutes / 1440.0).floor();
    let hours = ((minutes % 1440.0) / 60.0).floor();
    let rest = minutes % 60.0;
    let truthy = |n: f64| n != 0.0 && !n.is_nan();
    let mut parts: Vec<String> = Vec::new();
    if truthy(days) {
        parts.push(format!("{}d", number_to_string(days)));
    }
    if truthy(hours) {
        parts.push(format!("{}h", number_to_string(hours)));
    }
    parts.push(format!("{}min", number_to_string(rest)));
    parts.join(" ")
}

/// `bullets` (`src/lib/thanatos.ts:260`).
fn bullets(items: &[String]) -> String {
    if items.is_empty() {
        return "- (nenhum)".to_owned();
    }
    items
        .iter()
        .map(|i| format!("- {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn or_none(items: Vec<String>, sep: &str, empty: &str) -> String {
    if items.is_empty() {
        empty.to_owned()
    } else {
        items.join(sep)
    }
}

/// `renderMarkdown` (`src/lib/thanatos.ts:262`).
pub fn render_markdown(pm: &Postmortem) -> String {
    let ttf = match &pm.fix_data.time_to_fix {
        Some(t) => format!(
            "{} ({} → {}, base: {})",
            format_duration(t.ms),
            t.from,
            t.to,
            t.basis.as_str()
        ),
        None => "(não calculado)".to_owned(),
    };
    let timeline = if pm.timeline.is_empty() {
        "- (sem eventos)".to_owned()
    } else {
        pm.timeline
            .iter()
            .map(|e| format!("- {} · {} · {}", e.ts, e.source.as_str(), e.text))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let prs = or_none(
        pm.fix_data
            .prs
            .iter()
            .map(|p| {
                format!(
                    "#{} ({}, {})",
                    p.number,
                    p.author,
                    p.merged_at.as_deref().unwrap_or("não mergeada")
                )
            })
            .collect(),
        "; ",
        "(nenhuma)",
    );
    let files = if pm.fix_data.files.is_empty() {
        "  - (nenhum)".to_owned()
    } else {
        pm.fix_data
            .files
            .iter()
            .map(|f| format!("  - `{f}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut lines: Vec<String> = vec![
        format!("# Post-mortem {}: {}", pm.id, pm.title),
        String::new(),
        format!(
            "Gerado em {} (UTC) por `hades thanatos new`. Todos os horários estão em UTC.",
            pm.created_at
        ),
        String::new(),
        "## Resumo".into(),
        pm.summary.clone(),
        String::new(),
        "## Impacto".into(),
        pm.impact.clone(),
        String::new(),
        "## Linha do tempo (UTC)".into(),
        timeline,
        String::new(),
        "## Causa".into(),
        pm.cause.clone(),
        String::new(),
        "## Como foi corrigido".into(),
        pm.fix.clone(),
        String::new(),
        "## Dados da correção".into(),
        format!("- PRs: {prs}"),
        format!("- Commits: {}", pm.fix_data.commits.len()),
        format!(
            "- Autores: {}",
            or_none(pm.fix_data.authors.clone(), ", ", "(nenhum)")
        ),
        format!("- Tempo até a correção: {ttf}"),
        "- Arquivos:".into(),
        files,
    ];
    if !pm.source.metrics.is_empty() {
        let metrics: Vec<String> = pm
            .source
            .metrics
            .iter()
            .map(|m| {
                let unit = m
                    .unit
                    .as_deref()
                    .filter(|u| !u.is_empty())
                    .map(|u| format!(" {u}"))
                    .unwrap_or_default();
                let ts =
                    m.ts.as_deref()
                        .filter(|t| !t.is_empty())
                        .map(|t| format!(" ({t})"))
                        .unwrap_or_default();
                format!("{}: {}{unit}{ts}", m.name, number_to_string(m.value))
            })
            .collect();
        lines.push(String::new());
        lines.push("## Métricas da fonte externa".into());
        lines.push(bullets(&metrics));
    }
    lines.push(String::new());
    lines.push("## Lacunas".into());
    lines.push(bullets(&pm.gaps));
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::build_postmortem;
    use crate::model::{Gathered, Input, SourceMetric, SourceResult, SourceStatus};

    #[test]
    fn duracao_em_dias_horas_e_minutos() {
        assert_eq!(format_duration(0.0), "0min");
        assert_eq!(format_duration(8.0 * 3_600_000.0), "8h 0min");
        assert_eq!(format_duration(90_000.0), "1min");
        assert_eq!(
            format_duration((25 * 60 + 5) as f64 * 60_000.0),
            "1d 1h 5min"
        );
        assert_eq!(format_duration(f64::NAN), "NaNmin");
    }

    #[test]
    fn markdown_de_post_mortem_sem_pr_tem_todas_as_secoes() {
        let source = SourceResult {
            status: SourceStatus::Ok,
            note: String::new(),
            events: vec![],
            metrics: vec![SourceMetric {
                name: "erros/min".into(),
                value: 42.0,
                unit: Some("1/min".into()),
                ts: Some("2026-03-01T10:00:00Z".into()),
            }],
            start: None,
        };
        let input = Input::Card { id: "FIX-1".into() };
        let pm = build_postmortem(
            &input,
            &Gathered::default(),
            &source,
            &[],
            1_772_452_800_000,
        );
        let md = render_markdown(&pm);
        for h in [
            "## Resumo",
            "## Impacto",
            "## Linha do tempo (UTC)",
            "## Causa",
            "## Como foi corrigido",
            "## Dados da correção",
            "## Métricas da fonte externa",
            "## Lacunas",
        ] {
            assert!(md.contains(h), "{h}");
        }
        assert!(md.contains("- (sem eventos)"));
        assert!(md.contains("- PRs: (nenhuma)"));
        assert!(md.contains("- erros/min: 42 1/min (2026-03-01T10:00:00Z)"));
        assert!(md.contains("- Tempo até a correção: (não calculado)"));
        assert!(md.ends_with("\n"));
    }
}
