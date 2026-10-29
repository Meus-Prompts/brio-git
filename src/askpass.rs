//! Prompt de credenciais na UI via `GIT_ASKPASS` / `SSH_ASKPASS`.
//!
//! O próprio executável do brio-git é registrado como askpass. Quando o git o
//! invoca, `main` detecta `BRIO_GIT_ASKPASS_SOCKET`, envia o prompt à janela e
//! imprime a resposta que o usuário digitou.
//!
//! Transporte: socket Unix com permissão 0600 (Linux/macOS) ou TCP em
//! 127.0.0.1 com um token aleatório exigido na conexão (Windows).

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender, channel};

pub const SOCKET_ENV: &str = "BRIO_GIT_ASKPASS_SOCKET";

/// Endereço anunciado aos processos filhos (caminho do socket ou `tcp:porta:token`).
static ADDRESS: OnceLock<String> = OnceLock::new();
#[cfg(unix)]
static SOCKET_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Um pedido pendente: o texto do prompt e o canal para a resposta
/// (`None` = cancelado).
pub struct Request {
    pub prompt: String,
    pub reply: Sender<Option<String>>,
}

impl Request {
    pub fn is_secret(&self) -> bool {
        let p = self.prompt.to_lowercase();
        ["password", "passphrase", "senha", "pin", "token"].iter().any(|k| p.contains(k))
    }
}

/// Variáveis de ambiente para processos git/ssh filhos.
pub fn env() -> Vec<(&'static str, String)> {
    let (Some(addr), Ok(exe)) = (ADDRESS.get(), std::env::current_exe()) else { return Vec::new() };
    let exe = exe.display().to_string();
    vec![
        ("GIT_ASKPASS", exe.clone()),
        ("SSH_ASKPASS", exe),
        ("SSH_ASKPASS_REQUIRE", "force".into()),
        (SOCKET_ENV, addr.clone()),
    ]
}

/// Sobe o servidor de prompts; os pedidos chegam em `Receiver`.
pub fn serve(ctx: egui::Context) -> std::io::Result<Receiver<Request>> {
    let (tx, rx) = channel();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixListener;
        let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("brio-git-askpass-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        // Só o próprio usuário pode falar com o socket.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let _ = ADDRESS.set(path.display().to_string());
        let _ = SOCKET_PATH.set(path);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (tx, ctx) = (tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let _ = handle(stream, None, &tx, &ctx);
                });
            }
        });
    }
    #[cfg(not(unix))]
    {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let token = random_token()?;
        let _ = ADDRESS.set(format!("tcp:{port}:{token}"));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (tx, ctx, token) = (tx.clone(), ctx.clone(), token.clone());
                std::thread::spawn(move || {
                    let _ = handle(stream, Some(&token), &tx, &ctx);
                });
            }
        });
    }
    Ok(rx)
}

#[cfg(not(unix))]
fn random_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Protocolo: `[token\n]prompt\n` → `OK resposta\n` | `CANCEL\n`.
fn handle<S: Read + Write>(stream: S, token: Option<&str>, tx: &Sender<Request>, ctx: &egui::Context) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream);
    if let Some(expected) = token {
        let mut got = String::new();
        reader.read_line(&mut got)?;
        if got.trim_end() != expected {
            // Conexão sem o token: outro processo local tentando ler segredos.
            return Ok(());
        }
    }
    let mut prompt = String::new();
    reader.read_line(&mut prompt)?;
    let (reply_tx, reply_rx) = channel();
    if tx.send(Request { prompt: prompt.trim_end().to_string(), reply: reply_tx }).is_err() {
        return Ok(());
    }
    ctx.request_repaint();
    let stream = reader.get_mut();
    match reply_rx.recv().ok().flatten() {
        Some(answer) => writeln!(stream, "OK {answer}"),
        None => writeln!(stream, "CANCEL"),
    }
}

/// Lado helper (processo filho do git): devolve a resposta ou `None`.
pub fn ask(address: &str, prompt: &str) -> Option<String> {
    let prompt = prompt.replace('\n', " ");
    let mut line = String::new();
    if let Some(rest) = address.strip_prefix("tcp:") {
        let (port, token) = rest.split_once(':')?;
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port.parse::<u16>().ok()?)).ok()?;
        write!(stream, "{token}\n{prompt}\n").ok()?;
        BufReader::new(stream).read_line(&mut line).ok()?;
    } else {
        #[cfg(unix)]
        {
            let mut stream = std::os::unix::net::UnixStream::connect(address).ok()?;
            writeln!(stream, "{prompt}").ok()?;
            BufReader::new(stream).read_line(&mut line).ok()?;
        }
        #[cfg(not(unix))]
        return None;
    }
    line.trim_end_matches(['\n', '\r']).strip_prefix("OK ").map(str::to_string)
}

/// Remove o socket ao fechar o app.
pub fn cleanup() {
    #[cfg(unix)]
    if let Some(p) = SOCKET_PATH.get() {
        let _ = std::fs::remove_file(p);
    }
}

/// Entrada do modo helper; retorna o código de saída do processo.
pub fn helper_main(address: &str) -> i32 {
    let prompt = std::env::args().nth(1).unwrap_or_else(|| "Password:".into());
    match ask(address, &prompt) {
        Some(answer) => {
            println!("{answer}");
            0
        }
        None => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn respond(rx: Receiver<Request>, expected_prompt: &'static str) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let req = rx.recv().unwrap();
            assert_eq!(req.prompt, expected_prompt);
            assert!(req.is_secret());
            req.reply.send(Some("s3nh@ com espaço".into())).unwrap();
            let req = rx.recv().unwrap();
            req.reply.send(None).unwrap();
        })
    }

    #[cfg(unix)]
    #[test]
    fn round_trip_through_unix_socket() {
        let dir = std::env::temp_dir().join(format!("brio-git-askpass-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (tx, rx) = channel();
        let ctx = egui::Context::default();
        std::thread::spawn(move || {
            for s in listener.incoming().flatten() {
                let (tx, ctx) = (tx.clone(), ctx.clone());
                std::thread::spawn(move || handle(s, None, &tx, &ctx));
            }
        });
        let responder = respond(rx, "Password for 'https://ana@example.com':");
        let addr = path.display().to_string();
        assert_eq!(ask(&addr, "Password for 'https://ana@example.com':").as_deref(), Some("s3nh@ com espaço"));
        assert_eq!(ask(&addr, "Username:"), None, "cancelar devolve None");
        responder.join().unwrap();
    }

    /// O transporte do Windows (TCP + token) também roda no Linux: testado aqui.
    #[test]
    fn round_trip_through_tcp_with_token() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = channel();
        let ctx = egui::Context::default();
        std::thread::spawn(move || {
            for s in listener.incoming().flatten() {
                let (tx, ctx) = (tx.clone(), ctx.clone());
                std::thread::spawn(move || handle(s, Some("tok123"), &tx, &ctx));
            }
        });
        // Sem o token certo a conexão é ignorada e nenhum prompt chega à UI.
        assert_eq!(ask(&format!("tcp:{port}:errado"), "Password:"), None);
        let responder = respond(rx, "Password:");
        let addr = format!("tcp:{port}:tok123");
        assert_eq!(ask(&addr, "Password:").as_deref(), Some("s3nh@ com espaço"));
        assert_eq!(ask(&addr, "Username:"), None);
        responder.join().unwrap();
    }
}
