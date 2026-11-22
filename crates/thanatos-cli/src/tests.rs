use super::*;
use thanatos_core::gather::ProcResult;

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn context(dir: &Path, exec: Box<dyn Exec>) -> Context {
    Context {
        now_ms: 0,
        exec,
        dir: dir.to_path_buf(),
        slug: "acme__demo".into(),
        repo: "acme/demo".into(),
        card_pattern: None,
        plugin_env: None,
        credentials: None,
        journal_root: dir.join("work"),
        settings: settings::DEFAULTS,
        env: Env::empty(),
    }
}

fn no_exec() -> Box<dyn Exec> {
    Box::new(|_: &str, _: &[&str]| ProcResult::fail("não devia rodar"))
}

fn never() -> Result<Context, HadesError> {
    panic!("o contexto não devia ser montado");
}

fn code_of(result: Result<i32, HadesError>) -> i32 {
    match result {
        Ok(code) => code,
        Err(error) => error.code,
    }
}

#[test]
fn parser_separa_posicionais_e_opcoes() {
    let (positional, options) = parse_options(&args(&[
        "a", "--json", "b", "--force", "--base", "main", "--pr", "7",
    ]))
    .unwrap();
    assert_eq!(positional, ["a", "b"]);
    assert!(options.json && options.force);
    assert_eq!(options.base.as_deref(), Some("main"));
    assert_eq!(options.pr.as_deref(), Some("7"));
}

#[test]
fn parser_base_consome_o_proximo_argumento_mesmo_opcao() {
    let (positional, options) = parse_options(&args(&["--base", "--json"])).unwrap();
    assert!(positional.is_empty() && !options.json);
    assert_eq!(options.base.as_deref(), Some("--json"));
}

#[test]
fn parser_recusa_opcao_desconhecida_e_valor_faltando() {
    let unknown = parse_options(&args(&["--x"])).err().unwrap();
    assert_eq!(unknown.code, exit::USAGE);
    assert!(unknown.message.starts_with("opção desconhecida: --x\n"));
    let missing = parse_options(&args(&["--pr"])).err().unwrap();
    assert!(missing.message.starts_with("--pr exige um valor\n"));
}

#[test]
fn despacho_sem_verbo_e_ajuda_nao_montam_contexto() {
    assert_eq!(run(&[], &never).unwrap(), exit::USAGE);
    assert_eq!(run(&args(&["-h"]), &never).unwrap(), exit::OK);
    assert_eq!(run(&args(&["--help"]), &never).unwrap(), exit::OK);
}

#[test]
fn despacho_recusa_verbo_desconhecido_e_opcao_antes_do_verbo() {
    let error = run(&args(&["x"]), &never).unwrap_err();
    assert_eq!(error.code, exit::USAGE);
    assert!(error.message.starts_with("subcomando desconhecido: x\n"));
    assert_eq!(code_of(run(&args(&["x", "--y"]), &never)), exit::USAGE);
    assert_eq!(
        code_of(run(&args(&["cited", "--base"]), &never)),
        exit::USAGE
    );
}

#[test]
fn despacho_confere_a_aridade_antes_do_contexto() {
    assert_eq!(code_of(run(&args(&["new"]), &never)), exit::USAGE);
    assert_eq!(code_of(run(&args(&["new", "1", "2"]), &never)), exit::USAGE);
    assert_eq!(code_of(run(&args(&["show"]), &never)), exit::USAGE);
    assert_eq!(
        code_of(run(&args(&["show", "a", "b"]), &never)),
        exit::USAGE
    );
}

#[test]
fn list_ignora_force_e_sai_zero_com_pasta_ausente() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = || Ok(context(&tmp.path().join("postmortems"), no_exec()));
    assert_eq!(run(&args(&["list", "--force"]), &ctx).unwrap(), exit::OK);
}

#[test]
fn show_inexistente_sai_1_e_id_invalido_sai_2() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = || Ok(context(tmp.path(), no_exec()));
    let missing = run(&args(&["show", "pr-1"]), &ctx).unwrap_err();
    assert_eq!(missing.code, exit::FAIL);
    assert_eq!(missing.message, "post-mortem não encontrado: pr-1");
    assert_eq!(code_of(run(&args(&["show", "../x"]), &ctx)), exit::USAGE);
}

#[test]
fn search_sem_termo_e_uso() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = || Ok(context(tmp.path(), no_exec()));
    assert_eq!(code_of(run(&args(&["search", " "]), &ctx)), exit::USAGE);
    assert_eq!(run(&args(&["search", "x"]), &ctx).unwrap(), exit::OK);
}

