use thanatos_core::model::{
    Basis, CommitFact, FixData, Input, Postmortem, SourceMetric, SourceStatus, SourceSummary,
    TimeToFix, TimelineEntry, TimelineSource,
};

use super::*;

fn entrada(id: &str, criado: &str, lacunas: usize) -> IndexEntry {
    IndexEntry {
        id: id.to_owned(),
        title: "t".to_owned(),
        created_at: criado.to_owned(),
        kind: "pr".to_owned(),
        prs: vec![],
        files: 0,
        gaps: lacunas,
        time_to_fix_ms: None,
    }
}

fn documento() -> Postmortem {
    Postmortem {
        v: 1.0,
        id: "pr-1234".to_owned(),
        input: Input::Pr { number: 1234.0 },
        created_at: "2026-11-21T14:00:00.000Z".to_owned(),
        title: "fila travava o pronto".to_owned(),
        summary: String::new(),
        impact: String::new(),
        timeline: vec![
            TimelineEntry {
                ts: "2026-11-21T13:02:10.000Z".to_owned(),
                source: TimelineSource::Git,
                text: "commit".to_owned(),
            },
            TimelineEntry {
                ts: "2026-11-21T13:09:00.000Z".to_owned(),
                source: TimelineSource::Diario,
                text: "ready vermelho".to_owned(),
            },
            TimelineEntry {
                ts: "2026-11-21T13:10:00.000Z".to_owned(),
                source: TimelineSource::Diario,
                text: "outro".to_owned(),
            },
        ],
        cause: String::new(),
        fix: String::new(),
        fix_data: FixData {
            prs: vec![],
            commits: vec![CommitFact {
                sha: "a".to_owned(),
                subject: "s".to_owned(),
                author: "x".to_owned(),
                ts: "2026-11-21T13:02:10.000Z".to_owned(),
            }],
            files: vec![],
            authors: vec![],
            time_to_fix: Some(TimeToFix {
                ms: 41.0 * 60_000.0,
                from: "a".to_owned(),
                to: "b".to_owned(),
                basis: Basis::Fonte,
            }),
        },
        source: SourceSummary {
            status: SourceStatus::NoToken,
            note: "sem token do plugin".to_owned(),
            metrics: vec![SourceMetric {
                name: "pods".to_owned(),
                value: 2.0,
                unit: Some("un".to_owned()),
                ts: None,
            }],
        },
        gaps: vec![],
        extra: Default::default(),
    }
}

#[test]
fn tela_por_rotulo_e_por_tecla() {
    assert_eq!(Tela::de_rotulo("Fontes"), Some(Tela::Fontes));
    assert_eq!(Tela::de_rotulo("Novo"), Some(Tela::Novo));
    assert_eq!(Tela::de_rotulo("nada"), None);
    assert_eq!(tela_da_tecla("1"), Some(Tela::Postmortems));
    assert_eq!(tela_da_tecla("2"), Some(Tela::Novo));
    assert_eq!(tela_da_tecla("3"), Some(Tela::Fontes));
    assert_eq!(tela_da_tecla("x"), None);
}

#[test]
fn tarefa_so_tem_uma_em_voo() {
    let mut t = Tarefa::default();
    let g = t.iniciar().expect("primeira");
    assert!(t.carregando());
    assert_eq!(t.iniciar(), None);
    assert!(t.concluir(g));
    assert!(!t.carregando());
    assert!(t.iniciar().is_some());
}

#[test]
fn tarefa_descarta_resultado_velho() {
    let mut t = Tarefa::default();
    let velha = t.iniciar().expect("primeira");
    t.invalidar();
    assert!(!t.carregando());
    let nova = t.iniciar().expect("segunda");
    assert!(!t.concluir(velha));
    assert!(
        t.carregando(),
        "o resultado velho não libera a execução nova"
    );
    assert!(t.concluir(nova));
}

#[test]
fn data_e_hora_em_utc() {
    assert_eq!(data_hora("2026-11-21T13:02:44.000Z"), "2026-11-21 13:02");
    assert_eq!(hora("2026-11-21T13:02:44.000Z"), "13:02");
    assert_eq!(hora("sem data"), "sem data");
    assert_eq!(data_hora("curto"), "curto");
    assert_eq!(hora("2026-11-21 13:02"), "2026-11-21 13:02");
}

