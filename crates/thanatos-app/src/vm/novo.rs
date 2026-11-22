//! View-model da tela Novo e do Refazer: validação ao digitar, modo da entrada, confirmação e resumo do resultado.
//! Sem janela nem I/O.

use thanatos_core::input::{id_of, parse_input};
use thanatos_core::model::Input;

use crate::service::Criado;

/// As três entradas de `thanatos new` (`design/thanatos.html`, tela Novo).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Modo {
    #[default]
    Pr,
    Intervalo,
    Card,
}

impl Modo {
    pub const TODOS: [Modo; 3] = [Modo::Pr, Modo::Intervalo, Modo::Card];

    pub fn rotulo(self) -> &'static str {
        match self {
            Modo::Pr => "PR",
            Modo::Intervalo => "Intervalo",
            Modo::Card => "Card",
        }
    }

    pub fn de_indice(indice: usize) -> Modo {
        Modo::TODOS.get(indice).copied().unwrap_or_default()
    }

    pub fn indice(self) -> usize {
        Modo::TODOS.iter().position(|m| *m == self).unwrap_or(0)
    }

    pub fn dica(self) -> &'static str {
        match self {
            Modo::Pr => "123 ou #123",
            Modo::Intervalo => "2026-11-20T10:00:00-03:00..2026-11-20T12:00:00-03:00",
            Modo::Card => "id do card, pelo padrão do perfil",
        }
    }

    fn do_input(input: &Input) -> Modo {
        match input {
            Input::Pr { .. } => Modo::Pr,
            Input::Range { .. } => Modo::Intervalo,
            Input::Card { .. } => Modo::Card,
        }
    }
}

/// Estado do campo a cada tecla.
#[derive(Clone, Debug, PartialEq)]
pub enum Validacao {
    /// Campo vazio: sem erro e sem botão.
    Vazia,
    /// Entrada aceita; `id` é o do documento que será gravado.
    Valida {
        input: Input,
        id: String,
    },
    Erro(String),
}

impl Validacao {
    pub fn erro(&self) -> Option<&str> {
        match self {
            Validacao::Erro(m) => Some(m),
            _ => None,
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Validacao::Valida { id, .. } => Some(id),
            _ => None,
        }
    }
}

/// Valida `texto` com o `parse_input` do core; recusa o que não é do modo escolhido (um número de PR no modo
/// Intervalo, por exemplo). Intervalo sem fuso é recusado aqui, no próprio campo.
pub fn validar(modo: Modo, texto: &str, card_pattern: Option<&str>) -> Validacao {
    if texto.trim().is_empty() {
        return Validacao::Vazia;
    }
    match parse_input(texto, card_pattern) {
        Err(e) => Validacao::Erro(e.to_string()),
        Ok(input) if Modo::do_input(&input) != modo => Validacao::Erro(format!(
            "isso é uma entrada de {}; troque o modo ou corrija o campo",
            Modo::do_input(&input).rotulo()
        )),
        Ok(input) => {
            let id = id_of(&input);
            Validacao::Valida { input, id }
        }
    }
}

/// Refazer pede confirmação na própria tela: a edição à mão do `.md` se perde (D1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Confirmacao {
    #[default]
    Nenhuma,
    Pendente(String),
}

impl Confirmacao {
    pub fn pedir(&mut self, id: &str) {
        *self = Confirmacao::Pendente(id.to_owned());
    }

    pub fn cancelar(&mut self) {
        *self = Confirmacao::Nenhuma;
    }

    /// Fecha a confirmação e devolve o id a refazer, se havia um pendente.
    pub fn confirmar(&mut self) -> Option<String> {
        match std::mem::take(self) {
            Confirmacao::Pendente(id) => Some(id),
            Confirmacao::Nenhuma => None,
        }
    }

    pub fn pendente(&self) -> Option<&str> {
        match self {
            Confirmacao::Pendente(id) => Some(id),
            Confirmacao::Nenhuma => None,
        }
    }
}

pub const AVISO_REFAZER: &str =
    "Regerar sobrescreve o .md e o .json; a edição feita à mão no .md se perde.";

/// O texto que o Refazer manda para `criar`: a entrada reconstruída do `.json` (`7`, `a..b`, id do card).
pub fn entrada_do_documento(input: &Input) -> String {
    match input {
        Input::Pr { number } => thanatos_core::jsstr::number_to_string(*number),
        Input::Range { from, to } => format!("{from}..{to}"),
        Input::Card { id } => id.clone(),
    }
}

