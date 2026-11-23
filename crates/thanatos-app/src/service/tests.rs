use std::path::Path;

use hades_core::config::config_value;

use super::*;
use crate::vm::config::{Origem, Pagina};

const CATALOGO: &str = "\
HADES_OUTRA_COISA\t5\tint:1:9\tGeral\tOutra\tnext\tNão é do Thanatos.
HADES_THANATOS_SUGGEST\t1\tbool\tOrquestração\tSugerir post-mortem\tnext\tSugere ao mergear.
HADES_THANATOS_SOURCE_TIMEOUT_S\t60\tint:1:600\tOrquestração\tTempo da fonte (s)\tnext\tTempo da fonte.
HADES_THANATOS_IO_TIMEOUT_S\t60\tint:1:600\tOrquestração\tTempo de git e gh (s)\tnext\tTempo de cada chamada.
HADES_THANATOS_MAX_PRS\t30\tint:1:200\tOrquestração\tPRs por intervalo\tnext\tTeto de PRs.
HADES_THANATOS_JOURNAL_MAX\t40\tint:1:500\tOrquestração\tEventos do diário\tnext\tEventos na linha do tempo.
";

struct Ambiente {
    _skill: tempfile::TempDir,
    dados: tempfile::TempDir,
    env: Env,
}

fn ambiente(extra: &[(&str, &str)]) -> Ambiente {
    let skill = tempfile::tempdir().unwrap();
    let dados = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(skill.path().join("kit")).unwrap();
    std::fs::write(skill.path().join("kit/config.tsv"), CATALOGO).unwrap();
    let mut env = Env::empty()
        .with("HOME", dados.path().to_string_lossy())
        .with("HADES_SKILL_DIR", skill.path().to_string_lossy())
        .with("HADES_DATA", dados.path().to_string_lossy());
    for (k, v) in extra {
        env = env.with(k, *v);
    }
    Ambiente {
        _skill: skill,
        dados,
        env,
    }
}

fn fonte(a: &Ambiente) -> ConfigDoCore {
    ConfigDoCore::new(a.env.clone(), a.dados.path().to_path_buf())
}

fn pagina(a: &Ambiente) -> Pagina {
    Pagina::new(fonte(a).ler().unwrap())
}

fn indice(p: &Pagina, chave: &str) -> usize {
    p.linhas()
        .iter()
        .position(|l| l.chave.chave == chave)
        .unwrap()
}

/// O mesmo valor que o CLI enxergaria (`config_value`, o que `settings::load` lê).
fn do_cli(a: &Ambiente, chave: &str) -> String {
    config_value(&a.env, Path::new(a.dados.path()), chave, "?")
}

fn gravar(a: &Ambiente, p: &Pagina) {
    fonte(a).gravar(&p.mudancas().expect("sem erro")).unwrap();
}

#[test]
fn so_as_chaves_do_thanatos_na_ordem_do_catalogo() {
    let a = ambiente(&[]);
    let chaves: Vec<_> = pagina(&a)
        .linhas()
        .iter()
        .map(|l| l.chave.chave.clone())
        .collect();
    assert_eq!(
        chaves,
        [
            "HADES_THANATOS_SUGGEST",
            "HADES_THANATOS_SOURCE_TIMEOUT_S",
            "HADES_THANATOS_IO_TIMEOUT_S",
            "HADES_THANATOS_MAX_PRS",
            "HADES_THANATOS_JOURNAL_MAX",
        ]
    );
    let p = pagina(&a);
    let i = indice(&p, "HADES_THANATOS_MAX_PRS");
    assert_eq!(p.linhas()[i].chave.padrao, "30");
    assert_eq!(p.linhas()[i].chave.origem(), Origem::Padrao);
}

#[test]
fn grava_pela_pagina_e_o_cli_enxerga() {
    let a = ambiente(&[]);
    let mut p = pagina(&a);
    let i = indice(&p, "HADES_THANATOS_MAX_PRS");
    p.definir(i, " 12 ");
    let j = indice(&p, "HADES_THANATOS_SUGGEST");
    p.definir(j, "não");
    assert_eq!(p.pendentes(), 2);
    gravar(&a, &p);
    assert_eq!(do_cli(&a, "HADES_THANATOS_MAX_PRS"), "12");
    assert_eq!(do_cli(&a, "HADES_THANATOS_SUGGEST"), "0");
    let ajustes = thanatos_core::settings::load(&a.env, a.dados.path());
    assert_eq!(ajustes.max_prs, 12);
    let relida = pagina(&a);
    let linha = &relida.linhas()[indice(&relida, "HADES_THANATOS_MAX_PRS")];
    assert_eq!(linha.chave.origem(), Origem::Arquivo);
    assert_eq!(linha.rascunho, "12");
    assert_eq!(relida.pendentes(), 0);
    let texto = std::fs::read_to_string(a.dados.path().join("config.env")).unwrap();
    assert!(texto.contains("HADES_THANATOS_MAX_PRS=12\n"));
    assert!(texto.contains("HADES_THANATOS_SUGGEST=0\n"));
}

