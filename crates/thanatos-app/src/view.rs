//! `ThanatosView`: casca (topo, rail, status) e as telas Post-mortems e Fontes.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
    relative,
};
use gpui_component::input::{InputEvent, InputState};
use hades_core::Env;
use hades_ui::components::{
    Align, Button, Card, Confirm, CopyButton, DataTable, EmptyState, MarkdownView, PageHeader,
    Rail, RailGroup, RailItem, Segmented, Skeleton, Spinner, StatusBadge, StatusBar, TermBlock,
    TextField, Timeline, TopBar,
};
use hades_ui::tokens::space;
use hades_ui::{Tone, palette};

use crate::service::{self, Contexto, Criado, Documento};
use crate::vm::novo::{self, AVISO_REFAZER, Confirmacao, Modo, Validacao};
use crate::vm::{self, COLUNAS_LISTA, Tarefa, Tela};
use thanatos_core::model::IndexEntry;

const PERIODO_MS: u64 = 1000;
const TIQUES_LISTA: u32 = 5;
const FRACAO_LISTA: f32 = 0.34;

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
    ctx: Option<Contexto>,
    campo: Entity<InputState>,
    modo: Modo,
    validacao: Validacao,
    resultado: Option<Result<Criado, String>>,
    confirmacao: Confirmacao,
    t_criar: Tarefa,
    t_editor: Tarefa,
}

impl ThanatosView {
    pub fn new(
        env: Env,
        cwd: PathBuf,
        foco: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let campo = cx.new(|cx| InputState::new(window, cx).placeholder(Modo::default().dica()));
        cx.subscribe_in(&campo, window, |v, _, e: &InputEvent, _, cx| match e {
            InputEvent::Change => {
                v.revalidar(cx);
                cx.notify();
            }
            InputEvent::PressEnter { .. } => v.gerar(cx),
            _ => {}
        })
        .detach();
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
            ctx: None,
            campo,
            modo: Modo::default(),
            validacao: Validacao::Vazia,
            resultado: None,
            confirmacao: Confirmacao::default(),
            t_criar: Tarefa::default(),
            t_editor: Tarefa::default(),
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
        let (env, cwd, ctx) = (self.env.clone(), self.cwd.clone(), self.ctx.clone());
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move {
                    let ctx = match ctx {
                        Some(c) => c,
                        None => service::contexto(&env, &cwd).map_err(|e| e.to_string())?,
                    };
                    let lista = service::listar(&ctx.dir);
                    Ok::<_, String>((ctx, lista))
                })
                .await;
            let _ = this.update(cx, |v, cx| {
                if !v.t_lista.concluir(g) {
                    return;
                }
                match r {
                    Ok((ctx, lista)) => {
                        v.dir = Some(ctx.dir.clone());
                        v.ctx = Some(ctx);
                        v.revalidar(cx);
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

    fn revalidar(&mut self, cx: &mut Context<Self>) {
        let texto = self.campo.read(cx).value();
        let padrao = self.ctx.as_ref().and_then(|c| c.card_pattern.as_deref());
        self.validacao = novo::validar(self.modo, &texto, padrao);
    }

    fn mudar_modo(&mut self, modo: Modo, window: &mut Window, cx: &mut Context<Self>) {
        self.modo = modo;
        self.campo
            .update(cx, |c, cx| c.set_placeholder(modo.dica(), window, cx));
        self.revalidar(cx);
        cx.notify();
    }

    /// Por que "Gerar" está desligado, ou `None` se pode gerar.
    fn motivo_sem_gerar(&self) -> Option<&'static str> {
        if self.ctx.is_none() {
            return Some("projeto ainda não carregado");
        }
        if self.t_criar.carregando() {
            return Some("gerando…");
        }
        let Some(id) = self.validacao.id() else {
            return Some("informe uma entrada válida");
        };
        self.lista
            .iter()
            .any(|e| e.id == id)
            .then_some("já existe: use Refazer em Post-mortems")
    }

    fn gerar(&mut self, cx: &mut Context<Self>) {
        if self.motivo_sem_gerar().is_some() {
            return;
        }
        let texto = self.campo.read(cx).value().to_string();
        self.criar(texto, false, cx);
    }

    /// Roda `criar` no executor de fundo; ao voltar, mostra o resultado e abre o documento gravado.
    fn criar(&mut self, texto: String, force: bool, cx: &mut Context<Self>) {
        let Some(ctx) = self.ctx.clone() else {
            return;
        };
        let Some(g) = self.t_criar.iniciar() else {
            return;
        };
        self.resultado = None;
        let env = self.env.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { service::criar(&env, &ctx, &texto, force) })
                .await;
            let _ = this.update(cx, |v, cx| {
                if !v.t_criar.concluir(g) {
                    return;
                }
                if let Ok(c) = &r {
                    v.selecionado = Some(c.id.clone());
                    v.documento = None;
                    v.recarregar(cx);
                    v.abrir_selecionado(cx);
                }
                v.resultado = Some(r);
                cx.notify();
            });
        })
        .detach();
    }

