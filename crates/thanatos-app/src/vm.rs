//! View-model do app: formatação, linhas e tons, sem janela nem I/O.

use hades_ui::Tone;
use thanatos_core::model::{IndexEntry, Postmortem, SourceStatus, TimelineEntry, TimelineSource};
use thanatos_core::render::format_duration;

pub mod novo;

/// Telas da navegação lateral (`design/thanatos.html`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tela {
    Postmortems,
    Novo,
    Fontes,
}

impl Tela {
    pub const TODAS: [Tela; 3] = [Tela::Postmortems, Tela::Novo, Tela::Fontes];

    pub fn rotulo(self) -> &'static str {
        match self {
            Tela::Postmortems => "Post-mortems",
            Tela::Novo => "Novo",
            Tela::Fontes => "Fontes",
        }
    }

    pub fn icone(self) -> hades_ui::icons::IconName {
        use hades_ui::icons::IconName;
        match self {
            Tela::Postmortems => IconName::Skull,
            Tela::Novo => IconName::Plus,
            Tela::Fontes => IconName::History,
        }
    }

    /// Nome de tela aceito em `--tela`.
    pub fn de_nome(nome: &str) -> Option<Tela> {
        match nome {
            "postmortems" => Some(Tela::Postmortems),
            "novo" => Some(Tela::Novo),
            "fontes" => Some(Tela::Fontes),
            _ => None,
        }
    }

    pub fn de_rotulo(rotulo: &str) -> Option<Tela> {
        Tela::TODAS.into_iter().find(|t| t.rotulo() == rotulo)
    }
}

/// Atalho de teclado: `1`, `2` e `3` trocam de tela.
pub fn tela_da_tecla(tecla: &str) -> Option<Tela> {
    match tecla {
        "1" => Some(Tela::Postmortems),
        "2" => Some(Tela::Novo),
        "3" => Some(Tela::Fontes),
        _ => None,
    }
}

/// Controle de uma tarefa de fundo: no máximo uma em voo e resultado de geração velha é descartado (`invalidar`
/// quando a entrada mudou: outra pasta, outro documento).
#[derive(Debug, Default)]
pub struct Tarefa {
    geracao: u64,
    em_voo: bool,
}

impl Tarefa {
    /// Geração da nova execução, ou `None` se a anterior ainda não terminou.
    pub fn iniciar(&mut self) -> Option<u64> {
        if self.em_voo {
            return None;
        }
        self.em_voo = true;
        Some(self.geracao)
    }

    /// Descarta o que estiver em voo e libera uma nova execução.
    pub fn invalidar(&mut self) {
        self.geracao = self.geracao.wrapping_add(1);
        self.em_voo = false;
    }

    /// `true` se o resultado da geração `geracao` ainda vale; só então libera a próxima execução.
    pub fn concluir(&mut self, geracao: u64) -> bool {
        if geracao != self.geracao {
            return false;
        }
        self.em_voo = false;
        true
    }

    pub fn carregando(&self) -> bool {
        self.em_voo
    }
}

pub const COLUNAS_LISTA: [&str; 3] = ["Id", "Criado (UTC)", "Lacunas"];

/// `2026-11-21T13:02:44.000Z` → `2026-11-21 13:02`; texto que não é ISO volta como veio.
pub fn data_hora(ts: &str) -> String {
    match (ts.get(..10), ts.get(11..16)) {
        (Some(dia), Some(hora)) if ts.as_bytes().get(10) == Some(&b'T') => format!("{dia} {hora}"),
        _ => ts.to_owned(),
    }
}

/// `13:02` da linha do tempo; texto que não é ISO volta como veio.
pub fn hora(ts: &str) -> String {
    match ts.get(11..16) {
        Some(h) if ts.as_bytes().get(10) == Some(&b'T') => h.to_owned(),
        _ => ts.to_owned(),
    }
}

pub fn celulas_lista(e: &IndexEntry) -> [String; 3] {
    [e.id.clone(), data_hora(&e.created_at), e.gaps.to_string()]
}

/// O id selecionado só vale se ainda está na lista recarregada; senão cai no primeiro (ou em nenhum).
pub fn selecao_valida(lista: &[IndexEntry], atual: Option<&str>) -> Option<String> {
    atual
        .filter(|id| lista.iter().any(|e| e.id == *id))
        .map(str::to_owned)
        .or_else(|| lista.first().map(|e| e.id.clone()))
}

pub fn indice_do_id(lista: &[IndexEntry], id: Option<&str>) -> Option<usize> {
    id.and_then(|id| lista.iter().position(|e| e.id == id))
}

pub fn rotulo_da_fonte(fonte: TimelineSource) -> &'static str {
    match fonte {
        TimelineSource::Git => "git",
        TimelineSource::Gh => "gh",
        TimelineSource::Diario => "diário",
        TimelineSource::Fonte => "plugin",
    }
}

/// A cor identifica a origem do evento, não o estado.
pub fn tom_da_fonte(fonte: TimelineSource) -> Tone {
    match fonte {
        TimelineSource::Git => Tone::Info,
        TimelineSource::Gh => Tone::Vivo,
        TimelineSource::Diario => Tone::Neutral,
        TimelineSource::Fonte => Tone::Warn,
    }
}

