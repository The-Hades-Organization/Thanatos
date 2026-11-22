//! `ThanatosView`: casca (topo, rail, status) e as telas Post-mortems e Fontes.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use hades_core::Env;
use hades_ui::components::{
    Align, Button, Card, CopyButton, DataTable, EmptyState, MarkdownView, PageHeader, Rail,
    RailGroup, RailItem, StatusBadge, StatusBar, TermBlock, Timeline, TopBar,
};
use hades_ui::{Tone, palette};

use crate::service::{self, Documento};
use crate::vm::{self, COLUNAS_LISTA, Tarefa, Tela};
use thanatos_core::model::IndexEntry;

const PERIODO_MS: u64 = 1000;
const TIQUES_LISTA: u32 = 5;
const LARGURA_LISTA: f32 = 380.0;

pub struct ThanatosView {
    env: Env,
    cwd: PathBuf,
    tela: Tela,
    dir: Option<PathBuf>,
    lista: Vec<IndexEntry>,
    selecionado: Option<String>,
    documento: Option<Documento>,
    aviso: Option<String>,
    foco: FocusHandle,
    tique: u32,
    t_lista: Tarefa,
    t_documento: Tarefa,
}

impl ThanatosView {
    pub fn new(env: Env, cwd: PathBuf, foco: FocusHandle, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            tela: Tela::Postmortems,
            dir: None,
            lista: Vec::new(),
            selecionado: None,
            documento: None,
            aviso: None,
            foco,
            tique: 0,
            t_lista: Tarefa::default(),
            t_documento: Tarefa::default(),
            env,
            cwd,
        };
        view.recarregar(cx);
        view
    }

    /// Resolve a pasta e lista os post-mortems no executor de fundo; ao voltar, reabre o documento selecionado.
    fn recarregar(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.t_lista.iniciar() else {
            return;
        };
        let (env, cwd, dir) = (self.env.clone(), self.cwd.clone(), self.dir.clone());
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move {
                    let dir = match dir {
                        Some(d) => d,
                        None => service::pasta(&env, &cwd).map_err(|e| e.to_string())?,
                    };
                    let lista = service::listar(&dir);
                    Ok::<_, String>((dir, lista))
                })
                .await;
            let _ = this.update(cx, |v, cx| {
                if !v.t_lista.concluir(g) {
                    return;
                }
                match r {
                    Ok((dir, lista)) => {
                        v.dir = Some(dir);
                        let novo = vm::selecao_valida(&lista, v.selecionado.as_deref());
                        let mudou = novo != v.selecionado;
                        v.lista = lista;
                        v.selecionado = novo;
                        v.aviso = None;
                        if mudou || v.documento.is_none() {
                            v.abrir_selecionado(cx);
                        }
                    }
                    Err(e) => v.aviso = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Lê `.json` e `.md` do selecionado no executor de fundo; trocar de selecionado descarta a leitura em voo.
    fn abrir_selecionado(&mut self, cx: &mut Context<Self>) {
        self.t_documento.invalidar();
        let (Some(dir), Some(id)) = (self.dir.clone(), self.selecionado.clone()) else {
            self.documento = None;
            return;
        };
        let Some(g) = self.t_documento.iniciar() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { service::abrir(&dir, &id).map_err(|e| e.to_string()) })
                .await;
            let _ = this.update(cx, |v, cx| {
                if !v.t_documento.concluir(g) {
                    return;
                }
                match r {
                    Ok(doc) => {
                        v.documento = doc;
                        v.aviso = None;
                    }
                    Err(e) => {
                        v.documento = None;
                        v.aviso = Some(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn selecionar(&mut self, indice: usize, cx: &mut Context<Self>) {
        let Some(e) = self.lista.get(indice) else {
            return;
        };
        if self.selecionado.as_deref() == Some(e.id.as_str()) {
            return;
        }
        self.selecionado = Some(e.id.clone());
        self.documento = None;
        self.abrir_selecionado(cx);
    }

    fn tiquetaque(&mut self, cx: &mut Context<Self>) {
        self.tique = self.tique.wrapping_add(1);
        if self.tique.is_multiple_of(TIQUES_LISTA) {
            self.recarregar(cx);
        }
    }

    pub fn iniciar(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(PERIODO_MS))
                    .await;
                if this.update(cx, |v, cx| v.tiquetaque(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn mudar_tela(&mut self, tela: Tela, cx: &mut Context<Self>) {
        self.tela = tela;
        self.recarregar(cx);
    }

    fn rodape(&self) -> (String, String) {
        let esquerda = self.aviso.clone().unwrap_or_else(|| {
            if self.t_lista.carregando() || self.t_documento.carregando() {
                "carregando…".to_owned()
            } else {
                vm::rodape_da_lista(self.lista.len())
            }
        });
        let direita = match (&self.dir, &self.selecionado) {
            (Some(d), Some(id)) => format!("{}/{id}.json", d.display()),
            (Some(d), None) => d.display().to_string(),
            _ => format!("thanatos {}", env!("CARGO_PKG_VERSION")),
        };
        (esquerda, direita)
    }

    fn rail(&self, cx: &mut Context<Self>) -> Rail {
        let entidade = cx.entity();
        let itens = Tela::TODAS.map(|t| {
            let item = RailItem::new(t.rotulo());
            if t == Tela::Postmortems {
                item.badge(self.lista.len().to_string())
            } else {
                item
            }
        });
        Rail::new([RailGroup::new("Thanatos", itens)])
            .selected(self.tela.rotulo())
            .on_select(move |rotulo, _, cx| {
                if let Some(t) = Tela::de_rotulo(rotulo) {
                    entidade.update(cx, |v, cx| {
                        v.mudar_tela(t, cx);
                        cx.notify();
                    });
                }
            })
    }

    fn titulo(&self) -> String {
        match (&self.selecionado, self.tela) {
            (Some(id), Tela::Fontes) => format!("Thanatos · {id}"),
            _ => "Thanatos".to_owned(),
        }
    }

    fn tabela_da_lista(&self, cx: &mut Context<Self>) -> DataTable {
        let entidade = cx.entity();
        let colunas = COLUNAS_LISTA.iter().map(|n| {
            let alinhamento = if *n == "Lacunas" {
                Align::Numeric
            } else {
                Align::Start
            };
            (SharedString::from((*n).to_owned()), alinhamento)
        });
        let mut tabela = DataTable::new(colunas.collect::<Vec<_>>());
        for e in &self.lista {
            tabela = tabela.row(vm::celulas_lista(e).into_iter().map(SharedString::from));
        }
        if let Some(i) = vm::indice_do_id(&self.lista, self.selecionado.as_deref()) {
            tabela = tabela.selected(i);
        }
        tabela.on_select(move |i, _, cx| {
            entidade.update(cx, |v, cx| {
                v.selecionar(i, cx);
                cx.notify();
            });
        })
    }

    fn painel_do_documento(&self) -> gpui::AnyElement {
        let Some(doc) = &self.documento else {
            let (titulo, dica) = if self.t_documento.carregando() {
                ("Carregando…", "Lendo o post-mortem.")
            } else {
                (
                    "Documento ilegível",
                    "O .json não pôde ser lido; o .md segue no disco.",
                )
            };
            return EmptyState::new(titulo).hint(dica).into_any_element();
        };
        let pm = &doc.postmortem;
        let caminho = doc.caminho_md.display().to_string();
        let cabecalho = PageHeader::new(vm::titulo_do_documento(pm))
            .action(StatusBadge::new(
                format!("até a correção: {}", vm::tempo_ate_a_correcao(pm)),
                Tone::Neutral,
            ))
            .action(CopyButton::new("copiar-caminho", caminho).label("Copiar caminho"));
        let mut coluna = div().flex().flex_col().gap(px(14.0)).child(cabecalho);
        coluna = match &doc.markdown {
            Some(md) => coluna.child(MarkdownView::new(vm::sem_linha_do_tempo(md))),
            None => coluna
                .child(EmptyState::new("Sem .md").hint("O .json existe, mas o .md foi removido.")),
        };
        let mut linha = Timeline::new();
        for l in vm::linhas_do_tempo(&pm.timeline) {
            linha = linha.sourced(l.hora, l.texto, l.fonte, l.tom);
        }
        coluna
            .child(Card::new().title("Linha do tempo (UTC)").child(linha))
            .into_any_element()
    }

    fn tela_postmortems(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let cabecalho = PageHeader::new("Post-mortems").action(
            Button::new("recarregar", "Recarregar").on_click({
                let e = cx.entity();
                move |_, _, cx| e.update(cx, |v, cx| v.recarregar(cx))
            }),
        );
        let coluna = div().flex().flex_col().gap(px(14.0)).child(cabecalho);
        if self.lista.is_empty() {
            let (titulo, dica) = if self.t_lista.carregando() {
                ("Carregando…", "Lendo a pasta de post-mortems.")
            } else {
                (
                    "Nenhum post-mortem",
                    "Rode `thanatos new <pr|intervalo|card>` para gerar o primeiro.",
                )
            };
            return coluna
                .child(EmptyState::new(titulo).hint(dica))
                .into_any_element();
        }
        let corpo = div()
            .flex()
            .gap(px(16.0))
            .child(
                div()
                    .flex_none()
                    .w(px(LARGURA_LISTA))
                    .child(self.tabela_da_lista(cx)),
            )
            .child(
                div()
                    .id("documento")
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_y_scroll()
                    .child(self.painel_do_documento()),
            );
        coluna.child(corpo).into_any_element()
    }

    fn tela_fontes(&self) -> gpui::AnyElement {
        let cabecalho = PageHeader::new("Fontes");
        let coluna = div().flex().flex_col().gap(px(14.0)).child(cabecalho);
        let Some(doc) = &self.documento else {
            return coluna
                .child(EmptyState::new("Nenhum post-mortem aberto").hint(
                    "Escolha um post-mortem em Post-mortems para ver o que cada fonte trouxe.",
                ))
                .into_any_element();
        };
        let pm = &doc.postmortem;
        let mut cartoes = div().flex().gap(px(12.0));
        for c in vm::cartoes_das_fontes(pm) {
            let mut cartao = Card::new()
                .title(c.nome)
                .child(StatusBadge::new(c.selo, c.tom))
                .child(c.resumo);
            if let Some(nota) = c.nota {
                cartao = cartao.child(nota);
            }
            cartoes = cartoes.child(div().flex_1().child(cartao));
        }
        let mut coluna = coluna.child(cartoes);
        let metricas = vm::linhas_das_metricas(pm);
        if !metricas.is_empty() {
            let mut bloco = TermBlock::new();
            for m in metricas {
                bloco = bloco.line(m);
            }
            coluna = coluna.child(Card::new().title("Métricas da fonte externa").child(bloco));
        }
        let mut lacunas = Card::new().title("Lacunas");
        if pm.gaps.is_empty() {
            lacunas = lacunas.child("Nenhuma lacuna registrada.");
        }
        for g in &pm.gaps {
            lacunas = lacunas.child(g.clone());
        }
        coluna.child(lacunas).into_any_element()
    }
}

impl Render for ThanatosView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let (esquerda, direita) = self.rodape();
        let rail = self.rail(cx);
        let corpo = match self.tela {
            Tela::Postmortems => self.tela_postmortems(cx),
            Tela::Fontes => self.tela_fontes(),
        };
        div()
            .id("thanatos")
            .track_focus(&self.foco)
            .on_key_down(cx.listener(|v, ev: &KeyDownEvent, _, cx| {
                if let Some(t) = vm::tela_da_tecla(&ev.keystroke.key) {
                    v.mudar_tela(t, cx);
                    cx.notify();
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .text_color(p.fg)
            .child(TopBar::new(self.titulo()).trailing("gpui"))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .min_h(px(0.0))
                    .child(rail)
                    .child(div().flex_1().p(px(16.0)).child(corpo)),
            )
            .child(StatusBar::new(esquerda, direita))
    }
}

pub fn abrir_janela(cx: &mut App) {
    let aberta = cx.open_window(gpui::WindowOptions::default(), |window, cx| {
        window.set_window_title(crate::NOME);
        let cwd = std::env::current_dir().unwrap_or_default();
        let view: Entity<ThanatosView> = cx.new(|cx| {
            ThanatosView::iniciar(cx);
            ThanatosView::new(Env::from_process(), cwd, cx.focus_handle(), cx)
        });
        view
    });
    if aberta.is_err() {
        cx.quit();
    }
}
