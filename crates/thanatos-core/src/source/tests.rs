use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde_json::json;
use tempfile::TempDir;

use super::*;

const FROM: &str = "2026-03-01T09:00:00Z";
const TO: &str = "2026-03-01T18:00:00Z";

fn script(body: &str) -> String {
    format!("#!/usr/bin/env bash\nset -euo pipefail\n{body}\n")
}

fn plugin(dir: &TempDir, name: &str, commands: &Value, scripts: &[(&str, &str)]) -> PathBuf {
    let env = dir.path().join(name).join("env");
    fs::create_dir_all(&env).expect("env");
    fs::write(
        env.join("plugin.json"),
        json!({"name": name, "commands": commands}).to_string(),
    )
    .expect("plugin.json");
    for (file, body) in scripts {
        let path = env.join(file);
        fs::create_dir_all(path.parent().expect("pai")).expect("dir");
        fs::write(&path, body).expect("script");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    env
}

fn process_env() -> Env {
    Env::from_process()
}

fn fetch(
    env_dir: Option<&Path>,
    credentials: Option<&str>,
    timeout_s: u32,
    env: &Env,
) -> SourceResult {
    fetch_source(
        FROM,
        TO,
        &SourceDeps {
            env_dir,
            credentials,
            timeout_s,
            env,
        },
    )
}

fn decl(script: &str) -> Value {
    json!([{"name": "thanatos-source", "script": script}])
}

const GOOD: &str = r#"echo '{"v":1,"start":"2026-03-01T09:00:00Z","events":[{"ts":"2026-03-01T10:15:00-03:00","text":"erro 500 após '"$1 até $2"' --token valor1234","level":"error"},{"ts":"sem-fuso","text":"descartado"}],"metrics":[{"name":"erros/min","value":42,"unit":"1/min"},{"name":"ruim","value":"x"}]}'"#;

#[test]
fn sem_plugin_ou_sem_o_comando_declarado_ausente_com_o_motivo() {
    let dir = tempfile::tempdir().expect("tmp");
    let env = process_env();
    let none = fetch(None, None, 10, &env);
    assert_eq!(none.status, SourceStatus::Absent);
    assert!(none.note.contains("env/"), "{}", none.note);
    let empty = plugin(&dir, "vazio", &json!([]), &[]);
    let r = fetch(Some(&empty), None, 10, &env);
    assert_eq!(r.status, SourceStatus::Absent);
    assert!(r.note.contains("thanatos-source"), "{}", r.note);
    let semjson = dir.path().join("semjson/env");
    fs::create_dir_all(&semjson).expect("dir");
    let r = fetch(Some(&semjson), None, 10, &env);
    assert_eq!(r.note, "o plugin não traz plugin.json legível");
}

#[test]
fn script_fora_do_env_ou_de_tipo_desconhecido_e_recusado() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("x.sh"), "echo").expect("x");
    let fora = plugin(&dir, "fora", &decl("../x.sh"), &[]);
    assert!(declared_source(Some(&fora)).is_err());
    let abs = plugin(&dir, "abs", &decl("/bin/ls"), &[]);
    assert_eq!(
        declared_source(Some(&abs)),
        Err("thanatos-source: \"script\" precisa ser um caminho relativo ao env/".to_owned())
    );
    let tipo = plugin(
        &dir,
        "tipo",
        &decl("commands/s.py"),
        &[("commands/s.py", "print(1)")],
    );
    assert_eq!(
        declared_source(Some(&tipo)),
        Err("thanatos-source: só scripts .sh e .ts".to_owned())
    );
    let inexistente = plugin(&dir, "nada", &decl("commands/s.sh"), &[]);
    assert!(declared_source(Some(&inexistente)).is_err());
    let vazio = plugin(&dir, "vazio-script", &decl(""), &[]);
    assert!(declared_source(Some(&vazio)).is_err());
}

#[test]
fn link_que_sai_do_env_e_recusado() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("fora.sh"), "echo").expect("fora");
    let env = plugin(&dir, "link", &decl("commands/s.sh"), &[]);
    fs::create_dir_all(env.join("commands")).expect("dir");
    std::os::unix::fs::symlink(dir.path().join("fora.sh"), env.join("commands/s.sh"))
        .expect("link");
    assert!(declared_source(Some(&env)).is_err());
}

