//! Kortlivede Docker-containere for tester.
//!
//! Styrer `docker`-CLI-et direkte. Hver container får tilfeldige vertsporter
//! på 127.0.0.1 og fjernes når verdien droppes.

use std::io::Write;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

pub const POSTGRES_PORT: u16 = 5432;

#[derive(Debug, Clone)]
pub struct ContainerSpec {
    image: String,
    ports: Vec<u16>,
    env: Vec<(String, String)>,
    filer: Vec<(String, Vec<u8>)>,
    args: Vec<String>,
}

impl ContainerSpec {
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            ports: Vec::new(),
            env: Vec::new(),
            filer: Vec::new(),
            args: Vec::new(),
        }
    }

    pub fn port(mut self, container_port: u16) -> Self {
        self.ports.push(container_port);
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Kopieres inn før containeren starter.
    pub fn fil(mut self, sti: impl Into<String>, innhold: impl Into<Vec<u8>>) -> Self {
        self.filer.push((sti.into(), innhold.into()));
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn start(&self) -> Result<Container> {
        let mut create = Command::new("docker");
        create.arg("create");
        for port in &self.ports {
            create.arg("-p").arg(format!("127.0.0.1::{port}"));
        }
        for (key, value) in &self.env {
            create.arg("-e").arg(format!("{key}={value}"));
        }
        create.arg(&self.image).args(&self.args);

        let container = Container {
            id: docker(&mut create)?,
        };
        for (sti, innhold) in &self.filer {
            container.kopier_inn(sti, innhold)?;
        }
        docker(Command::new("docker").args(["start", &container.id]))?;
        Ok(container)
    }
}

#[derive(Debug)]
pub struct Container {
    id: String,
}

impl Container {
    pub fn host_port(&self, container_port: u16) -> Result<u16> {
        let utdata = docker(Command::new("docker").args([
            "port",
            &self.id,
            &format!("{container_port}/tcp"),
        ]))?;
        utdata
            .lines()
            .find_map(|linje| linje.rsplit_once(':')?.1.parse().ok())
            .with_context(|| format!("ingen vertsport for {container_port}/tcp: {utdata}"))
    }

    /// Venter til kommandoen lykkes inne i containeren.
    pub fn vent_paa(&self, kommando: &[&str], tidsfrist: Duration) -> Result<()> {
        let frist = Instant::now() + tidsfrist;
        loop {
            let status = Command::new("docker")
                .arg("exec")
                .arg(&self.id)
                .args(kommando)
                .output()
                .context("kunne ikke kjøre docker exec")?;
            if status.status.success() {
                return Ok(());
            }
            if Instant::now() >= frist {
                bail!(
                    "{} ble ikke klar innen {tidsfrist:?}: {}",
                    self.id,
                    String::from_utf8_lossy(&status.stderr)
                );
            }
            sleep(Duration::from_millis(200));
        }
    }

    fn kopier_inn(&self, sti: &str, innhold: &[u8]) -> Result<()> {
        let mut lokal =
            tempfile::NamedTempFile::new().context("kunne ikke lage midlertidig fil")?;
        lokal
            .write_all(innhold)
            .context("kunne ikke skrive midlertidig fil")?;
        docker(
            Command::new("docker")
                .arg("cp")
                .arg(lokal.path())
                .arg(format!("{}:{sti}", self.id)),
        )?;
        Ok(())
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "--force", "--volumes", &self.id])
            .output();
    }
}

/// Postgres med brukeren, passordet og databasen `postgres`, klar til å ta
/// imot TCP-forbindelser. Returnerer containeren og vertsporten.
pub fn postgres() -> Result<(Container, u16)> {
    let container = ContainerSpec::new("postgres:11-alpine")
        .port(POSTGRES_PORT)
        .env("POSTGRES_USER", "postgres")
        .env("POSTGRES_PASSWORD", "postgres")
        .env("POSTGRES_DB", "postgres")
        .start()?;
    // Under initdb lytter Postgres bare på unix-socket; TCP svarer først når
    // den endelige serveren kjører.
    container.vent_paa(
        &[
            "pg_isready",
            "--host",
            "127.0.0.1",
            "--username",
            "postgres",
        ],
        Duration::from_secs(60),
    )?;
    let port = container.host_port(POSTGRES_PORT)?;
    Ok((container, port))
}

fn docker(kommando: &mut Command) -> Result<String> {
    let utdata = kommando
        .output()
        .with_context(|| format!("kunne ikke kjøre {kommando:?}"))?;
    if !utdata.status.success() {
        bail!(
            "{kommando:?} feilet: {}",
            String::from_utf8_lossy(&utdata.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&utdata.stdout).trim().to_string())
}
