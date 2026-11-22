use std::cell::RefCell;
use std::fs;

use serde_json::json;

use super::*;

fn gh_pr() -> Value {
    json!({
        "number": 12, "title": "fix: retry no gateway", "body": "corpo",
        "url": "https://example.invalid/pr/12", "author": {"login": "ana"},
        "headRefName": "fix/gateway", "labels": [{"name": "incident"}],
        "createdAt": "2026-03-01T12:00:00-03:00", "mergedAt": "2026-03-01T17:00:00Z", "state": "MERGED",
        "commits": [{"oid": "a".repeat(40), "messageHeadline": "fix: retry", "authors": [{"login": "ana"}],
                     "committedDate": "2026-03-01T14:00:00Z"}],
        "files": [{"path": "src/gateway.ts"}, {"path": "src/pay.ts"}],
        "reviews": [{"author": {"login": "bia"}, "submittedAt": "2026-03-01T16:00:00Z", "state": "APPROVED"}],
    })
}

fn options(max_prs: u32) -> GatherOptions {
    GatherOptions {
        repo: "a/b".into(),
        max_prs,
        journal_max: 5,
    }
}

const RANGE: (&str, &str) = ("2026-03-01T00:00:00Z", "2026-03-02T00:00:00Z");

fn range() -> Input {
    Input::Range {
        from: RANGE.0.into(),
        to: RANGE.1.into(),
    }
}

#[test]
fn campos_ausentes_do_gh_viram_vazio_nunca_valor_inventado() {
    let pr = pr_of(&json!({"number": 5})).expect("tem número");
    assert_eq!(pr.number, 5);
    assert_eq!(pr.title, "");
    assert_eq!(pr.author, "");
    assert_eq!(pr.merged_at, None);
    assert!(pr.commits.is_empty() && pr.files.is_empty() && pr.reviews.is_empty());
    assert_eq!(pr_of(&json!({"title": "sem número"})), None);
    assert_eq!(pr_of(&json!({"number": "5"})), None);
    assert_eq!(pr_of(&json!([1])), None);
}

#[test]
fn pr_do_gh_vai_para_utc_e_descarta_o_incompleto() {
    let mut raw = gh_pr();
    raw["commits"] = json!([
        {"oid": "b".repeat(40), "messageHeadline": "x", "authors": [{"name": "Caio"}], "committedDate": "2026-03-01T10:00:00-03:00"},
        {"oid": "", "committedDate": "2026-03-01T10:00:00Z"},
        {"oid": "c".repeat(40), "committedDate": "sem-fuso"},
    ]);
    raw["reviews"] = json!([{"author": {"login": "bia"}, "submittedAt": "sem-fuso", "state": "X"}]);
    raw["labels"] = json!([{"name": ""}, {"name": "incident"}, "x"]);
    let pr = pr_of(&raw).expect("pr");
    assert_eq!(pr.created_at, "2026-03-01T15:00:00Z");
    assert_eq!(pr.commits.len(), 1);
    assert_eq!(pr.commits[0].author, "Caio");
    assert_eq!(pr.commits[0].ts, "2026-03-01T13:00:00Z");
    assert!(pr.reviews.is_empty());
    assert_eq!(pr.labels, ["incident"]);
    assert_eq!(pr.files, ["src/gateway.ts", "src/pay.ts"]);
}

#[test]
fn gh_que_devolve_texto_que_nao_e_json_e_erro_claro() {
    let exec = |_: &str, _: &[&str]| ProcResult::ok("<html>");
    let err = gather(&Input::Pr { number: 1.0 }, &exec, &options(5)).expect_err("erro");
    assert!(err.0.contains("não é JSON"), "{err}");
}

#[test]
fn falha_do_gh_leva_a_ultima_linha_do_stderr() {
    let exec = |_: &str, _: &[&str]| ProcResult::fail("aviso\nHTTP 404: Not Found\n");
    let err = gather(&Input::Pr { number: 99.0 }, &exec, &options(5)).expect_err("erro");
    assert_eq!(err.0, "gh pr view 99: HTTP 404: Not Found");
}

#[test]
fn pr_sem_numero_na_resposta_nao_e_encontrada() {
    let exec = |_: &str, _: &[&str]| ProcResult::ok("{}");
    let err = gather(&Input::Pr { number: 7.0 }, &exec, &options(5)).expect_err("erro");
    assert_eq!(err.0, "PR #7 não encontrada em a/b");
}

#[test]
fn o_teto_de_prs_vai_ao_gh() {
    let seen = RefCell::new(Vec::<String>::new());
    let exec = |c: &str, a: &[&str]| {
        if c == "gh" {
            *seen.borrow_mut() = a.iter().map(|s| s.to_string()).collect();
        }
        if c == "git" {
            ProcResult::ok("")
        } else {
            ProcResult::ok("[]")
        }
    };
    gather(&range(), &exec, &options(7)).expect("ok");
    let seen = seen.borrow();
    let at = seen.iter().position(|a| a == "--limit").expect("--limit");
    assert_eq!(seen[at + 1], "7");
}