#[test]
fn plugin_de_exemplo_eventos_e_metricas_validados_em_utc_e_com_segredo_mascarado() {
    let dir = tempfile::tempdir().expect("tmp");
    let env = plugin(
        &dir,
        "ok",
        &decl("commands/thanatos-source.sh"),
        &[("commands/thanatos-source.sh", &script(GOOD))],
    );
    let r = fetch(Some(&env), None, 10, &process_env());
    assert_eq!(r.status, SourceStatus::Ok, "{}", r.note);
    assert_eq!(r.start.as_deref(), Some(FROM));
    assert_eq!(
        r.events,
        [SourceEvent {
            ts: "2026-03-01T13:15:00Z".into(),
            text: format!("erro 500 após {FROM} até {TO} --token ***"),
            level: Some("error".into())
        }]
    );
    assert_eq!(
        r.metrics,
        [SourceMetric {
            name: "erros/min".into(),
            value: 42.0,
            unit: Some("1/min".into()),
            ts: None
        }]
    );
    assert_eq!(r.note, "1 evento(s) e 1 métrica(s) da fonte");
}

#[test]
fn token_declarado_e_ausente_de_credenciais_no_token_sem_rodar_o_script() {
    let dir = tempfile::tempdir().expect("tmp");
    let commands =
        json!([{"name": "thanatos-source", "script": "commands/s.sh", "tokenKey": "FONTE_TOKEN"}]);
    let body = script("echo '{\"v\":1}' > \"$(dirname \"$0\")/rodou\"; echo '{\"v\":1}'");
    let env = plugin(&dir, "tok", &commands, &[("commands/s.sh", &body)]);
    let r = fetch(Some(&env), Some("OUTRA=1"), 10, &process_env());
    assert_eq!(r.status, SourceStatus::NoToken);
    assert!(r.note.contains("FONTE_TOKEN"));
    assert!(!env.join("commands/rodou").exists());
}

#[test]
fn o_token_chega_ao_script_pela_variavel_do_contrato_e_nao_aparece_no_resultado() {
    let dir = tempfile::tempdir().expect("tmp");
    let commands =
        json!([{"name": "thanatos-source", "script": "commands/s.sh", "tokenKey": "FONTE_TOKEN"}]);
    let body = script(
        r#"echo '{"v":1,"events":[{"ts":"2026-03-01T10:00:00Z","text":"tamanho '"${#HADES_THANATOS_TOKEN}"'"}]}'"#,
    );
    let env = plugin(&dir, "tok2", &commands, &[("commands/s.sh", &body)]);
    let r = fetch(
        Some(&env),
        Some("- FONTE_TOKEN=abcdefghij\n"),
        10,
        &process_env(),
    );
    assert_eq!(r.status, SourceStatus::Ok, "{}", r.note);
    assert!(!format!("{r:?}").contains("abcdefghij"));
    assert_eq!(r.events[0].text, "tamanho 10");
}

#[test]
fn ambiente_do_script_so_tem_path_home_lang_tz_e_o_token() {
    let dir = tempfile::tempdir().expect("tmp");
    let commands =
        json!([{"name": "thanatos-source", "script": "commands/s.sh", "tokenKey": "FONTE_TOKEN"}]);
    let body = script(
        r#"vars=$(compgen -e | grep -E '^(HOME|LANG|PATH|TZ|HADES_THANATOS_TOKEN|SEGREDO)$' | sort | tr '\n' ' ')
echo '{"v":1,"events":[{"ts":"2026-03-01T10:00:00Z","text":"'"$vars$HOME $LANG $TZ"'"}]}'"#,
    );
    let env = plugin(&dir, "amb", &commands, &[("commands/s.sh", &body)]);
    let process = Env::empty()
        .with("PATH", std::env::var("PATH").unwrap_or_default())
        .with("HOME", "/h")
        .with("SEGREDO", "x");
    let r = fetch(Some(&env), Some("FONTE_TOKEN=t"), 10, &process);
    assert_eq!(r.status, SourceStatus::Ok, "{}", r.note);
    assert_eq!(
        r.events[0].text,
        "HADES_THANATOS_TOKEN HOME LANG PATH TZ /h C.UTF-8 UTC"
    );
}

