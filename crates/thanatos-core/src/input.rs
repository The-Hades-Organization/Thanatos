//! Entrada de `thanatos new` e id do post-mortem (`src/lib/thanatos.ts:103-135`).

use hades_core::output::{HadesError, exit};

use crate::jsdate;
use crate::jsstr::{number_to_string, trim};
use crate::model::Input;

const INTERVAL_SEPARATOR: &str = "..";

fn usage(message: String) -> HadesError {
    HadesError::with_code(message, exit::USAGE)
}

/// `/^#?(\d+)$/` (`PR_INPUT`, `src/lib/thanatos.ts:12`): `\d` é só ASCII.
fn pr_number(raw: &str) -> Option<f64> {
    let digits = raw.strip_prefix('#').unwrap_or(raw);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<f64>().ok()
}

/// `new RegExp("^(?:" + pattern + ")$", "i").test(raw)`; expressão inválida não casa.
fn card_matches(pattern: &str, raw: &str) -> bool {
    regress::Regex::with_flags(&format!("^(?:{pattern})$"), "i")
        .is_ok_and(|re| re.find(raw).is_some())
}

/// `parseInput` (`src/lib/thanatos.ts:103`): PR (`123` ou `#123`), intervalo `<iso>..<iso>` com fuso nas duas pontas ou
/// id de card pelo `card.idPattern` do perfil, sem caixa. `card_pattern` é uma regex JS.
pub fn parse_input(text: &str, card_pattern: Option<&str>) -> Result<Input, HadesError> {
    let raw = trim(text);
    if let Some(number) = pr_number(raw) {
        return Ok(Input::Pr { number });
    }
    if let Some(cut) = raw.find(INTERVAL_SEPARATOR).filter(|cut| *cut > 0) {
        let from = jsdate::to_utc(&raw[..cut]);
        let to = jsdate::to_utc(&raw[cut + INTERVAL_SEPARATOR.len()..]);
        let (Some(from), Some(to)) = (from, to) else {
            return Err(usage(format!(
                "intervalo não entendido: {raw} (use <iso>..<iso>, cada ponta com Z ou fuso)"
            )));
        };
        if jsdate::parse(&from) >= jsdate::parse(&to) {
            return Err(usage(format!("intervalo vazio ou invertido: {raw}")));
        }
        return Ok(Input::Range { from, to });
    }
    if let Some(pattern) = card_pattern.filter(|p| !p.is_empty())
        && card_matches(pattern, raw)
    {
        return Ok(Input::Card { id: raw.to_owned() });
    }
    Err(usage(format!(
        "entrada não entendida: {raw} (número de PR, id de card do perfil ou intervalo <iso>..<iso>)"
    )))
}

/// `slugPart`: minúsculas, o que não é `[a-z0-9]` vira `-`, sem `-` nas pontas.
fn slug_part(text: &str) -> String {
    let mut out = String::new();
    let mut pending = false;
    for c in text.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if pending && !out.is_empty() {
                out.push('-');
            }
            pending = false;
            out.push(c);
        } else {
            pending = true;
        }
    }
    out
}

fn compact(iso: &str) -> String {
    iso.chars().filter(|c| *c != '-' && *c != ':').collect()
}

/// `idOf` (`src/lib/thanatos.ts:131`): `pr-<n>`, `card-<id>` ou `range-<de>-<ate>`.
pub fn id_of(input: &Input) -> String {
    match input {
        Input::Pr { number } => format!("pr-{}", number_to_string(*number)),
        Input::Card { id } => format!("card-{}", slug_part(id)),
        Input::Range { from, to } => format!("range-{}-{}", compact(from), compact(to)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str) -> Input {
        Input::Card { id: id.to_owned() }
    }

    #[test]
    fn numero_de_pr_com_ou_sem_cerquilha_vira_pr() {
        assert_eq!(parse_input("12", None), Ok(Input::Pr { number: 12.0 }));
        assert_eq!(parse_input("#12", None), Ok(Input::Pr { number: 12.0 }));
        assert_eq!(parse_input(" 12 ", None), Ok(Input::Pr { number: 12.0 }));
        assert!(parse_input("1 2", None).is_err());
        assert!(parse_input("#", None).is_err());
        assert!(parse_input("\u{0661}\u{0662}", None).is_err());
    }

    #[test]
    fn intervalo_normaliza_as_pontas_para_utc() {
        assert_eq!(
            parse_input("2026-03-01T09:00:00-03:00..2026-03-01T18:00:00Z", None),
            Ok(Input::Range {
                from: "2026-03-01T12:00:00Z".into(),
                to: "2026-03-01T18:00:00Z".into()
            })
        );
    }

    #[test]
    fn intervalo_sem_fuso_vazio_ou_invertido_e_recusado_com_exit_2() {
        for bad in [
            "2026-03-01T09:00:00..2026-03-01T18:00:00Z",
            "2026-03-02T00:00:00Z..2026-03-01T00:00:00Z",
            "2026-03-01T00:00:00Z..2026-03-01T00:00:00Z",
            "x..y",
        ] {
            let err = parse_input(bad, None).unwrap_err();
            assert_eq!(err.code, exit::USAGE, "{bad}");
        }
        let msg = parse_input("x..y", None).unwrap_err().message;
        assert_eq!(
            msg,
            "intervalo não entendido: x..y (use <iso>..<iso>, cada ponta com Z ou fuso)"
        );
    }

    #[test]
    fn separador_no_inicio_nao_e_intervalo() {
        let err = parse_input("..2026-03-01T00:00:00Z", None).unwrap_err();
        assert!(err.message.starts_with("entrada não entendida"));
    }

    #[test]
    fn id_de_card_so_vale_pelo_id_pattern_do_perfil_sem_caixa() {
        assert_eq!(
            parse_input("fix-77", Some("FIX-[0-9]+")),
            Ok(card("fix-77"))
        );
        assert!(parse_input("fix-77", None).is_err());
        assert!(parse_input("fix-77", Some("")).is_err());
        assert!(parse_input("OUTRO-1", Some("FIX-[0-9]+")).is_err());
        assert!(parse_input("FIX-77x", Some("FIX-[0-9]+")).is_err());
    }

    #[test]
    fn id_pattern_com_alternativa_fica_ancorado_inteiro() {
        assert_eq!(parse_input("B-1", Some("A-1|B-1")), Ok(card("B-1")));
        assert!(parse_input("B-12", Some("A-1|B-1")).is_err());
    }

    #[test]
    fn regex_invalida_nao_casa() {
        let err = parse_input("FIX-1", Some("FIX-(")).unwrap_err();
        assert_eq!(err.code, exit::USAGE);
    }

    #[test]
    fn ids_sao_estaveis_e_sem_caracteres_de_caminho() {
        assert_eq!(id_of(&Input::Pr { number: 12.0 }), "pr-12");
        assert_eq!(id_of(&card("FIX-77/../x")), "card-fix-77-x");
        assert_eq!(id_of(&card("--A  b--")), "card-a-b");
        assert_eq!(
            id_of(&Input::Range {
                from: "2026-03-01T12:00:00Z".into(),
                to: "2026-03-01T18:00:00Z".into()
            }),
            "range-20260301T120000Z-20260301T180000Z"
        );
    }
}
