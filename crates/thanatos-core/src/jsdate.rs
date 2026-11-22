//! `Date.parse` e `Date#toISOString` do Bun (JavaScriptCore) para a gramática ISO 8601 que o Thanatos recebe de `git`, `gh`,
//! do diário e da linha de comando (`src/lib/thanatos.ts:96-100, 139`). A tabela dos testes foi medida no Bun 1.4.4.
//!
//! O que o JSC aceita além do ISO (`Mar 1 2026 09:00:00 +0000`, `12/25/2026 10:00 +0000`) não é lido aqui: devolve
//! `None`. Data-hora sem fuso o JS lê como hora local; aqui vale UTC, e nenhuma entrada do Thanatos chega sem fuso, porque
//! `toUtc` o exige e `git`/`gh` o escrevem.

const MS_PER_DAY: i64 = 86_400_000;
const TIME_CLIP: i64 = 8_640_000_000_000_000;

/// Dias desde 1970-01-01 do dia 1 do mês (civil proléptico gregoriano).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn digits(&mut self, count: usize) -> Option<i64> {
        let end = self.at.checked_add(count)?;
        let slice = self.bytes.get(self.at..end)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.at = end;
        Some(
            slice
                .iter()
                .fold(0, |acc, b| acc * 10 + i64::from(b - b'0')),
        )
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

struct Time {
    hour: i64,
    minute: i64,
    second: i64,
    millis: i64,
}

fn parse_time(c: &mut Cursor<'_>) -> Option<Time> {
    let hour = c.digits(2)?;
    if !c.eat(b':') {
        return None;
    }
    let minute = c.digits(2)?;
    let mut second = 0;
    let mut millis = 0;
    let mut fraction_zero = true;
    if c.eat(b':') {
        second = c.digits(2)?;
        if c.eat(b'.') {
            let mut count = 0;
            while let Some(b) = c.peek().filter(u8::is_ascii_digit) {
                if count < 3 {
                    millis += i64::from(b - b'0') * [100, 10, 1][count];
                }
                if b != b'0' {
                    fraction_zero = false;
                }
                count += 1;
                c.at += 1;
            }
            if count == 0 {
                return None;
            }
        }
    }
    if hour > 24 || minute > 59 || second > 59 {
        return None;
    }
    if hour == 24 && (minute != 0 || second != 0 || !fraction_zero) {
        return None;
    }
    Some(Time {
        hour,
        minute,
        second,
        millis,
    })
}

fn parse_offset(c: &mut Cursor<'_>) -> Option<i64> {
    if c.eat(b'Z') || c.eat(b'z') {
        return Some(0);
    }
    let sign = match c.peek()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    c.at += 1;
    let hour = c.digits(2)?;
    c.eat(b':');
    let minute = c.digits(2)?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(sign * (hour * 60 + minute))
}