#[test]
fn script_que_falha_passa_do_tempo_ou_devolve_lixo_vira_erro() {
    let dir = tempfile::tempdir().expect("tmp");
    let env = process_env();
    let bad = plugin(
        &dir,
        "rc",
        &decl("commands/s.sh"),
        &[(
            "commands/s.sh",
            &script("echo 'Loki fora do ar' >&2; exit 3"),
        )],
    );
    let r = fetch(Some(&bad), None, 10, &env);
    assert_eq!(
        (r.status, r.note.as_str()),
        (
            SourceStatus::Error,
            "thanatos-source saiu com 3: Loki fora do ar"
        )
    );
    let mudo = plugin(
        &dir,
        "mudo",
        &decl("commands/s.sh"),
        &[("commands/s.sh", &script("exit 4"))],
    );
    assert_eq!(
        fetch(Some(&mudo), None, 10, &env).note,
        "thanatos-source saiu com 4"
    );
    let slow = plugin(
        &dir,
        "lento",
        &decl("commands/s.sh"),
        &[("commands/s.sh", &script("sleep 5"))],
    );
    let r = fetch(Some(&slow), None, 1, &env);
    assert_eq!(
        (r.status, r.note.as_str()),
        (SourceStatus::Error, "thanatos-source passou de 1s")
    );
    let lixo = plugin(
        &dir,
        "lixo",
        &decl("commands/s.sh"),
        &[("commands/s.sh", &script("echo nada"))],
    );
    let r = fetch(Some(&lixo), None, 10, &env);
    assert_eq!(r.status, SourceStatus::Error);
    assert!(r.note.contains("contrato"), "{}", r.note);
}

#[test]
fn stderr_do_script_sai_mascarado_na_nota() {
    let dir = tempfile::tempdir().expect("tmp");
    let bad = plugin(
        &dir,
        "seg",
        &decl("commands/s.sh"),
        &[(
            "commands/s.sh",
            &script("echo 'falhou --token abc12345' >&2; exit 2"),
        )],
    );
    let r = fetch(Some(&bad), None, 10, &process_env());
    assert_eq!(r.note, "thanatos-source saiu com 2: falhou --token ***");
}

#[test]
fn saida_acima_de_4_mib_vira_erro() {
    let dir = tempfile::tempdir().expect("tmp");
    let env = plugin(
        &dir,
        "grande",
        &decl("commands/s.sh"),
        &[(
            "commands/s.sh",
            &script("head -c 5000000 /dev/zero | tr '\\0' a"),
        )],
    );
    let r = fetch(Some(&env), None, 20, &process_env());
    assert_eq!(r.status, SourceStatus::Error);
    assert!(r.note.contains("não executou"), "{}", r.note);
}

#[test]
fn fonte_ts_roda_com_o_bun_do_hades_bun() {
    let dir = tempfile::tempdir().expect("tmp");
    let fake = dir.path().join("fake-bun");
    fs::write(&fake, "#!/usr/bin/env bash\necho \"{\\\"v\\\":1,\\\"events\\\":[{\\\"ts\\\":\\\"2026-03-01T10:00:00Z\\\",\\\"text\\\":\\\"bun $(basename \"$1\") $2\\\"}]}\"\n").expect("fake");
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).expect("chmod");
    let env = plugin(
        &dir,
        "ts",
        &decl("commands/s.ts"),
        &[("commands/s.ts", "// ts")],
    );
    let process = Env::from_process().with(BUN_ENV, fake.to_string_lossy());
    let r = fetch(Some(&env), None, 10, &process);
    assert_eq!(r.status, SourceStatus::Ok, "{}", r.note);
    assert_eq!(r.events[0].text, format!("bun s.ts {FROM}"));
}

#[test]
fn fonte_ts_acha_o_bun_no_path() {
    let dir = tempfile::tempdir().expect("tmp");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&bin).expect("bin");
    let fake = bin.join("bun");
    fs::write(&fake, "#!/usr/bin/env bash\necho '{\"v\":1}'\n").expect("fake");
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).expect("chmod");
    let env = plugin(
        &dir,
        "ts2",
        &decl("commands/s.ts"),
        &[("commands/s.ts", "// ts")],
    );
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let process = Env::empty().with("PATH", path);
    let r = fetch(Some(&env), None, 10, &process);
    assert_eq!(r.status, SourceStatus::Ok, "{}", r.note);
}