    /// Refazer confirmado: regera a partir da entrada gravada no `.json`, com `force`.
    fn refazer(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.confirmacao.confirmar() else {
            return;
        };
        let texto = match &self.documento {
            Some(d) if d.postmortem.id == id => novo::entrada_do_documento(&d.postmortem.input),
            _ => return,
        };
        self.tela = Tela::Novo;
        self.criar(texto, true, cx);
    }

    /// Abre o `.md` no editor do sistema, fora da thread da interface.
    fn editar(&mut self, cx: &mut Context<Self>) {
        let Some(caminho) = self.documento.as_ref().map(|d| d.caminho_md.clone()) else {
            return;
        };
        let Some(g) = self.t_editor.iniciar() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { service::abrir_no_editor(&caminho) })
                .await;
            let _ = this.update(cx, |v, cx| {
                if !v.t_editor.concluir(g) {
                    return;
                }
                if let Err(e) = r {
                    v.aviso = Some(e);
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

    fn esqueleto(&self, id: &'static str) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(space::SM))
            .child(Skeleton::new(format!("{id}-1")))
            .child(Skeleton::new(format!("{id}-2")))
            .child(Skeleton::new(format!("{id}-3")))
            .into_any_element()
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
        let item = |t: Tela| {
            let item = RailItem::new(t.rotulo());
            if t == Tela::Postmortems {
                item.badge(self.lista.len().to_string())
            } else {
                item
            }
        };
        Rail::new([
            RailGroup::new("Consulta", [item(Tela::Postmortems), item(Tela::Fontes)]),
            RailGroup::new("Geração", [item(Tela::Novo)]),
        ])
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
            if self.t_documento.carregando() {
                return self.esqueleto("documento").into_any_element();
            }
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
        let mut coluna = div().flex().flex_col().gap(px(space::LG)).child(cabecalho);
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

    fn botoes_do_documento(&self, cx: &mut Context<Self>) -> (Button, Button) {
        let e = cx.entity();
        let mut refazer = Button::new("refazer", "Refazer");
        let mut editar = Button::new("editar", "Editar .md");
        match &self.documento {
            None => {
                refazer = refazer.disabled("nenhum documento aberto");
                editar = editar.disabled("nenhum documento aberto");
            }
            Some(doc) => {
                if self.t_criar.carregando() {
                    refazer = refazer.disabled("gerando…");
                } else {
                    let id = doc.postmortem.id.clone();
                    let e = e.clone();
                    refazer = refazer.on_click(move |_, _, cx| {
                        let id = id.clone();
                        e.update(cx, |v, cx| {
                            v.confirmacao.pedir(&id);
                            cx.notify();
                        });
                    });
                }
                if doc.markdown.is_none() {
                    editar = editar.disabled("o .md foi removido");
                } else {
                    editar = editar.on_click(move |_, _, cx| e.update(cx, |v, cx| v.editar(cx)));
                }
            }
        }
        (refazer, editar)
    }

    fn tela_postmortems(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (refazer, editar) = self.botoes_do_documento(cx);
        let cabecalho = PageHeader::new("Post-mortems")
            .action(editar)
            .action(refazer)
            .action(Button::new("recarregar", "Recarregar").on_click({
                let e = cx.entity();
                move |_, _, cx| e.update(cx, |v, cx| v.recarregar(cx))
            }));
        let coluna = div().flex().flex_col().gap(px(space::LG)).child(cabecalho);
        if self.lista.is_empty() {
            let (titulo, dica) = if self.t_lista.carregando() {
                ("Carregando…", "Lendo a pasta de post-mortems.")
            } else {
                (
                    "Nenhum post-mortem",
                    "Rode `thanatos new <pr|intervalo|card>` para gerar o primeiro.",
                )
            };
            if self.t_lista.carregando() {
                return coluna.child(self.esqueleto("lista")).into_any_element();
            }
            return coluna
                .child(EmptyState::new(titulo).hint(dica))
                .into_any_element();
        }
        let corpo = div()
            .flex()
            .gap(px(space::LG))
            .child(
                div()
                    .flex_none()
                    .w(relative(FRACAO_LISTA))
                    .child(self.tabela_da_lista(cx)),
            )
            .child(
                div()
                    .id("documento")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .child(self.painel_do_documento()),
            );
        coluna.child(corpo).into_any_element()
    }

    fn tela_novo(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let e = cx.entity();
        let modos = Segmented::new("modo", Modo::TODOS.map(Modo::rotulo))
            .selected(self.modo.indice())
            .on_select(move |i, window, cx| {
                e.update(cx, |v, cx| v.mudar_modo(Modo::de_indice(i), window, cx));
            });
        let campo = TextField::new(&self.campo)
            .label("Entrada")
            .error_opt(self.validacao.erro().map(str::to_owned));
        let mut gerar = Button::new("gerar", "Gerar").primary();
        gerar = match self.motivo_sem_gerar() {
            Some(motivo) => gerar.disabled(motivo),
            None => {
                let e = cx.entity();
                gerar.on_click(move |_, _, cx| e.update(cx, |v, cx| v.gerar(cx)))
            }
        };
        let mut formulario = Card::new()
            .title("Novo post-mortem")
            .child(modos)
            .child(campo)
            .child(div().flex().gap(px(space::SM)).child(gerar));
        if let Some(id) = self.validacao.id() {
            formulario = formulario.child(format!("Será gravado como {id}."));
        }
        let mut coluna = div()
            .flex()
            .flex_col()
            .gap(px(space::LG))
            .child(PageHeader::new("Novo"))
            .child(formulario);
        if self.t_criar.carregando() {
            coluna = coluna.child(
                Card::new().title("Gerando…").child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::SM))
                        .child(Spinner::new("gerando"))
                        .child(StatusBadge::new("coletando", Tone::Busy))
                        .child("git, gh, diário e fonte externa"),
                ),
            );
        }
        match &self.resultado {
            Some(Ok(c)) => {
                let mut bloco = TermBlock::new();
                for l in novo::linhas_do_resultado(c) {
                    bloco = bloco.line(l);
                }
                coluna = coluna.child(Card::new().title("Gravado").child(bloco));
            }
            Some(Err(msg)) => {
                coluna = coluna.child(Card::new().title("Não gerou").child(msg.clone()));
            }
            None => {}
        }
        coluna.into_any_element()
    }

    fn tela_fontes(&self) -> gpui::AnyElement {
        let cabecalho = PageHeader::new("Fontes");
        let coluna = div().flex().flex_col().gap(px(space::LG)).child(cabecalho);
        let Some(doc) = &self.documento else {
            return coluna
                .child(EmptyState::new("Nenhum post-mortem aberto").hint(
                    "Escolha um post-mortem em Post-mortems para ver o que cada fonte trouxe.",
                ))
                .into_any_element();
        };
        let pm = &doc.postmortem;
        let mut cartoes = div().flex().gap(px(space::MD));
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
            Tela::Novo => self.tela_novo(cx),
            Tela::Fontes => self.tela_fontes(),
        };
        let confirmar = self.confirmacao.pendente().map(|id| {
            let (sim, nao) = (cx.entity(), cx.entity());
            Confirm::new(format!("Refazer {id}?"))
                .detail(AVISO_REFAZER)
                .target(
                    self.documento
                        .as_ref()
                        .map(|d| d.caminho_md.display().to_string())
                        .unwrap_or_default(),
                )
                .confirm_label("Refazer")
                .on_confirm(move |_, _, cx| {
                    sim.update(cx, |v, cx| {
                        v.refazer(cx);
                        cx.notify();
                    });
                })
                .on_cancel(move |_, _, cx| {
                    nao.update(cx, |v, cx| {
                        v.confirmacao.cancelar();
                        cx.notify();
                    });
                })
        });
        div()
            .id("thanatos")
            .track_focus(&self.foco)
            .on_key_down(cx.listener(|v, ev: &KeyDownEvent, _, cx| {
                if v.tela == Tela::Novo {
                    return;
                }
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
                    .min_h_0()
                    .child(rail)
                    .child(div().flex_1().p(px(space::LG)).child(corpo)),
            )
            .child(StatusBar::new(esquerda, direita))
            .children(confirmar)
    }
}

pub fn abrir_janela(cx: &mut App) {
    let aberta = cx.open_window(gpui::WindowOptions::default(), |window, cx| {
        window.set_window_title(crate::NOME);
        let cwd = std::env::current_dir().unwrap_or_default();
        let view: Entity<ThanatosView> = cx.new(|cx| {
            ThanatosView::iniciar(cx);
            ThanatosView::new(Env::from_process(), cwd, cx.focus_handle(), window, cx)
        });
        view
    });
    if aberta.is_err() {
        cx.quit();
    }
}