#[test]
fn cited_com_arquivos_nao_chama_processo_e_sai_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = || Ok(context(tmp.path(), no_exec()));
    assert_eq!(
        run(&args(&["cited", "a.ts", "b.ts"]), &ctx).unwrap(),
        exit::OK
    );
}

#[test]
fn cited_pr_nao_numerica_e_uso_e_nao_chama_gh() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = || Ok(context(tmp.path(), no_exec()));
    for bad in ["abc", "1x", "-1", ""] {
        let error = run(&args(&["cited", "--pr", bad]), &ctx).unwrap_err();
        assert_eq!(error.code, exit::USAGE, "{bad}");
        assert_eq!(error.message, format!("--pr é o número da PR: {bad}"));
    }
}

#[test]
fn cited_base_falha_sai_1_com_a_ultima_linha_do_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    let exec = |command: &str, a: &[&str]| {
        assert_eq!(
            (command, a),
            ("git", &["diff", "--name-only", "nao-existe...HEAD"][..])
        );
        ProcResult::fail("aviso\nfatal: bad revision\n")
    };
    let ctx = context(tmp.path(), Box::new(exec));
    let error = cited(
        &ctx,
        &[],
        &Options {
            base: Some("nao-existe".into()),
            ..Options::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, exit::FAIL);
    assert_eq!(
        error.message,
        "git diff nao-existe...HEAD: fatal: bad revision"
    );
}

#[test]
fn cited_pr_usa_o_gh_com_o_repo_do_contexto() {
    let tmp = tempfile::tempdir().unwrap();
    let exec = |command: &str, a: &[&str]| {
        assert_eq!(command, "gh");
        assert_eq!(
            a,
            [
                "pr",
                "view",
                "12",
                "-R",
                "acme/demo",
                "--json",
                "files",
                "--jq",
                ".files[].path"
            ]
        );
        ProcResult::ok("src/a.ts\n\nsrc/b.ts\n")
    };
    let ctx = context(tmp.path(), Box::new(exec));
    let files = touched_files(
        &ctx,
        &[],
        &Options {
            pr: Some("12".into()),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(files, ["src/a.ts", "src/b.ts"]);
}

#[test]
fn new_recusa_existente_sem_force_antes_de_chamar_git_ou_gh() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("pr-12.md"), "x").unwrap();
    let ctx = context(tmp.path(), no_exec());
    let error = new_postmortem(&ctx, "12", &Options::default()).unwrap_err();
    assert_eq!(error.code, exit::FAIL);
    assert!(error.message.starts_with("post-mortem pr-12 já existe em "));
    assert!(
        error
            .message
            .ends_with("; use --force para regerar (a edição à mão se perde)")
    );
}

#[test]
fn new_falha_do_gh_sai_1_e_nao_cria_a_pasta() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("postmortems");
    let ctx = context(&dir, no_exec());
    let error = new_postmortem(&ctx, "99", &Options::default()).unwrap_err();
    assert_eq!(error.code, exit::FAIL);
    assert!(!dir.exists());
}

#[test]
fn new_entrada_invalida_sai_2() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = context(tmp.path(), no_exec());
    let error = new_postmortem(
        &ctx,
        "2026-03-02T00:00:00Z..2026-03-01T00:00:00Z",
        &Options::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, exit::USAGE);
}

#[test]
fn token_do_ambiente_vence_o_do_perfil() {
    let profile = json!({"identity": {"gh": {"user": "ana", "tokenEnv": "T"}}});
    let env = Env::empty().with("GH_TOKEN", "x").with("T", "y");
    assert_eq!(project_token(&profile, &env, &*no_exec()), None);
    let env = Env::empty().with("GITHUB_TOKEN", "x");
    assert_eq!(project_token(&profile, &env, &*no_exec()), None);
}

#[test]
fn token_do_perfil_vem_de_tokenenv_e_depois_do_gh_auth() {
    let profile = json!({"identity": {"gh": {"user": "ana", "tokenEnv": "T"}}});
    let env = Env::empty().with("T", "y");
    assert_eq!(
        project_token(&profile, &env, &*no_exec()).as_deref(),
        Some("y")
    );
    let exec = |command: &str, a: &[&str]| {
        assert_eq!(
            (command, a),
            ("gh", &["auth", "token", "--user", "ana"][..])
        );
        ProcResult::ok("fake-token-ana\n")
    };
    assert_eq!(
        project_token(&profile, &Env::empty(), &exec).as_deref(),
        Some("fake-token-ana")
    );
    let empty = |_: &str, _: &[&str]| ProcResult::ok("  \n");
    assert_eq!(project_token(&profile, &Env::empty(), &empty), None);
    assert_eq!(project_token(&json!({}), &Env::empty(), &*no_exec()), None);
}
