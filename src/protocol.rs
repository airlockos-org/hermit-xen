use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};

pub struct Channel {
    reader: BufReader<UnixStream>,
    log: File,
    qmp: bool,
    next_id: u64,
    pending: String,
}
impl Channel {
    pub fn connect(socket: &Path, log: &Path, qmp: bool) -> Result<Self> {
        let stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        Ok(Self {
            reader: BufReader::new(stream),
            log: OpenOptions::new().create(true).append(true).open(log)?,
            qmp,
            next_id: 0,
            pending: String::new(),
        })
    }
    fn receive(&mut self, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        // Preserve fragments across read timeouts; do not discard partial JSON frames.
        loop {
            crate::check_cancel()?;
            if Instant::now() >= deadline {
                bail!("channel response timed out after {timeout:?}");
            }
            self.reader.get_ref().set_read_timeout(Some(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(1)),
            ))?;
            match self.reader.read_line(&mut self.pending) {
                Ok(0) => bail!("channel closed"),
                Ok(_) if self.pending.ends_with('\n') => {
                    let line = std::mem::take(&mut self.pending);
                    let message: Value =
                        serde_json::from_str(&line).context("invalid JSON from VM")?;
                    writeln!(
                        self.log,
                        "{}",
                        json!({"at":chrono::Utc::now().to_rfc3339(),"direction":"rx","message":message})
                    )?;
                    return Ok(message);
                }
                Ok(_) => (),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    pub fn greeting(&mut self) -> Result<Value> {
        self.receive(Duration::from_secs(5))
    }
    pub fn request(&mut self, mut request: Value, timeout: Duration) -> Result<Value> {
        self.next_id += 1;
        request["id"] = json!(self.next_id);
        writeln!(
            self.log,
            "{}",
            json!({"at":chrono::Utc::now().to_rfc3339(),"direction":"tx","message":request})
        )?;
        writeln!(self.reader.get_mut(), "{request}")?;
        let deadline = Instant::now() + timeout;
        loop {
            let response = self.receive(deadline.saturating_duration_since(Instant::now()))?;
            if self.qmp && response.get("event").is_some() {
                continue;
            }
            if response["id"] != self.next_id {
                continue; // Discard a late reply from an earlier timed-out operation.
            }
            if response.get("error").is_some() {
                bail!("QMP error: {response}");
            }
            return Ok(response);
        }
    }
    pub fn qmp(&mut self, command: &str) -> Result<Value> {
        self.request(json!({"execute":command}), Duration::from_secs(5))
    }
    pub fn exec(&mut self, argv: &[&str], timeout: u64) -> Result<Value> {
        self.request(
            json!({"argv":argv,"timeout":timeout}),
            Duration::from_secs(timeout + 5),
        )
    }
    pub fn checked(&mut self, argv: &[&str], timeout: u64) -> Result<Value> {
        let response = self.exec(argv, timeout)?;
        if response["code"] != 0 {
            bail!("guest command {argv:?} failed: {response}");
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };

    fn pair() -> (Channel, UnixStream) {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let (client, server) = UnixStream::pair().unwrap();
        let path = std::env::temp_dir().join(format!(
            "hx-protocol-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let log = File::create(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        (
            Channel {
                reader: BufReader::new(client),
                log,
                qmp: true,
                next_id: 0,
                pending: String::new(),
            },
            server,
        )
    }

    #[test]
    fn ignores_events_and_stale_replies_and_preserves_partial_frames() {
        let (mut channel, mut server) = pair();
        let worker = thread::spawn(move || {
            let mut request = String::new();
            BufReader::new(server.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            assert_eq!(serde_json::from_str::<Value>(&request).unwrap()["id"], 1);
            server
                .write_all(b"{\"event\":\"RESET\"}\n{\"id\":0,\"return\":{}}\n{\"id\":1,")
                .unwrap();
            thread::sleep(Duration::from_millis(1100)); // Cross a socket read timeout mid-frame.
            server
                .write_all(b"\"return\":{\"enabled\":true}}\n")
                .unwrap();
        });
        let response = channel
            .request(json!({"execute":"query-kvm"}), Duration::from_secs(3))
            .unwrap();
        assert_eq!(response["return"]["enabled"], true);
        worker.join().unwrap();
    }

    #[test]
    fn stalled_channel_obeys_deadline() {
        let (mut channel, _server) = pair();
        let started = Instant::now();
        let error = channel
            .request(json!({"execute":"query-status"}), Duration::from_millis(40))
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_millis(500));
    }
}