/// O resumo que a CLI imprime depois de gravar (`src/commands/thanatos.ts:139`).
pub fn linhas_do_resultado(c: &Criado) -> Vec<String> {
    let mut linhas = vec![
        format!("post-mortem {} gravado", c.id),
        c.md.display().to_string(),
        c.json.display().to_string(),
        format!(
            "{} PR(s), {} evento(s), tempo até a correção: {}",
            c.prs,
            c.eventos,
            if c.tempo_ate_a_correcao == "—" {
                "n/d"
            } else {
                &c.tempo_ate_a_correcao
            }
        ),
    ];
    linhas.extend(c.lacunas.iter().map(|g| format!("lacuna: {g}")));
    linhas
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn vazio_nao_e_erro() {
        assert_eq!(validar(Modo::Pr, "  ", None), Validacao::Vazia);
    }

    #[test]
    fn pr_valida_com_e_sem_cerquilha() {
        assert_eq!(validar(Modo::Pr, "#1234", None).id(), Some("pr-1234"));
        assert_eq!(validar(Modo::Pr, "1234", None).id(), Some("pr-1234"));
    }

    #[test]
    fn lixo_vira_erro_inline() {
        let v = validar(Modo::Pr, "abc", None);
        assert!(v.erro().unwrap().contains("entrada não entendida"));
    }

    #[test]
    fn intervalo_sem_fuso_e_recusado() {
        let v = validar(
            Modo::Intervalo,
            "2026-11-20T10:00:00..2026-11-20T12:00:00",
            None,
        );
        assert!(v.erro().unwrap().contains("intervalo não entendido"));
    }

    #[test]
    fn intervalo_invertido_e_recusado() {
        let v = validar(
            Modo::Intervalo,
            "2026-11-20T12:00:00Z..2026-11-20T10:00:00Z",
            None,
        );
        assert!(v.erro().unwrap().contains("vazio ou invertido"));
    }

    #[test]
    fn intervalo_valido_vira_id_range() {
        let v = validar(
            Modo::Intervalo,
            "2026-11-20T10:00:00-03:00..2026-11-20T12:00:00-03:00",
            None,
        );
        assert!(v.id().unwrap().starts_with("range-"));
    }

    #[test]
    fn card_so_casa_com_o_padrao_do_perfil() {
        assert!(validar(Modo::Card, "SOFTWARE-12", None).erro().is_some());
        let v = validar(Modo::Card, "software-12", Some("SOFTWARE-\\d+"));
        assert!(v.id().unwrap().starts_with("card-"));
    }

    #[test]
    fn entrada_de_outro_modo_e_recusada() {
        let v = validar(Modo::Intervalo, "123", None);
        assert!(v.erro().unwrap().contains("entrada de PR"));
    }

    #[test]
    fn modo_por_indice_do_segmented() {
        assert_eq!(Modo::de_indice(1), Modo::Intervalo);
        assert_eq!(Modo::de_indice(9), Modo::Pr);
        assert_eq!(Modo::Card.indice(), 2);
    }

    #[test]
    fn confirmacao_do_refazer() {
        let mut c = Confirmacao::default();
        assert_eq!(c.confirmar(), None);
        c.pedir("pr-1");
        assert_eq!(c.pendente(), Some("pr-1"));
        c.cancelar();
        assert_eq!(c.pendente(), None);
        c.pedir("pr-2");
        assert_eq!(c.confirmar().as_deref(), Some("pr-2"));
        assert_eq!(c.pendente(), None);
    }

    #[test]
    fn entrada_do_documento_reconstroi_o_texto() {
        assert_eq!(entrada_do_documento(&Input::Pr { number: 7.0 }), "7");
        let r = Input::Range {
            from: "a".into(),
            to: "b".into(),
        };
        assert_eq!(entrada_do_documento(&r), "a..b");
        assert_eq!(
            entrada_do_documento(&Input::Card { id: "X-1".into() }),
            "X-1"
        );
    }

    #[test]
    fn resultado_traz_caminhos_e_lacunas() {
        let c = Criado {
            id: "pr-1".into(),
            md: PathBuf::from("/d/pr-1.md"),
            json: PathBuf::from("/d/pr-1.json"),
            prs: 1,
            eventos: 4,
            tempo_ate_a_correcao: "—".into(),
            lacunas: vec!["sem fonte".into()],
        };
        let l = linhas_do_resultado(&c);
        assert_eq!(l[0], "post-mortem pr-1 gravado");
        assert!(l[3].ends_with("n/d"));
        assert_eq!(l[4], "lacuna: sem fonte");
    }
}