/// `Date.parse(text)` em milissegundos desde a época; `None` é `NaN`.
pub fn parse(text: &str) -> Option<i64> {
    let mut c = Cursor {
        bytes: text.as_bytes(),
        at: 0,
    };
    let year = match c.peek()? {
        sign @ (b'+' | b'-') => {
            c.at += 1;
            let value = c.digits(6)?;
            if sign == b'-' {
                if value == 0 {
                    return None;
                }
                -value
            } else {
                value
            }
        }
        _ => c.digits(4)?,
    };
    let mut month = 1;
    let mut day = 1;
    if c.eat(b'-') {
        month = c.digits(2)?;
        if c.eat(b'-') {
            day = c.digits(2)?;
        }
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let date_ms = (days_from_civil(year, month, 1) + day - 1) * MS_PER_DAY;
    let (time, offset_min) = match c.peek() {
        Some(b'T' | b't') => {
            c.at += 1;
            let time = parse_time(&mut c)?;
            (time, parse_offset_opt(&mut c)?)
        }
        Some(b' ') => {
            while c.eat(b' ') {}
            let time = parse_time(&mut c)?;
            (time, parse_offset_opt(&mut c)?)
        }
        _ => {
            let zulu = c.eat(b'Z') || c.eat(b'z');
            if !zulu && !c.done() {
                return None;
            }
            (
                Time {
                    hour: 0,
                    minute: 0,
                    second: 0,
                    millis: 0,
                },
                0,
            )
        }
    };
    if !c.done() {
        return None;
    }
    let time_ms = ((time.hour * 60 + time.minute) * 60 + time.second) * 1000 + time.millis;
    let value = date_ms + time_ms - offset_min * 60_000;
    (-TIME_CLIP..=TIME_CLIP).contains(&value).then_some(value)
}

fn parse_offset_opt(c: &mut Cursor<'_>) -> Option<i64> {
    if c.done() { Some(0) } else { parse_offset(c) }
}

/// `new Date(ms).toISOString()`.
pub fn iso(ms: i64) -> String {
    let days = ms.div_euclid(MS_PER_DAY);
    let rest = ms.rem_euclid(MS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else if year < 0 {
        format!("-{:06}", -year)
    } else {
        format!("+{year:06}")
    };
    format!(
        "{year}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000
    )
}

/// `new Date(ms).toISOString().replace(".000Z", "Z")`.
pub fn iso_z(ms: i64) -> String {
    iso(ms).replace(".000Z", "Z")
}

/// `toUtc` (`src/lib/thanatos.ts:96`): instante ISO com fuso em UTC; sem fuso não vale como hora local.
pub fn to_utc(text: &str) -> Option<String> {
    let text = crate::jsstr::trim(text);
    if !ends_with_zone(text) {
        return None;
    }
    parse(text).map(iso_z)
}

/// `/(Z|[+-]\d{2}:?\d{2})$/.test(text)`.
fn ends_with_zone(text: &str) -> bool {
    if text.ends_with('Z') {
        return true;
    }
    let b = text.as_bytes();
    let tail_ok = |tail: &[u8], colon: bool| {
        let (sign, rest) = (tail[0], &tail[1..]);
        (sign == b'+' || sign == b'-')
            && if colon {
                rest[2] == b':' && rest[..2].iter().chain(&rest[3..]).all(u8::is_ascii_digit)
            } else {
                rest.iter().all(u8::is_ascii_digit)
            }
    };
    (b.len() >= 6 && tail_ok(&b[b.len() - 6..], true))
        || (b.len() >= 5 && tail_ok(&b[b.len() - 5..], false))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Medidos com `Date.parse` no Bun 1.4.4; `None` é `NaN`.
    const TABELA: &[(&str, Option<i64>)] = &[
        ("2026-03-01T09:00:00-03:00", Some(1_772_366_400_000)),
        ("2026-03-01T09:00:00Z", Some(1_772_355_600_000)),
        ("2026-03-01T09:00:00+0000", Some(1_772_355_600_000)),
        ("2026-03-01T09:00Z", Some(1_772_355_600_000)),
        ("2026-03-01T09:00:00.5Z", Some(1_772_355_600_500)),
        ("2026-03-01T09:00:00.123456Z", Some(1_772_355_600_123)),
        ("2026-03-01T09:00:00.1239Z", Some(1_772_355_600_123)),
        ("2026-03-01Z", Some(1_772_323_200_000)),
        ("2026-03Z", Some(1_772_323_200_000)),
        ("2026Z", Some(1_767_225_600_000)),
        ("2026-03-01T09Z", None),
        ("2026-03-01 09:00:00Z", Some(1_772_355_600_000)),
        ("2026-03-01 09:00:00+03:00", Some(1_772_344_800_000)),
        ("2026-03-01  09:00:00Z", Some(1_772_355_600_000)),
        ("2026-03-01T24:00:00Z", Some(1_772_409_600_000)),
        ("2026-03-01T24:00:00.000Z", Some(1_772_409_600_000)),
        ("2026-03-01T24:00:00.5Z", None),
        ("2026-03-01T24:00:00.00001Z", None),
        ("2026-03-01T24:00:01Z", None),
        ("2026-03-01T24:01:00Z", None),
        ("2026-02-30T00:00:00Z", Some(1_772_409_600_000)),
        ("2026-02-31T00:00:00Z", Some(1_772_496_000_000)),
        ("2026-02-32T00:00:00Z", None),
        ("2026-13-01T00:00:00Z", None),
        ("2026-00-10T00:00:00Z", None),
        ("2026-01-00T00:00:00Z", None),
        ("2026-03-01T09:00:00z", Some(1_772_355_600_000)),
        ("2026-03-01t09:00:00Z", Some(1_772_355_600_000)),
        ("+002026-03-01T00:00:00Z", Some(1_772_323_200_000)),
        ("-000001-03-01T00:00:00Z", Some(-62_193_657_600_000)),
        ("+2026-03-01T00:00:00Z", None),
        ("-000000-01-01T00:00:00Z", None),
        ("2026-03-01T09:00:00+24:00", None),
        ("2026-03-01T09:00:00+23:59", Some(1_772_269_260_000)),
        ("2026-03-01T09:00:00+00:60", None),
        ("2026-03-01T09:00:00-00:00", Some(1_772_355_600_000)),
        ("2026-03-01T09:00:00-00:30", Some(1_772_357_400_000)),
        ("2026-3-1T09:00:00Z", None),
        ("2026-03-01T9:00:00Z", None),
        ("2026-03-01T09:00:00.Z", None),
        ("2026-03-01T09:00:00,5Z", None),
        ("2026-03-01T09:00:00+03", None),
        ("2026-03-01T09:00:00 Z", None),
        ("2026-03-01T09:00:60Z", None),
        ("2026-03-01T09:60:00Z", None),
        ("2026-03-01T09:00:00+00:00 ", None),
        (" 2026-03-01T09:00:00Z", None),
        ("2026-03-01T09:00:00-3:00", None),
        ("2026-03-01T09:00:00-0300", Some(1_772_366_400_000)),
        ("2026-03-01+03:00", None),
        ("2026-03-01T09:00:00+0300Z", None),
        ("0000-01-01T00:00:00Z", Some(-62_167_219_200_000)),
        ("+275760-09-13T00:00:00.000Z", Some(8_640_000_000_000_000)),
        ("+275760-09-13T00:00:00.001Z", None),
        ("-271821-04-20T00:00:00Z", Some(-8_640_000_000_000_000)),
        ("-271821-04-19T23:59:59.999Z", None),
        ("1970-01-01T00:00:00Z", Some(0)),
        ("2024-02-29T00:00:00Z", Some(1_709_164_800_000)),
        ("2025-02-29T00:00:00Z", Some(1_740_787_200_000)),
        ("", None),
        ("x", None),
    ];

    #[test]
    fn parse_segue_o_bun() {
        for (text, want) in TABELA {
            assert_eq!(parse(text), *want, "{text:?}");
        }
    }

    #[test]
    fn iso_como_to_iso_string() {
        assert_eq!(iso(1_772_355_600_500), "2026-03-01T09:00:00.500Z");
        assert_eq!(iso(-1), "1969-12-31T23:59:59.999Z");
        assert_eq!(iso(-62_198_755_200_000), "-000001-01-01T00:00:00.000Z");
        assert_eq!(iso(253_402_300_800_000), "+010000-01-01T00:00:00.000Z");
        assert_eq!(iso(8_640_000_000_000_000), "+275760-09-13T00:00:00.000Z");
        assert_eq!(iso(-8_640_000_000_000_000), "-271821-04-20T00:00:00.000Z");
        assert_eq!(iso_z(1_772_355_600_000), "2026-03-01T09:00:00Z");
        assert_eq!(iso_z(1_772_355_600_500), "2026-03-01T09:00:00.500Z");
    }

    #[test]
    fn ida_e_volta() {
        for ms in [0, 1_772_355_600_123, -62_135_683_200_000, 951_782_400_000] {
            assert_eq!(parse(&iso(ms)), Some(ms));
        }
    }

    #[test]
    fn to_utc_exige_fuso() {
        assert_eq!(
            to_utc("2026-03-01T09:00:00-03:00").as_deref(),
            Some("2026-03-01T12:00:00Z")
        );
        assert_eq!(
            to_utc(" 2026-03-01T09:00:00+0000 ").as_deref(),
            Some("2026-03-01T09:00:00Z")
        );
        assert_eq!(
            to_utc("2026-03-01T09:00:00.500Z").as_deref(),
            Some("2026-03-01T09:00:00.500Z")
        );
        assert_eq!(to_utc("2026-03-01T09:00:00"), None);
        assert_eq!(to_utc("2026-03-01"), None);
        assert_eq!(to_utc("2026-03-01T09:00:00z"), None);
        assert_eq!(to_utc("x"), None);
        assert_eq!(to_utc("2026-13-01T09:00:00Z"), None);
    }
}
