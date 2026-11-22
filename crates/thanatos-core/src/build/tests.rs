//! Testes de TT "montagem" (`loop/test/ts/thanatos.test.ts:85-122`) e de `section_of`, mais dedupe e título.

use super::*;
use crate::model::{ReviewFact, SourceEvent, SourceMetric};

const BODY: &str = "## Impacto\nCheckout fora do ar por 2h.\n\n## Causa\nTimeout sem retry no gateway.\n\n## Correção\nRetry com backoff.\n";

fn now() -> i64 {
    parse("2026-03-02T12:00:00.000Z").unwrap()
}

fn commit(sha: char, author: &str, ts: &str) -> CommitFact {
    CommitFact {
        sha: sha.to_string().repeat(40),
        subject: "fix: retry".into(),
        author: author.into(),
        ts: ts.into(),
    }
}

fn pr() -> PrFact {
    PrFact {
        number: 12,
        title: "fix: retry no gateway".into(),
        body: BODY.into(),
        url: "https://example.invalid/pr/12".into(),
        author: "ana".into(),
        branch: "fix/gateway".into(),
        labels: vec!["incident".into()],
        created_at: "2026-03-01T15:00:00Z".into(),
        merged_at: Some("2026-03-01T17:00:00Z".into()),
        state: "MERGED".into(),
        commits: vec![commit('a', "ana", "2026-03-01T14:00:00Z")],
        files: vec!["src/gateway.ts".into(), "src/pay.ts".into()],
        reviews: vec![ReviewFact {
            author: "bia".into(),
            ts: "2026-03-01T16:00:00Z".into(),
            state: "APPROVED".into(),
        }],
    }
}

fn gathered() -> Gathered {
    Gathered {
        prs: vec![pr()],
        commits: vec![],
    }
}

fn no_source() -> SourceResult {
    SourceResult {
        status: SourceStatus::Absent,
        note: "sem plugin".into(),
        events: vec![],
        metrics: vec![],
        start: None,
    }
}

fn with_source() -> SourceResult {
    SourceResult {
        status: SourceStatus::Ok,
        note: "1 evento".into(),
        start: Some("2026-03-01T09:00:00Z".into()),
        metrics: vec![SourceMetric {
            name: "erros/min".into(),
            value: 42.0,
            unit: None,
            ts: None,
        }],
        events: vec![SourceEvent {
            ts: "2026-03-01T13:15:00Z".into(),
            text: "erro 500 no checkout".into(),
            level: Some("error".into()),
        }],
    }
}

fn pr_input() -> Input {
    Input::Pr { number: 12.0 }
}

#[test]
fn linha_do_tempo_em_ordem_e_em_utc_de_todas_as_fontes() {
    let journal = [JournalFact {
        ts: "2026-03-01T15:30:00Z".into(),
        agent: "impl-x".into(),
        kind: "watch.done".into(),
        summary: "DONE impl-x".into(),
    }];
    let pm = build_postmortem(&pr_input(), &gathered(), &with_source(), &journal, now());
    let got: Vec<String> = pm
        .timeline
        .iter()
        .map(|e| format!("{} {}", e.ts, e.source.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            "2026-03-01T13:15:00Z fonte",
            "2026-03-01T14:00:00Z git",
            "2026-03-01T15:00:00Z gh",
            "2026-03-01T15:30:00Z diario",
            "2026-03-01T16:00:00Z gh",
            "2026-03-01T17:00:00Z gh",
        ]
    );
    assert!(pm.timeline.iter().all(|e| e.ts.ends_with('Z')));
}

