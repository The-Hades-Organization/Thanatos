//! Ajustes numéricos do `thanatos` (`src/commands/thanatos.ts:31-32, 69-72`).

use std::path::Path;

use hades_core::config::config_value;
use hades_core::env::Env;

use crate::jsstr::trim;

/// `ThanatosContext.settings` (`src/commands/thanatos.ts:46`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub source_timeout_s: u32,
    pub io_timeout_s: u32,
    pub max_prs: u32,
    pub journal_max: u32,
}

/// `DEFAULTS` (`src/commands/thanatos.ts:31`).
pub const DEFAULTS: Settings = Settings {
    source_timeout_s: 60,
    io_timeout_s: 60,
    max_prs: 30,
    journal_max: 40,
};

/// `MAX` (`src/commands/thanatos.ts:32`).
pub const MAX: Settings = Settings {
    source_timeout_s: 600,
    io_timeout_s: 600,
    max_prs: 200,
    journal_max: 500,
};

/// `Number.parseInt(text, 10)`: espaço inicial, sinal opcional e o maior prefixo de dígitos; `None` onde o JS dá `NaN`.
/// Valor que estoura `i64` satura (fica fora de qualquer faixa, como no JS).
pub fn parse_int(text: &str) -> Option<i64> {
    let text = trim(text);
    let (negative, rest) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let value = digits.parse::<i64>().unwrap_or(i64::MAX);
    Some(if negative { -value } else { value })
}

/// `intSetting` (`src/commands/thanatos.ts:69`) sobre o texto já resolvido.
pub fn int_setting(raw: &str, default: u32, max: u32) -> u32 {
    match parse_int(raw) {
        Some(n) if n >= 1 && n <= i64::from(max) => n as u32,
        _ => default,
    }
}

fn read(env: &Env, cwd: &Path, key: &str, default: u32, max: u32) -> u32 {
    int_setting(
        &config_value(env, cwd, key, &default.to_string()),
        default,
        max,
    )
}

/// Os quatro ajustes, lidos pelo `config_value` do Hades-Core.
pub fn load(env: &Env, cwd: &Path) -> Settings {
    Settings {
        source_timeout_s: read(
            env,
            cwd,
            "HADES_THANATOS_SOURCE_TIMEOUT_S",
            DEFAULTS.source_timeout_s,
            MAX.source_timeout_s,
        ),
        io_timeout_s: read(
            env,
            cwd,
            "HADES_THANATOS_IO_TIMEOUT_S",
            DEFAULTS.io_timeout_s,
            MAX.io_timeout_s,
        ),
        max_prs: read(
            env,
            cwd,
            "HADES_THANATOS_MAX_PRS",
            DEFAULTS.max_prs,
            MAX.max_prs,
        ),
        journal_max: read(
            env,
            cwd,
            "HADES_THANATOS_JOURNAL_MAX",
            DEFAULTS.journal_max,
            MAX.journal_max,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_int_como_o_js() {
        assert_eq!(parse_int("12x"), Some(12));
        assert_eq!(parse_int("  +7 "), Some(7));
        assert_eq!(parse_int("-3"), Some(-3));
        assert_eq!(parse_int("x1"), None);
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("99999999999999999999"), Some(i64::MAX));
    }

    #[test]
    fn faixa_invalida_vira_o_padrao() {
        assert_eq!(int_setting("12x", 60, 600), 12);
        assert_eq!(int_setting("0", 60, 600), 60);
        assert_eq!(int_setting("601", 60, 600), 60);
        assert_eq!(int_setting("600", 60, 600), 600);
        assert_eq!(int_setting("-1", 60, 600), 60);
        assert_eq!(int_setting("abc", 30, 200), 30);
    }

    #[test]
    fn load_le_o_ambiente() {
        let env = Env::empty()
            .with("HADES_THANATOS_MAX_PRS", "7")
            .with("HADES_THANATOS_JOURNAL_MAX", "9999");
        let dir = tempfile::tempdir().unwrap();
        let s = load(&env, dir.path());
        assert_eq!(s.max_prs, 7);
        assert_eq!(s.journal_max, 40);
        assert_eq!(s.source_timeout_s, 60);
    }
}