/// Uma linha da linha do tempo: hora UTC, texto, rótulo e tom da fonte.
#[derive(Clone, Debug, PartialEq)]
pub struct LinhaDoTempo {
    pub hora: String,
    pub texto: String,
    pub fonte: &'static str,
    pub tom: Tone,
}

pub fn linhas_do_tempo(entradas: &[TimelineEntry]) -> Vec<LinhaDoTempo> {
    entradas
        .iter()
        .map(|e| LinhaDoTempo {
            hora: hora(&e.ts),
            texto: e.text.clone(),
            fonte: rotulo_da_fonte(e.source),
            tom: tom_da_fonte(e.source),
        })
        .collect()
}

const SECAO_LINHA_DO_TEMPO: &str = "## Linha do tempo";

/// O `.md` sem a seção da linha do tempo, que o app desenha com o componente próprio (hora em mono, cor só na fonte).
pub fn sem_linha_do_tempo(markdown: &str) -> String {
    let mut fora = false;
    let mut saida: Vec<&str> = Vec::new();
    for linha in markdown.lines() {
        if linha.starts_with("## ") {
            fora = linha.starts_with(SECAO_LINHA_DO_TEMPO);
        }
        if !fora {
            saida.push(linha);
        }
    }
    saida.join("\n")
}

/// Título da página do documento: `pr-1234 · fila travava o pronto`.
pub fn titulo_do_documento(pm: &Postmortem) -> String {
    format!("{} · {}", pm.id, pm.title)
}

/// Tempo até a correção, no formato do `.md`, ou `—` quando a fonte não o sabe.
pub fn tempo_ate_a_correcao(pm: &Postmortem) -> String {
    pm.fix_data
        .time_to_fix
        .as_ref()
        .map_or_else(|| "—".to_owned(), |t| format_duration(t.ms))
}

pub fn rotulo_do_status(status: SourceStatus) -> &'static str {
    match status {
        SourceStatus::Ok => "ok",
        SourceStatus::Absent => "ausente",
        SourceStatus::NoToken => "sem token",
        SourceStatus::Error => "erro",
    }
}

pub fn tom_do_status(status: SourceStatus) -> Tone {
    match status {
        SourceStatus::Ok => Tone::Ok,
        SourceStatus::Absent => Tone::Neutral,
        SourceStatus::NoToken => Tone::Warn,
        SourceStatus::Error => Tone::Bad,
    }
}

fn plural(n: usize, singular: &str, plural: &str) -> String {
    format!("{n} {}", if n == 1 { singular } else { plural })
}

/// Cartão de uma fonte em "Fontes": o que ela trouxe e, quando falhou, o que faltou.
#[derive(Clone, Debug, PartialEq)]
pub struct CartaoDaFonte {
    pub nome: &'static str,
    pub resumo: String,
    pub selo: &'static str,
    pub tom: Tone,
    pub nota: Option<String>,
}

/// As quatro fontes do post-mortem: git, gh, diário e plugin (`source`). Git e gh são `ok` quando trouxeram algo e
/// `ausente` quando vieram vazios; o diário conta os eventos da linha do tempo; o plugin traz o status que gravou.
pub fn cartoes_das_fontes(pm: &Postmortem) -> Vec<CartaoDaFonte> {
    let commits = pm.fix_data.commits.len();
    let prs = pm.fix_data.prs.len();
    let diario = pm
        .timeline
        .iter()
        .filter(|e| e.source == TimelineSource::Diario)
        .count();
    let simples = |nome, n: usize, unidade: (&str, &str)| CartaoDaFonte {
        nome,
        resumo: plural(n, unidade.0, unidade.1),
        selo: if n > 0 { "ok" } else { "ausente" },
        tom: if n > 0 { Tone::Ok } else { Tone::Neutral },
        nota: None,
    };
    let nota_do_plugin = (!pm.source.note.is_empty()).then(|| pm.source.note.clone());
    let metricas = pm.source.metrics.len();
    vec![
        simples("git", commits, ("commit", "commits")),
        simples("gh", prs, ("PR", "PRs")),
        simples("diário", diario, ("evento", "eventos")),
        CartaoDaFonte {
            nome: "plugin",
            resumo: if metricas == 0 {
                "sem métricas".to_owned()
            } else {
                plural(metricas, "métrica", "métricas")
            },
            selo: rotulo_do_status(pm.source.status),
            tom: tom_do_status(pm.source.status),
            nota: nota_do_plugin,
        },
    ]
}

/// `nome: valor unidade` de cada métrica da fonte externa.
pub fn linhas_das_metricas(pm: &Postmortem) -> Vec<String> {
    pm.source
        .metrics
        .iter()
        .map(|m| {
            let unidade = m.unit.as_deref().unwrap_or("");
            format!(
                "{}: {} {}",
                m.name,
                thanatos_core::jsstr::number_to_string(m.value),
                unidade
            )
            .trim_end()
            .to_owned()
        })
        .collect()
}

pub fn rodape_da_lista(total: usize) -> String {
    plural(total, "post-mortem", "post-mortems")
}

#[cfg(test)]
mod tests;
