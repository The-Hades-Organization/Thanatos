//! Coleta do Thanatos na fronteira (`src/lib/thanatos-gather.ts`): PRs e reviews pelo `gh`, commits pelo `git` e o diário
//! (só leitura). O executor de processos entra por parâmetro, para o teste trocar `gh` e `git` por dublê sem rede.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hades_core::journal;
use serde_json::{Map, Value};

use crate::jsdate::{parse, to_utc};
use crate::jsstr::{cmp_utf16, trim};
use crate::model::{CommitFact, Gathered, Input, JournalFact, PrFact, ReviewFact};
use crate::proc::{EnvMode, Outcome, SpawnOptions, spawn};

/// `ProcResult` (`src/lib/thanatos-gather.ts:8`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl ProcResult {
    pub fn ok(stdout: &str) -> Self {
        Self {
            status: 0,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    pub fn fail(stderr: &str) -> Self {
        Self {
            status: 1,
            stdout: String::new(),
            stderr: stderr.to_owned(),
        }
    }
}

/// `Exec` (`src/lib/thanatos-gather.ts:9`).
pub trait Exec {
    fn run(&self, command: &str, args: &[&str]) -> ProcResult;
}

impl<F> Exec for F
where
    F: Fn(&str, &[&str]) -> ProcResult,
{
    fn run(&self, command: &str, args: &[&str]) -> ProcResult {
        self(command, args)
    }
}

/// O executor real: cwd na raiz do repositório, tempo limite por processo (`src/commands/thanatos.ts:102`) e, quando
/// há, o `GH_TOKEN` do perfil só no filho (a conta ativa do `gh` não muda).
#[derive(Clone, Debug)]
pub struct SystemExec {
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub gh_token: Option<String>,
}

impl Exec for SystemExec {
    fn run(&self, command: &str, args: &[&str]) -> ProcResult {
        let env = match (&self.gh_token, command) {
            (Some(token), "gh") => EnvMode::Extend(vec![("GH_TOKEN".to_owned(), token.clone())]),
            _ => EnvMode::Inherit,
        };
        let options = SpawnOptions {
            cwd: Some(self.cwd.clone()),
            timeout: Some(self.timeout),
            max_buffer: None,
            env,
        };
        match spawn(command, args, &options) {
            Outcome::Done {
                status,
                stdout,
                stderr,
            } => ProcResult {
                status,
                stdout,
                stderr,
            },
            Outcome::TimedOut | Outcome::Overflow => ProcResult::fail(""),
            Outcome::Failed(why) => ProcResult::fail(&why),
        }
    }
}

/// `GatherOptions` (`src/lib/thanatos-gather.ts:12`).
#[derive(Clone, Debug)]
pub struct GatherOptions {
    /// `dono/nome` do projeto.
    pub repo: String,
    pub max_prs: u32,
    pub journal_max: u32,
}

/// `GatherError` (`src/lib/thanatos-gather.ts:19`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatherError(pub String);

impl fmt::Display for GatherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GatherError {}

const PR_FIELDS: &str = "number,title,body,author,headRefName,labels,createdAt,mergedAt,state,url,commits,files,reviews";
const UNIT: char = '\u{1f}';

fn object(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}

fn str_of(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_owned()
}

fn list(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn utc(v: Option<&Value>) -> String {
    to_utc(v.and_then(Value::as_str).unwrap_or("")).unwrap_or_default()
}

/// Última linha do stderr aparado: `r.stderr.trim().split("\n").pop()` (`:26`).
fn last_line(stderr: &str) -> &str {
    trim(stderr).rsplit('\n').next().unwrap_or("")
}

/// `json` (`src/lib/thanatos-gather.ts:25`).
fn json(r: &ProcResult, what: &str) -> Result<Value, GatherError> {
    if r.status != 0 {
        return Err(GatherError(format!("{what}: {}", last_line(&r.stderr))));
    }
    serde_json::from_str(&r.stdout)
        .map_err(|_| GatherError(format!("{what}: resposta que não é JSON")))
}

/// `PrFact.number` e o número das listas são `u64`; o TS aceita qualquer `number`, e o `gh` só devolve inteiro positivo.
fn number_of(v: Option<&Value>) -> Option<u64> {
    v.and_then(Value::as_u64)
}

/// `commitOf` (`src/lib/thanatos-gather.ts:34`).
fn commit_of(raw: &Value) -> Option<CommitFact> {
    let raw = object(raw)?;
    let authors: Vec<&Map<String, Value>> =
        list(raw.get("authors")).iter().filter_map(object).collect();
    let first = authors.first();
    let ts = utc(raw.get("committedDate"));
    let sha = str_of(raw.get("oid"));
    if sha.is_empty() || ts.is_empty() {
        return None;
    }
    let login = str_of(first.and_then(|a| a.get("login")));
    let author = if login.is_empty() {
        str_of(first.and_then(|a| a.get("name")))
    } else {
        login
    };
    Some(CommitFact {
        sha,
        subject: str_of(raw.get("messageHeadline")),
        author,
        ts,
    })
}

/// `prOf` (`src/lib/thanatos-gather.ts:45`): PR do `gh pr view --json`; campo que falta vira vazio, nunca valor inventado.
pub fn pr_of(raw: &Value) -> Option<PrFact> {
    let raw = object(raw)?;
    let number = raw.get("number").filter(|n| n.is_number())?;
    let number = number_of(Some(number))?;
    let login_of = |v: Option<&Value>| str_of(v.and_then(object).and_then(|a| a.get("login")));
    let mut reviews = Vec::new();
    for r in list(raw.get("reviews")) {
        let Some(r) = object(r) else { continue };
        let ts = utc(r.get("submittedAt"));
        if !ts.is_empty() {
            reviews.push(ReviewFact {
                author: login_of(r.get("author")),
                ts,
                state: str_of(r.get("state")),
            });
        }
    }
    let merged = utc(raw.get("mergedAt"));
    Some(PrFact {
        number,
        title: str_of(raw.get("title")),
        body: str_of(raw.get("body")),
        url: str_of(raw.get("url")),
        author: login_of(raw.get("author")),
        branch: str_of(raw.get("headRefName")),
        labels: list(raw.get("labels"))
            .iter()
            .filter_map(object)
            .map(|l| str_of(l.get("name")))
            .filter(|n| !n.is_empty())
            .collect(),
        created_at: utc(raw.get("createdAt")),
        merged_at: (!merged.is_empty()).then_some(merged),
        state: str_of(raw.get("state")),
        commits: list(raw.get("commits"))
            .iter()
            .filter_map(commit_of)
            .collect(),
        files: list(raw.get("files"))
            .iter()
            .filter_map(object)
            .map(|f| str_of(f.get("path")))
            .filter(|p| !p.is_empty())
            .collect(),
        reviews,
    })
}

/// `viewPr` (`src/lib/thanatos-gather.ts:68`).
fn view_pr(exec: &dyn Exec, repo: &str, number: u64) -> Result<PrFact, GatherError> {
    let n = number.to_string();
    let what = format!("gh pr view {number}");
    let r = exec.run("gh", &["pr", "view", &n, "-R", repo, "--json", PR_FIELDS]);
    pr_of(&json(&r, &what)?)
        .ok_or_else(|| GatherError(format!("PR #{number} não encontrada em {repo}")))
}

struct Listed {
    number: u64,
    title: String,
    branch: String,
    body: String,
}

/// `numbersOf` (`src/lib/thanatos-gather.ts:74`).
fn numbers_of(r: &ProcResult, what: &str) -> Result<Vec<Listed>, GatherError> {
    let doc = json(r, what)?;
    Ok(list(Some(&doc))
        .iter()
        .filter_map(object)
        .filter_map(|p| {
            Some(Listed {
                number: number_of(p.get("number").filter(|n| n.is_number()))?,
                title: str_of(p.get("title")),
                branch: str_of(p.get("headRefName")),
                body: str_of(p.get("body")),
            })
        })
        .collect())
}

/// `gitCommits` (`src/lib/thanatos-gather.ts:82`): commits do `git log` do intervalo (autor e data de autoria em UTC).
pub fn git_commits(exec: &dyn Exec, from: &str, to: &str) -> Result<Vec<CommitFact>, GatherError> {
    let since = format!("--since={from}");
    let until = format!("--until={to}");
    let format = format!("--format=%H{UNIT}%an{UNIT}%aI{UNIT}%s");
    let r = exec.run("git", &["log", &since, &until, "--no-merges", &format]);
    if r.status != 0 {
        return Err(GatherError(format!("git log: {}", last_line(&r.stderr))));
    }
    let mut commits = Vec::new();
    for line in r.stdout.split('\n') {
        let mut parts = line.split(UNIT);
        let sha = parts.next().unwrap_or("");
        let author = parts.next().unwrap_or("");
        let at = parts.next().unwrap_or("");
        let subject = parts.next().unwrap_or("");
        if let (false, Some(ts)) = (sha.is_empty(), to_utc(at)) {
            commits.push(CommitFact {
                sha: sha.to_owned(),
                subject: subject.to_owned(),
                author: author.to_owned(),
                ts,
            });
        }
    }
    Ok(commits)
}

/// `gather` (`src/lib/thanatos-gather.ts:95`).
pub fn gather(
    input: &Input,
    exec: &dyn Exec,
    options: &GatherOptions,
) -> Result<Gathered, GatherError> {
    let repo = options.repo.as_str();
    let limit = options.max_prs.to_string();
    match input {
        Input::Pr { number } => Ok(Gathered {
            prs: vec![view_pr(exec, repo, *number as u64)?],
            commits: Vec::new(),
        }),
        Input::Range { from, to } => {
            let search = format!("merged:{from}..{to}");
            let found = numbers_of(
                &exec.run(
                    "gh",
                    &[
                        "pr",
                        "list",
                        "-R",
                        repo,
                        "--state",
                        "merged",
                        "--search",
                        &search,
                        "--limit",
                        &limit,
                        "--json",
                        "number,title,headRefName,body",
                    ],
                ),
                "gh pr list",
            )?;
            let prs = found
                .iter()
                .map(|p| view_pr(exec, repo, p.number))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Gathered {
                prs,
                commits: git_commits(exec, from, to)?,
            })
        }
        Input::Card { id } => {
            let needle = id.to_lowercase();
            let found = numbers_of(
                &exec.run(
                    "gh",
                    &[
                        "pr",
                        "list",
                        "-R",
                        repo,
                        "--state",
                        "merged",
                        "--search",
                        id,
                        "--limit",
                        &limit,
                        "--json",
                        "number,title,headRefName,body",
                    ],
                ),
                "gh pr list",
            )?;
            let prs = found
                .iter()
                .filter(|p| {
                    format!("{}\n{}\n{}", p.title, p.branch, p.body)
                        .to_lowercase()
                        .contains(&needle)
                })
                .map(|p| view_pr(exec, repo, p.number))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Gathered {
                prs,
                commits: Vec::new(),
            })
        }
    }
}