#[test]
fn secoes_vem_do_corpo_e_dados_da_correcao_somam_tudo() {
    let pm = build_postmortem(&pr_input(), &gathered(), &with_source(), &[], now());
    assert_eq!(pm.v, 1.0);
    assert_eq!(pm.id, "pr-12");
    assert_eq!(pm.created_at, "2026-03-02T12:00:00Z");
    assert_eq!(pm.impact, "Checkout fora do ar por 2h.");
    assert_eq!(pm.cause, "Timeout sem retry no gateway.");
    assert_eq!(pm.fix, "Retry com backoff.");
    assert_eq!(pm.fix_data.files, ["src/gateway.ts", "src/pay.ts"]);
    assert_eq!(pm.fix_data.authors, ["ana"]);
    assert_eq!(pm.fix_data.commits.len(), 1);
    assert_eq!(pm.fix_data.commits[0].sha, "a".repeat(12));
    assert_eq!(pm.fix_data.commits[0].author, "ana");
    assert_eq!(
        pm.fix_data.time_to_fix,
        Some(TimeToFix {
            ms: 8.0 * 3_600_000.0,
            from: "2026-03-01T09:00:00Z".into(),
            to: "2026-03-01T17:00:00Z".into(),
            basis: Basis::Fonte,
        })
    );
    assert!(pm.gaps.is_empty());
}

#[test]
fn sem_fonte_conta_do_primeiro_commit_e_a_lacuna_diz_o_que_faltou() {
    let pm = build_postmortem(&pr_input(), &gathered(), &no_source(), &[], now());
    let ttf = pm.fix_data.time_to_fix.unwrap();
    assert_eq!(ttf.ms, 3.0 * 3_600_000.0);
    assert_eq!(ttf.basis, Basis::PrimeiroCommit);
    assert_eq!(pm.gaps, ["fonte externa: sem plugin"]);
}

#[test]
fn pr_sem_secoes_e_sem_merge_declara_lacunas_e_nao_tem_tempo() {
    let mut p = pr();
    p.body = "só um texto".into();
    p.merged_at = None;
    p.state = "OPEN".into();
    let g = Gathered {
        prs: vec![p],
        commits: vec![],
    };
    let pm = build_postmortem(&pr_input(), &g, &no_source(), &[], now());
    assert!(pm.fix_data.time_to_fix.is_none());
    let hits = pm
        .gaps
        .iter()
        .filter(|g| g.contains("causa") || g.contains("impacto") || g.contains("mergeada"))
        .count();
    assert!(hits >= 4, "{:?}", pm.gaps);
    assert_eq!(pm.impact, "(não declarado)");
    assert_eq!(pm.cause, "(não declarada)");
}

#[test]
fn section_of_pega_ate_o_proximo_cabecalho_e_ignora_caixa_e_dois_pontos() {
    assert_eq!(
        section_of(
            "# Intro\nx\n### Causa raiz:\nA\nB\n## Outra\nC",
            &["causa raiz"]
        ),
        "A\nB"
    );
    assert_eq!(section_of("sem cabeçalho", &["causa"]), "");
}

#[test]
fn commit_repetido_entra_uma_vez_na_linha_do_tempo_e_nos_dados() {
    let g = Gathered {
        prs: vec![pr()],
        commits: vec![
            commit('a', "ana", "2026-03-01T14:00:00Z"),
            commit('b', "caio", "2026-03-01T16:30:00Z"),
        ],
    };
    let pm = build_postmortem(&pr_input(), &g, &no_source(), &[], now());
    let git = pm
        .timeline
        .iter()
        .filter(|e| e.source == TimelineSource::Git)
        .count();
    assert_eq!(git, 2);
    let shas: Vec<&str> = pm.fix_data.commits.iter().map(|c| &c.sha[..1]).collect();
    assert_eq!(shas, ["a", "b"]);
    assert_eq!(pm.fix_data.authors, ["ana", "caio"]);
}

#[test]
fn titulo_de_intervalo_e_de_card_sem_pr() {
    let range = Input::Range {
        from: "2026-03-01T00:00:00Z".into(),
        to: "2026-03-02T00:00:00Z".into(),
    };
    let pm = build_postmortem(&range, &Gathered::default(), &no_source(), &[], now());
    assert_eq!(
        pm.title,
        "Mudanças de 2026-03-01T00:00:00Z a 2026-03-02T00:00:00Z"
    );
    assert_eq!(
        pm.summary,
        "Sem PR mergeada para range-20260301T000000Z-20260302T000000Z."
    );
    let card = Input::Card {
        id: "FIX-77".into(),
    };
    let pm = build_postmortem(&card, &Gathered::default(), &no_source(), &[], now());
    assert_eq!(pm.title, "Card FIX-77");
    assert_eq!(pm.fix, "(sem PR)");
}
