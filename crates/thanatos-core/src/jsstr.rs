//! Comportamento de `string` e `number` do JS que o Rust não tem pronto: espaço de `\s`/`trim`, ordem por unidade
//! UTF-16, `Number#toString` e `slice` em unidades UTF-16.

use std::cmp::Ordering;

use serde_json::{Number, Value, json};

/// Espaço de `\s` e de `String#trim` do ECMAScript (WhiteSpace e LineTerminator): inclui U+FEFF e exclui U+0085.
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `String#trim`.
pub fn trim(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

/// `text.replace(/\s+/g, " ").trim()` (`flat`, `src/lib/thanatos.ts:141`).
pub fn flat(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if is_js_space(c) {
            in_space = true;
        } else {
            if in_space {
                out.push(' ');
                in_space = false;
            }
            out.push(c);
        }
    }
    if in_space {
        out.push(' ');
    }
    trim(&out).to_owned()
}

/// Comparação `<`/`>` de strings do JS: por unidade UTF-16, que difere da ordem de byte UTF-8 para caractere fora do
/// plano básico.
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `Number#toString` (e `${n}`): o mesmo texto que o `JSON.stringify` dá a um número finito.
pub fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let number = Number::from_f64(value).map_or(Value::Null, Value::Number);
    hades_core::jsonfmt::json_compact(&json!(number))
}

/// `text.slice(0, max)` em unidades UTF-16. Um par substituto cortado ao meio vira U+FFFD, que é o que o stdout do
/// Node/Bun grava para um substituto solto.
pub fn slice_utf16(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut units = 0;
    for c in text.chars() {
        let width = c.len_utf16();
        if units + width > max {
            if units < max {
                out.push('\u{FFFD}');
            }
            break;
        }
        out.push(c);
        units += width;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_junta_espacos_e_apara() {
        assert_eq!(flat("  a \n\t b\u{00A0}\u{FEFF}c  "), "a b c");
        assert_eq!(flat(""), "");
        assert_eq!(flat("a\u{0085}b"), "a\u{0085}b");
    }

    #[test]
    fn ordem_por_unidade_utf16() {
        assert_eq!(cmp_utf16("\u{FF5E}", "\u{1F600}"), Ordering::Greater);
        assert!("\u{FF5E}" < "\u{1F600}");
        assert_eq!(cmp_utf16("a", "b"), Ordering::Less);
        assert_eq!(cmp_utf16("a", "a"), Ordering::Equal);
    }

    #[test]
    fn numero_como_o_js() {
        assert_eq!(number_to_string(42.0), "42");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(0.1), "0.1");
        assert_eq!(number_to_string(1e21), "1e+21");
        assert_eq!(number_to_string(1e-7), "1e-7");
        assert_eq!(
            number_to_string(100_000_000_000_000_000_000.0),
            "100000000000000000000"
        );
    }

    #[test]
    fn slice_corta_em_unidades() {
        assert_eq!(slice_utf16("abcdef", 3), "abc");
        assert_eq!(slice_utf16("ab\u{1F600}", 3), "ab\u{FFFD}");
        assert_eq!(slice_utf16("ab\u{1F600}", 4), "ab\u{1F600}");
        assert_eq!(slice_utf16("ab", 10), "ab");
    }
}