#[test]
fn celulas_da_lista() {
    let c = celulas_lista(&entrada("pr-1", "2026-11-21T13:02:44.000Z", 2));
    assert_eq!(c, ["pr-1", "2026-11-21 13:02", "2"]);
}

#[test]
fn selecao_sobrevive_a_recarga_ou_cai_no_primeiro() {
    let lista = vec![entrada("a", "x", 0), entrada("b", "x", 0)];
    assert_eq!(selecao_valida(&lista, Some("b")), Some("b".to_owned()));
    assert_eq!(selecao_valida(&lista, Some("sumiu")), Some("a".to_owned()));
    assert_eq!(selecao_valida(&lista, None), Some("a".to_owned()));
    assert_eq!(selecao_valida(&[], Some("a")), None);
    assert_eq!(indice_do_id(&lista, Some("b")), Some(1));
    assert_eq!(indice_do_id(&lista, None), None);
}

#[test]
fn linha_do_tempo_com_hora_e_tom_da_fonte() {
    let linhas = linhas_do_tempo(&documento().timeline);
    assert_eq!(linhas[0].hora, "13:02");
    assert_eq!(linhas[0].fonte, "git");
    assert_eq!(linhas[0].tom, Tone::Info);
    assert_eq!(linhas[1].fonte, "diário");
}

#[test]
fn markdown_sem_a_secao_da_linha_do_tempo() {
    let md = "# T\n\n## Resumo\nx\n\n## Linha do tempo (UTC)\n- a\n- b\n\n## Causa\ny";
    assert_eq!(sem_linha_do_tempo(md), "# T\n\n## Resumo\nx\n\n## Causa\ny");
    assert_eq!(sem_linha_do_tempo("## Resumo\nx"), "## Resumo\nx");
}

#[test]
fn titulo_e_tempo_ate_a_correcao() {
    let mut pm = documento();
    assert_eq!(titulo_do_documento(&pm), "pr-1234 · fila travava o pronto");
    assert_eq!(tempo_ate_a_correcao(&pm), "41min");
    pm.fix_data.time_to_fix = None;
    assert_eq!(tempo_ate_a_correcao(&pm), "—");
}

#[test]
fn cartoes_das_quatro_fontes() {
    let c = cartoes_das_fontes(&documento());
    let nomes: Vec<_> = c.iter().map(|c| c.nome).collect();
    assert_eq!(nomes, ["git", "gh", "diário", "plugin"]);
    assert_eq!(c[0].resumo, "1 commit");
    assert_eq!((c[0].selo, c[0].tom), ("ok", Tone::Ok));
    assert_eq!(c[1].resumo, "0 PRs");
    assert_eq!((c[1].selo, c[1].tom), ("ausente", Tone::Neutral));
    assert_eq!(c[2].resumo, "2 eventos");
    assert_eq!(c[3].resumo, "1 métrica");
    assert_eq!((c[3].selo, c[3].tom), ("sem token", Tone::Warn));
    assert_eq!(c[3].nota.as_deref(), Some("sem token do plugin"));
}

#[test]
fn plugin_sem_metricas_e_sem_nota() {
    let mut pm = documento();
    pm.source.metrics.clear();
    pm.source.note.clear();
    pm.source.status = SourceStatus::Error;
    let c = cartoes_das_fontes(&pm);
    assert_eq!(c[3].resumo, "sem métricas");
    assert_eq!((c[3].selo, c[3].tom), ("erro", Tone::Bad));
    assert_eq!(c[3].nota, None);
}

#[test]
fn metricas_com_e_sem_unidade() {
    let mut pm = documento();
    pm.source.metrics.push(SourceMetric {
        name: "erros".to_owned(),
        value: 1.5,
        unit: None,
        ts: None,
    });
    assert_eq!(linhas_das_metricas(&pm), ["pods: 2 un", "erros: 1.5"]);
}

#[test]
fn rodape_com_plural() {
    assert_eq!(rodape_da_lista(1), "1 post-mortem");
    assert_eq!(rodape_da_lista(3), "3 post-mortems");
}