#[test]
fn pr_chama_o_gh_view_com_os_campos_do_contrato() {
    let calls = RefCell::new(Vec::<String>::new());
    let exec = |c: &str, a: &[&str]| {
        calls.borrow_mut().push(format!("{c} {}", a.join(" ")));
        ProcResult::ok(&gh_pr().to_string())
    };
    let got = gather(&Input::Pr { number: 12.0 }, &exec, &options(5)).expect("ok");
    assert_eq!(got.prs.len(), 1);
    assert!(got.commits.is_empty());
    assert_eq!(
        calls.borrow().as_slice(),
        [format!("gh pr view 12 -R a/b --json {PR_FIELDS}")]
    );
}

fn range_exec(calls: &RefCell<Vec<String>>) -> impl Fn(&str, &[&str]) -> ProcResult + '_ {
    move |c, a| {
        calls.borrow_mut().push(format!("{c} {}", a.join(" ")));
        match (c, a.get(1).copied()) {
            ("gh", Some("list")) => {
                ProcResult::ok(r#"[{"number": 12}, {"nota": 1}, {"number": "x"}]"#)
            }
            ("gh", Some("view")) => ProcResult::ok(&gh_pr().to_string()),
            ("git", _) => ProcResult::ok(&format!(
                "{}\u{1f}ana\u{1f}2026-03-01T11:00:00-03:00\u{1f}fix: retry\n{}\u{1f}caio\u{1f}2026-03-01T16:30:00+00:00\u{1f}chore: logs\n\nlixo\u{1f}x\u{1f}sem-fuso\u{1f}y\n",
                "a".repeat(40),
                "b".repeat(40)
            )),
            _ => ProcResult::fail("inesperado"),
        }
    }
}

#[test]
fn intervalo_le_as_prs_mergeadas_e_os_commits_do_git() {
    let calls = RefCell::new(Vec::new());
    let exec = range_exec(&calls);
    let got = gather(&range(), &exec, &options(30)).expect("ok");
    assert_eq!(got.prs.iter().map(|p| p.number).collect::<Vec<_>>(), [12]);
    assert_eq!(
        got.commits
            .iter()
            .map(|c| (c.sha.as_str().get(..1).unwrap_or(""), c.ts.as_str()))
            .collect::<Vec<_>>(),
        [("a", "2026-03-01T14:00:00Z"), ("b", "2026-03-01T16:30:00Z")]
    );
    assert_eq!(got.commits[1].author, "caio");
    assert_eq!(got.commits[0].subject, "fix: retry");
    let calls = calls.borrow();
    let list = calls
        .iter()
        .find(|c| c.starts_with("gh pr list"))
        .expect("list");
    assert!(list.contains("--search merged:2026-03-01T00:00:00Z..2026-03-02T00:00:00Z"));
    assert!(list.contains("--state merged"));
    let log = calls
        .iter()
        .find(|c| c.starts_with("git log"))
        .expect("log");
    assert!(log.contains("--since=2026-03-01T00:00:00Z --until=2026-03-02T00:00:00Z --no-merges"));
}

#[test]
fn git_log_com_falha_vira_erro() {
    let exec = |c: &str, _: &[&str]| {
        if c == "git" {
            ProcResult::fail("fatal: não é um repositório")
        } else {
            ProcResult::ok("[]")
        }
    };
    let err = gather(&range(), &exec, &options(5)).expect_err("erro");
    assert_eq!(err.0, "git log: fatal: não é um repositório");
}

#[test]
fn card_filtra_localmente_por_titulo_branch_ou_corpo_sem_caixa() {
    let viewed = RefCell::new(Vec::<String>::new());
    let exec = |_: &str, a: &[&str]| {
        if a[1] == "list" {
            return ProcResult::ok(
                &json!([
                    {"number": 12, "title": "fix-77 retry", "headRefName": "x", "body": ""},
                    {"number": 13, "title": "outra", "headRefName": "y", "body": ""},
                    {"number": 14, "title": "z", "headRefName": "FIX-77/b", "body": ""},
                    {"number": 15, "title": "w", "headRefName": "q", "body": "ver Fix-77"},
                ])
                .to_string(),
            );
        }
        viewed.borrow_mut().push(a[2].to_owned());
        ProcResult::ok(&gh_pr().to_string())
    };
    let got = gather(
        &Input::Card {
            id: "FIX-77".into(),
        },
        &exec,
        &options(30),
    )
    .expect("ok");
    assert_eq!(got.prs.len(), 3);
    assert_eq!(viewed.borrow().as_slice(), ["12", "14", "15"]);
    assert!(got.commits.is_empty());
}

fn journal_line(ts: &str, kind: &str, summary: &str) -> String {
    json!({"v": 1, "ts": ts, "agent": "impl-x", "session": "s", "source": "watch", "kind": kind, "summary": summary, "cwd": "/w"}).to_string()
}