const NOISE: [&str; 2] = ["tool_pre", "tool_post"];

/// `journalFacts` (`src/lib/thanatos-gather.ts:116`): eventos do diário do projeto (`slug`) no período, sem o ruído
/// de ferramenta, os `max` mais recentes; diário ausente = vazio.
pub fn journal_facts(
    slug: &str,
    from: &str,
    to: &str,
    max: usize,
    root: &Path,
) -> Vec<JournalFact> {
    let (lo, hi) = (parse(from), parse(to));
    let mut facts = Vec::new();
    for entry in journal::files(root, false) {
        if entry.project != slug {
            continue;
        }
        for line in journal::read_tail_default(&entry.file).split('\n') {
            let Some(e) = (!line.is_empty()).then(|| journal::parse(line)).flatten() else {
                continue;
            };
            if NOISE.contains(&e.kind.as_str()) {
                continue;
            }
            let (Some(at), Some(lo), Some(hi)) = (parse(&e.ts), lo, hi) else {
                continue;
            };
            if at >= lo && at <= hi {
                facts.push(JournalFact {
                    ts: e.ts,
                    agent: e.agent,
                    kind: e.kind,
                    summary: e.summary,
                });
            }
        }
    }
    facts.sort_by(|a, b| cmp_utf16(&a.ts, &b.ts));
    let skip = facts.len().saturating_sub(max);
    facts.drain(..skip);
    facts
}

/// `windowOf` (`src/lib/thanatos-gather.ts:131`): janela do diário para a entrada: o intervalo, ou da abertura da
/// primeira PR à última mergeada.
pub fn window_of(input: &Input, gathered: &Gathered) -> Option<(String, String)> {
    if let Input::Range { from, to } = input {
        return Some((from.clone(), to.clone()));
    }
    let mut starts: Vec<&str> = gathered
        .prs
        .iter()
        .flat_map(|p| {
            std::iter::once(p.created_at.as_str()).chain(p.commits.iter().map(|c| c.ts.as_str()))
        })
        .filter(|t| !t.is_empty())
        .collect();
    starts.sort_by(|a, b| cmp_utf16(a, b));
    let mut ends: Vec<&str> = gathered
        .prs
        .iter()
        .filter_map(|p| p.merged_at.as_deref())
        .collect();
    ends.sort_by(|a, b| cmp_utf16(a, b));
    Some((starts.first()?.to_string(), ends.last()?.to_string()))
}

#[cfg(test)]
mod tests;
