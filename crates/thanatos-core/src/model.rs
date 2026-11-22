//! Tipos do post-mortem (`src/lib/thanatos.ts:22-87, 342-351, 391`). A ordem dos campos é a das chaves do TS: o `.json`
//! gravado e o `--json` saem byte a byte iguais.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `THANATOS_VERSION` (`src/lib/thanatos.ts:10`).
pub const THANATOS_VERSION: f64 = 1.0;

/// `Input` (`src/lib/thanatos.ts:22`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Input {
    Pr { number: f64 },
    Range { from: String, to: String },
    Card { id: String },
}

impl Input {
    /// O `kind` do TS.
    pub fn kind(&self) -> &'static str {
        match self {
            Input::Pr { .. } => "pr",
            Input::Range { .. } => "range",
            Input::Card { .. } => "card",
        }
    }
}

/// `CommitFact` (`src/lib/thanatos.ts:27`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommitFact {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub ts: String,
}

/// `ReviewFact` (`src/lib/thanatos.ts:28`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewFact {
    pub author: String,
    pub ts: String,
    pub state: String,
}

/// `PrFact` (`src/lib/thanatos.ts:29`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrFact {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub url: String,
    pub author: String,
    pub branch: String,
    pub labels: Vec<String>,
    pub created_at: String,
    pub merged_at: Option<String>,
    pub state: String,
    pub commits: Vec<CommitFact>,
    pub files: Vec<String>,
    pub reviews: Vec<ReviewFact>,
}

/// `Gathered` (`src/lib/thanatos.ts:43`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gathered {
    pub prs: Vec<PrFact>,
    pub commits: Vec<CommitFact>,
}

/// `SourceStatus` (`src/lib/thanatos.ts:45`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceStatus {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "absent")]
    Absent,
    #[serde(rename = "no-token")]
    NoToken,
    #[serde(rename = "error")]
    Error,
}

/// `SourceEvent` (`src/lib/thanatos.ts:46`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceEvent {
    pub ts: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

/// `SourceMetric` (`src/lib/thanatos.ts:47`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceMetric {
    pub name: String,
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

/// `SourceResult` (`src/lib/thanatos.ts:48`): o que a fonte externa devolveu.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceResult {
    pub status: SourceStatus,
    /// O que faltou ou o que veio, uma frase.
    pub note: String,
    pub events: Vec<SourceEvent>,
    pub metrics: Vec<SourceMetric>,
    /// Início do incidente, quando a fonte sabe (ISO UTC).
    pub start: Option<String>,
}

/// `JournalFact` (`src/lib/thanatos.ts:58`).
#[derive(Clone, Debug, PartialEq)]
pub struct JournalFact {
    pub ts: String,
    pub agent: String,
    pub kind: String,
    pub summary: String,
}

/// `TimelineSource` (`src/lib/thanatos.ts:60`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineSource {
    #[serde(rename = "git")]
    Git,
    #[serde(rename = "gh")]
    Gh,
    #[serde(rename = "diario")]
    Diario,
    #[serde(rename = "fonte")]
    Fonte,
}

impl TimelineSource {
    /// O texto do TS, que a linha do tempo do `.md` imprime.
    pub fn as_str(self) -> &'static str {
        match self {
            TimelineSource::Git => "git",
            TimelineSource::Gh => "gh",
            TimelineSource::Diario => "diario",
            TimelineSource::Fonte => "fonte",
        }
    }
}

/// `TimelineEntry` (`src/lib/thanatos.ts:61`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub ts: String,
    pub source: TimelineSource,
    pub text: String,
}

/// `TimeToFix.basis` (`src/lib/thanatos.ts:63`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Basis {
    #[serde(rename = "fonte")]
    Fonte,
    #[serde(rename = "intervalo")]
    Intervalo,
    #[serde(rename = "primeiro-commit")]
    PrimeiroCommit,
    #[serde(rename = "abertura-da-pr")]
    AberturaDaPr,
}

impl Basis {
    pub fn as_str(self) -> &'static str {
        match self {
            Basis::Fonte => "fonte",
            Basis::Intervalo => "intervalo",
            Basis::PrimeiroCommit => "primeiro-commit",
            Basis::AberturaDaPr => "abertura-da-pr",
        }
    }
}

/// `TimeToFix` (`src/lib/thanatos.ts:63`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimeToFix {
    pub ms: f64,
    pub from: String,
    pub to: String,
    pub basis: Basis,
}

/// Elemento de `FixData.prs` (`src/lib/thanatos.ts:65`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixPr {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub url: String,
    pub branch: String,
    pub merged_at: Option<String>,
}

/// `FixData` (`src/lib/thanatos.ts:64`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixData {
    pub prs: Vec<FixPr>,
    pub commits: Vec<CommitFact>,
    pub files: Vec<String>,
    pub authors: Vec<String>,
    pub time_to_fix: Option<TimeToFix>,
}

/// O `source` gravado no post-mortem: a fonte sem os eventos (`src/lib/thanatos.ts:83`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceSummary {
    pub status: SourceStatus,
    pub note: String,
    pub metrics: Vec<SourceMetric>,
}

/// `Postmortem` (`src/lib/thanatos.ts:70`). Contrato versionado (`v`): campo novo só se acrescenta. `extra` guarda os
/// campos de uma `v` maior, que `show --json` devolve como o arquivo os tem.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Postmortem {
    pub v: f64,
    pub id: String,
    pub input: Input,
    pub created_at: String,
    pub title: String,
    pub summary: String,
    pub impact: String,
    pub timeline: Vec<TimelineEntry>,
    pub cause: String,
    pub fix: String,
    pub fix_data: FixData,
    pub source: SourceSummary,
    pub gaps: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `IndexEntry` (`src/lib/thanatos.ts:342`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub kind: String,
    pub prs: Vec<u64>,
    pub files: usize,
    pub gaps: usize,
    pub time_to_fix_ms: Option<f64>,
}

/// `SearchHit` (`src/lib/thanatos.ts:375`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub line: String,
}

/// `Citation` (`src/lib/thanatos.ts:391`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    pub file: String,
    pub id: String,
    pub title: String,
    pub merged_at: Option<String>,
}
