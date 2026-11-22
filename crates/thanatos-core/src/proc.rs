//! Subprocesso com ambiente controlado. O `hades_core::proc::run` herda o ambiente do processo e não o aceita por
//! parâmetro; o Thanatos precisa de `GH_TOKEN` só no filho do `gh` (`src/commands/thanatos.ts:75-88` muda o `process.env`)
//! e de ambiente limpo na fonte externa (`src/lib/thanatos-source.ts:96-111`).

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(5);

/// Como o filho recebe o ambiente.
#[derive(Clone, Debug, Default)]
pub enum EnvMode {
    /// O do processo.
    #[default]
    Inherit,
    /// O do processo mais estas variáveis.
    Extend(Vec<(String, String)>),
    /// Só estas variáveis.
    Clear(Vec<(String, String)>),
}

/// Parâmetros de [`spawn`].
#[derive(Clone, Debug, Default)]
pub struct SpawnOptions {
    pub cwd: Option<PathBuf>,
    pub timeout: Option<Duration>,
    /// Teto de bytes por saída.
    pub max_buffer: Option<usize>,
    pub env: EnvMode,
}

/// Como o filho terminou.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Saiu sozinho; morte por sinal vale 1 (o `r.status ?? 1` do TS).
    Done {
        status: i32,
        stdout: String,
        stderr: String,
    },
    TimedOut,
    /// Uma das saídas passou de `max_buffer`.
    Overflow,
    /// Não iniciou (comando ausente, sem permissão).
    Failed(String),
}

fn drain(
    mut pipe: impl Read + Send + 'static,
    max: Option<usize>,
    exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(m) = max.filter(|m| buf.len() > *m) {
                        buf.truncate(m);
                        exceeded.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }
        }
        buf
    })
}

fn kill_group(child: &std::process::Child) {
    use rustix::process::{Pid, Signal, kill_process_group};
    if let Some(pgid) = Pid::from_raw(child.id() as i32) {
        let _ = kill_process_group(pgid, Signal::KILL);
    }
}

/// Roda o comando com stdin fechado, saídas capturadas, tempo limite e grupo próprio (o timeout mata o grupo todo).
pub fn spawn(command: &str, args: &[&str], options: &SpawnOptions) -> Outcome {
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if let Some(cwd) = &options.cwd {
        cmd.current_dir(cwd);
    }
    match &options.env {
        EnvMode::Inherit => {}
        EnvMode::Extend(vars) => {
            cmd.envs(vars.iter().map(|(k, v)| (k, v)));
        }
        EnvMode::Clear(vars) => {
            cmd.env_clear().envs(vars.iter().map(|(k, v)| (k, v)));
        }
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let exceeded = Arc::new(AtomicBool::new(false));
    let out = drain(
        child.stdout.take().expect("stdout piped"),
        options.max_buffer,
        exceeded.clone(),
    );
    let err = drain(
        child.stderr.take().expect("stderr piped"),
        options.max_buffer,
        exceeded.clone(),
    );

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.code(),
            Ok(None) => {}
            Err(_) => break None,
        }
        let over_time = options.timeout.is_some_and(|t| started.elapsed() >= t);
        if over_time || exceeded.load(Ordering::SeqCst) {
            timed_out = over_time;
            kill_group(&child);
            let _ = child.wait();
            break None;
        }
        thread::sleep(POLL);
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if exceeded.load(Ordering::SeqCst) {
        return Outcome::Overflow;
    }
    if timed_out {
        return Outcome::TimedOut;
    }
    Outcome::Done {
        status: status.unwrap_or(1),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str, options: &SpawnOptions) -> Outcome {
        spawn("sh", &["-c", script], options)
    }

    #[test]
    fn captura_saida_e_status() {
        let r = sh("echo oi; echo erro >&2; exit 3", &SpawnOptions::default());
        assert_eq!(
            r,
            Outcome::Done {
                status: 3,
                stdout: "oi\n".into(),
                stderr: "erro\n".into()
            }
        );
    }

    #[test]
    fn ambiente_limpo_so_leva_o_que_foi_dado() {
        let options = SpawnOptions {
            env: EnvMode::Clear(vec![("A".into(), "1".into())]),
            ..SpawnOptions::default()
        };
        let Outcome::Done { stdout, .. } = sh("echo \"$A|${HOME-x}|${GH_TOKEN-y}\"", &options)
        else {
            panic!("não terminou");
        };
        assert_eq!(stdout, "1|x|y\n");
    }

    #[test]
    fn ambiente_estendido_mantem_o_do_processo() {
        let options = SpawnOptions {
            env: EnvMode::Extend(vec![("GH_TOKEN".into(), "t".into())]),
            ..SpawnOptions::default()
        };
        let Outcome::Done { stdout, .. } =
            sh("echo \"$GH_TOKEN\"; command -v sh >/dev/null", &options)
        else {
            panic!("não terminou");
        };
        assert_eq!(stdout, "t\n");
    }

    #[test]
    fn tempo_limite_mata_o_grupo() {
        let options = SpawnOptions {
            timeout: Some(Duration::from_millis(200)),
            ..SpawnOptions::default()
        };
        assert_eq!(sh("sleep 5", &options), Outcome::TimedOut);
    }

    #[test]
    fn saida_acima_do_teto_estoura() {
        let options = SpawnOptions {
            max_buffer: Some(10),
            ..SpawnOptions::default()
        };
        assert_eq!(sh("yes | head -c 100000", &options), Outcome::Overflow);
    }

    #[test]
    fn comando_ausente_nao_inicia() {
        let r = spawn(
            "comando-que-nao-existe-thanatos",
            &[],
            &SpawnOptions::default(),
        );
        assert!(matches!(r, Outcome::Failed(_)));
    }
}
