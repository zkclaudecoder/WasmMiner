use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

pub fn send_line(writer: &Arc<Mutex<TcpStream>>, msg: &str) -> anyhow::Result<()> {
    let mut w = writer
        .lock()
        .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
    writeln!(w, "{}", msg)?;
    w.flush()?;
    Ok(())
}

pub fn read_json(reader: &mut impl BufRead) -> anyhow::Result<serde_json::Value> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.is_empty() {
        anyhow::bail!("Connection closed by pool");
    }
    Ok(serde_json::from_str(line.trim())?)
}