#[test]
fn fora_da_faixa_nao_grava() {
    let a = ambiente(&[]);
    let mut p = pagina(&a);
    let i = indice(&p, "HADES_THANATOS_MAX_PRS");
    for ruim in ["0", "201", "abc", "", "1.5", "-3", "+4"] {
        p.definir(i, ruim);
        assert_eq!(
            p.erro(i).as_deref(),
            Some("Número inteiro entre 1 e 200"),
            "{ruim:?}"
        );
        assert!(p.mudancas().is_none(), "{ruim:?}");
    }
    p.definir(i, "200");
    assert_eq!(p.erro(i), None);
    gravar(&a, &p);
    assert_eq!(do_cli(&a, "HADES_THANATOS_MAX_PRS"), "200");
    assert!(pagina(&a).linhas().iter().all(|l| l.rascunho != "201"));
}

#[test]
fn restaurar_o_padrao_tira_a_chave_do_arquivo() {
    let a = ambiente(&[]);
    let mut p = pagina(&a);
    let i = indice(&p, "HADES_THANATOS_JOURNAL_MAX");
    p.definir(i, "90");
    gravar(&a, &p);
    assert_eq!(do_cli(&a, "HADES_THANATOS_JOURNAL_MAX"), "90");
    let mut p = pagina(&a);
    assert!(p.modificado(i));
    p.restaurar(i);
    assert!(!p.modificado(i));
    assert_eq!(p.pendentes(), 1);
    let m = p.mudancas().unwrap();
    assert_eq!(m.remover, ["HADES_THANATOS_JOURNAL_MAX"]);
    assert!(m.atualizar.is_empty());
    fonte(&a).gravar(&m).unwrap();
    assert_eq!(do_cli(&a, "HADES_THANATOS_JOURNAL_MAX"), "40");
    let texto = std::fs::read_to_string(a.dados.path().join("config.env")).unwrap();
    assert!(!texto.contains("JOURNAL_MAX"));
    assert_eq!(pagina(&a).fora_do_padrao(), 0);
}

#[test]
fn valor_igual_ao_padrao_digitado_tambem_sai_do_arquivo() {
    let a = ambiente(&[]);
    let mut p = pagina(&a);
    let i = indice(&p, "HADES_THANATOS_IO_TIMEOUT_S");
    p.definir(i, "5");
    gravar(&a, &p);
    let mut p = pagina(&a);
    p.definir(i, "060");
    let m = p.mudancas().unwrap();
    assert_eq!(m.remover, ["HADES_THANATOS_IO_TIMEOUT_S"]);
}

#[test]
fn valor_do_ambiente_vence_e_fica_so_leitura() {
    let a = ambiente(&[("HADES_THANATOS_MAX_PRS", "7")]);
    let mut p = pagina(&a);
    let i = indice(&p, "HADES_THANATOS_MAX_PRS");
    assert_eq!(p.linhas()[i].chave.origem(), Origem::Ambiente);
    assert_eq!(p.linhas()[i].rascunho, "7");
    p.definir(i, "99");
    p.restaurar(i);
    assert_eq!(p.linhas()[i].rascunho, "7");
    assert_eq!(p.pendentes(), 0);
    assert!(!p.modificado(i));
    assert_eq!(do_cli(&a, "HADES_THANATOS_MAX_PRS"), "7");
}

#[test]
fn descartar_volta_ao_valor_salvo() {
    let a = ambiente(&[]);
    let mut p = pagina(&a);
    let i = indice(&p, "HADES_THANATOS_MAX_PRS");
    p.definir(i, "abc");
    assert_eq!(p.pendentes(), 1);
    p.descartar();
    assert_eq!(p.pendentes(), 0);
    assert_eq!(p.linhas()[i].rascunho, "30");
}

#[test]
fn sem_instalacao_nao_ha_onde_gravar() {
    let env = Env::empty().with("HOME", "/nao/existe");
    let f = ConfigDoCore::new(env, PathBuf::from("/nao/existe"));
    assert!(f.ler().is_err());
}