fn journal_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("tmp");
    let own = root.path().join("acme__fixer/journal");
    let other = root.path().join("outro__repo/journal");
    fs::create_dir_all(&own).expect("dir");
    fs::create_dir_all(&other).expect("dir");
    let lines = [
        journal_line("2026-03-01T15:30:00Z", "watch.done", "DONE impl-x"),
        journal_line("2026-03-01T15:31:00Z", "tool_pre", "Bash ls"),
        journal_line("2026-03-05T00:00:00Z", "watch.done", "fora do período"),
        "linha que não parseia".to_owned(),
    ];
    fs::write(own.join("impl-x.jsonl"), lines.join("\n") + "\n").expect("write");
    fs::write(
        other.join("z.jsonl"),
        journal_line("2026-03-01T15:40:00Z", "watch.done", "de outro projeto") + "\n",
    )
    .expect("write");
    root
}

#[test]
fn diario_so_o_projeto_so_o_periodo_sem_ruido_e_sem_linha_quebrada() {
    let root = journal_root();
    let got = journal_facts("acme__fixer", RANGE.0, RANGE.1, 10, root.path());
    assert_eq!(
        got,
        [JournalFact {
            ts: "2026-03-01T15:30:00Z".into(),
            agent: "impl-x".into(),
            kind: "watch.done".into(),
            summary: "DONE impl-x".into()
        }]
    );
}

#[test]
fn diario_fica_com_os_mais_recentes_e_em_ordem() {
    let root = tempfile::tempdir().expect("tmp");
    let dir = root.path().join("p/journal");
    fs::create_dir_all(&dir).expect("dir");
    let lines: Vec<String> = ["03", "01", "02", "04"]
        .iter()
        .map(|h| journal_line(&format!("2026-03-01T{h}:00:00Z"), "k", h))
        .collect();
    fs::write(dir.join("a.jsonl"), lines.join("\n")).expect("write");
    let got = journal_facts("p", RANGE.0, RANGE.1, 2, root.path());
    assert_eq!(
        got.iter().map(|f| f.summary.as_str()).collect::<Vec<_>>(),
        ["03", "04"]
    );
}

#[test]
fn diario_ausente_e_vazio() {
    let root = tempfile::tempdir().expect("tmp");
    assert!(journal_facts("p", RANGE.0, RANGE.1, 5, root.path()).is_empty());
    assert!(journal_facts("p", RANGE.0, RANGE.1, 5, &root.path().join("nao-existe")).is_empty());
}

#[test]
fn janela_do_intervalo_e_o_proprio_intervalo() {
    assert_eq!(
        window_of(&range(), &Gathered::default()),
        Some((RANGE.0.to_owned(), RANGE.1.to_owned()))
    );
}

#[test]
fn janela_da_pr_vai_da_abertura_ou_primeiro_commit_ao_ultimo_merge() {
    let early = pr_of(&{
        let mut raw = gh_pr();
        raw["commits"][0]["committedDate"] = json!("2026-03-01T09:00:00Z");
        raw
    })
    .expect("pr");
    let late = pr_of(&{
        let mut raw = gh_pr();
        raw["mergedAt"] = json!("2026-03-01T19:00:00Z");
        raw
    })
    .expect("pr");
    let gathered = Gathered {
        prs: vec![late, early],
        commits: Vec::new(),
    };
    assert_eq!(
        window_of(&Input::Pr { number: 12.0 }, &gathered),
        Some((
            "2026-03-01T09:00:00Z".to_owned(),
            "2026-03-01T19:00:00Z".to_owned()
        ))
    );
}

#[test]
fn sem_pr_mergeada_ou_sem_pr_nao_ha_janela() {
    let open = pr_of(&{
        let mut raw = gh_pr();
        raw["mergedAt"] = Value::Null;
        raw
    })
    .expect("pr");
    let gathered = Gathered {
        prs: vec![open],
        commits: Vec::new(),
    };
    assert_eq!(window_of(&Input::Pr { number: 12.0 }, &gathered), None);
    assert_eq!(
        window_of(&Input::Pr { number: 12.0 }, &Gathered::default()),
        None
    );
}

#[test]
fn executor_real_roda_com_cwd_e_token_so_no_gh() {
    let dir = tempfile::tempdir().expect("tmp");
    let exec = SystemExec {
        cwd: dir.path().to_path_buf(),
        timeout: Duration::from_secs(5),
        gh_token: Some("tok".into()),
    };
    let r = exec.run("pwd", &[]);
    assert_eq!(r.status, 0);
    assert_eq!(
        fs::canonicalize(r.stdout.trim()).expect("canon"),
        fs::canonicalize(dir.path()).expect("canon")
    );
    let missing = exec.run("comando-que-nao-existe-thanatos", &[]);
    assert_eq!(missing.status, 1);
    assert!(!missing.stderr.is_empty());
}