#[test]
fn fonte_ts_sem_bun_vira_erro_com_a_causa() {
    let dir = tempfile::tempdir().expect("tmp");
    let env = plugin(
        &dir,
        "ts3",
        &decl("commands/s.ts"),
        &[("commands/s.ts", "// ts")],
    );
    let process = Env::empty().with("PATH", dir.path().to_string_lossy());
    let r = fetch(Some(&env), None, 10, &process);
    assert_eq!(r.status, SourceStatus::Error);
    assert!(r.note.contains("Bun"), "{}", r.note);
    assert!(r.events.is_empty() && r.metrics.is_empty());
}

#[test]
fn parse_da_saida_v_maior_vale_v_ausente_nao() {
    assert_eq!(
        parse_source_output(r#"{"v":2,"campo_novo":1}"#),
        Some(SourceOutput {
            events: vec![],
            metrics: vec![],
            start: None
        })
    );
    assert_eq!(parse_source_output(r#"{"events":[]}"#), None);
    assert_eq!(parse_source_output(r#"{"v":0}"#), None);
    assert_eq!(parse_source_output(r#"{"v":"1"}"#), None);
    assert_eq!(parse_source_output("[]"), None);
    assert_eq!(parse_source_output("nada"), None);
}

#[test]
fn parse_da_saida_respeita_os_tetos_e_os_cortes() {
    let events: Vec<Value> = (0..600)
        .map(|i| json!({"ts": "2026-03-01T10:00:00Z", "text": format!("e{i}")}))
        .collect();
    let metrics: Vec<Value> = (0..300)
        .map(|i| json!({"name": format!("m{i}"), "value": i}))
        .collect();
    let doc = json!({"v": 1, "events": events, "metrics": metrics});
    let got = parse_source_output(&doc.to_string()).expect("ok");
    assert_eq!((got.events.len(), got.metrics.len()), (500, 200));

    let long = "x".repeat(600);
    let doc = json!({"v": 1, "events": [{"ts": "2026-03-01T10:00:00Z", "text": long, "level": "l".repeat(40)}],
        "metrics": [{"name": "n".repeat(100), "value": 1.5, "unit": "u".repeat(30), "ts": "2026-03-01T07:00:00-03:00"}],
        "start": "2026-03-01T07:00:00-03:00"});
    let got = parse_source_output(&doc.to_string()).expect("ok");
    assert_eq!(got.events[0].text, format!("{}…", "x".repeat(499)));
    assert_eq!(
        got.events[0].level.as_deref(),
        Some(format!("{}…", "l".repeat(15)).as_str())
    );
    assert_eq!(got.metrics[0].name, format!("{}…", "n".repeat(79)));
    assert_eq!(
        got.metrics[0].unit.as_deref(),
        Some(format!("{}…", "u".repeat(15)).as_str())
    );
    assert_eq!(got.metrics[0].ts.as_deref(), Some("2026-03-01T10:00:00Z"));
    assert_eq!(got.start.as_deref(), Some("2026-03-01T10:00:00Z"));
}

#[test]
fn token_em_credenciais_tolera_formatos() {
    assert_eq!(
        token_from_credentials(Some("A=1\n`K_TOKEN`: \"valor\"\n"), "K_TOKEN").as_deref(),
        Some("valor")
    );
    assert_eq!(token_from_credentials(Some("K_TOKEN=\n"), "K_TOKEN"), None);
    assert_eq!(
        token_from_credentials(Some("* K_TOKEN = 'abc'  \r\n"), "K_TOKEN").as_deref(),
        Some("abc")
    );
    assert_eq!(
        token_from_credentials(Some("K_TOKEN: a b\n"), "K_TOKEN").as_deref(),
        Some("a b")
    );
    assert_eq!(
        token_from_credentials(Some("K_TOKEN_2=1\n"), "K_TOKEN"),
        None
    );
    assert_eq!(token_from_credentials(Some("k_token=1\n"), "K_TOKEN"), None);
    assert_eq!(token_from_credentials(None, "K_TOKEN"), None);
    assert_eq!(
        token_from_credentials(Some("X=1\nK_TOKEN=\"\n"), "K_TOKEN").as_deref(),
        Some("")
    );
}

#[test]
fn chave_do_token_segue_a_regex_do_contrato() {
    for ok in ["AB", "FONTE_TOKEN", &format!("A{}", "B".repeat(63))] {
        assert!(is_token_key(ok), "{ok}");
    }
    for bad in ["A", "aB", "A-B", "1AB", &format!("A{}", "B".repeat(64)), ""] {
        assert!(!is_token_key(bad), "{bad}");
    }
}
