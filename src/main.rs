mod protocol;
mod runner;

use anyhow::{Context, Result, bail};
use serde_json::json;
use std::os::unix::{
    fs::{OpenOptionsExt, PermissionsExt},
    process::CommandExt,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

static CANCEL: AtomicBool = AtomicBool::new(false);
extern "C" fn cancel(_: libc::c_int) {
    CANCEL.store(true, Ordering::Relaxed);
}

fn main() {
    unsafe {
        libc::signal(libc::SIGINT, cancel as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, cancel as *const () as libc::sighandler_t);
    }
    if let Err(e) = run() {
        eprintln!("xen-lab: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".into());
    let cwd = env::current_dir()?;
    let root = cwd
        .ancestors()
        .find(|p| p.join("flake.nix").is_file() && p.join("Cargo.toml").is_file())
        .context("run xen-lab from this repository or one of its subdirectories")?
        .to_owned();
    env::set_current_dir(&root)?;
    fs::create_dir_all(".lab/runs")?;
    if command == "logs" {
        let run = latest()?;
        println!("{}", run.display());
        println!(
            "{}",
            fs::read_to_string(run.join("summary.json"))
                .unwrap_or_else(|_| "Run still in progress".into())
        );
        return Ok(());
    }
    if command == "help" || command == "--help" {
        println!(
            "cargo run -- doctor|build|test|logs|reset\n\
                  test [--guest linux|hermit|ipc|service] [--peer PATH] [--peer-expect TEXT] [--ipc-case normal|malformed|peer-failure] [--cycles N] [--kernel PATH] [--initrd PATH|none] [--cmdline TEXT] [--expect TEXT] [--timeout SECONDS] [--repeat N] [--no-build]\n\
                  Every test boots real Xen under KVM. Logs: .lab/runs/<UTC timestamp>-<pid>/"
        );
        return Ok(());
    }
    if command == "reset" {
        fs::write(".lab/cancel", b"reset requested\n")?;
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(".lab/lock")?;
    let deadline = Instant::now() + Duration::from_secs(if command == "reset" { 30 } else { 1 });
    loop {
        if unsafe {
            libc::flock(
                std::os::fd::AsRawFd::as_raw_fd(&lock),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        } == 0
        {
            break;
        }
        if Instant::now() > deadline {
            bail!("another lab command is running; reset requests cancellation of an active test");
        }
        thread::sleep(Duration::from_millis(100));
    }
    if command == "reset" {
        runner::reset()?;
        let _ = fs::remove_file(".lab/cancel");
        println!("Lab stopped; disposable state removed; run logs retained.");
        return Ok(());
    }
    let _ = fs::remove_file(".lab/cancel");
    match command.as_str() {
        "doctor" => doctor(),
        "build" => build(),
        "test" => {
            let options = runner::Options::parse(args.collect())?;
            // Allocate the run before preflight/build so failures always have a summary.
            runner::run(options)
        }
        _ => bail!("unknown command {command}; use --help"),
    }
}

fn check_cancel() -> Result<()> {
    if CANCEL.load(Ordering::Relaxed) || Path::new(".lab/cancel").exists() {
        bail!("cancelled");
    }
    Ok(())
}

fn latest() -> Result<PathBuf> {
    Ok(PathBuf::from(
        fs::read_to_string(".lab/latest")
            .context("no runs yet")?
            .trim(),
    ))
}

fn doctor() -> Result<()> {
    let cpu = fs::read_to_string("/proc/cpuinfo")?;
    let virt = cpu.split_whitespace().any(|f| f == "svm" || f == "vmx");
    let nested = [
        "/sys/module/kvm_amd/parameters/nested",
        "/sys/module/kvm_intel/parameters/nested",
    ]
    .into_iter()
    .find_map(|p| fs::read_to_string(p).ok())
    .unwrap_or_default();
    let kvm = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm");
    let api = kvm
        .as_ref()
        .map(|f| unsafe {
            libc::ioctl(
                std::os::fd::AsRawFd::as_raw_fd(f),
                0xAE00 as libc::c_ulong,
                0,
            )
        })
        .unwrap_or(-1);
    let report = json!({"cpu_virtualization":virt,"kvm_api_version":api,"kvm_open_error":kvm.as_ref().err().map(ToString::to_string),"kvm_ioctl_error":if api < 0 { Some(std::io::Error::last_os_error().to_string()) } else {None},"nested":nested.trim(),
    "host":fs::read_to_string("/proc/version")?.trim(),"os":fs::read_to_string("/etc/os-release")?,
    "tools": (["nix", "cargo", "rustc", "qemu-system-x86_64", "xl"].map(|tool| {
        let path = env::split_paths(&env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join(tool))
            .find(|p| p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0));
        json!({"name":tool,"path":path})
    }))});
    fs::write(".lab/doctor.json", serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if !virt || api != 12 || !["1", "Y", "y"].contains(&nested.trim()) {
        bail!("hardware virtualization, KVM API 12 and nested virtualization are required");
    }
    Ok(())
}

// All child commands have deadlines; kill their whole process group on failure.
struct ManagedChild(std::process::Child);
impl ManagedChild {
    fn spawn(command: &mut Command) -> Result<Self> {
        command.process_group(0);
        Ok(Self(command.spawn()?))
    }
    fn stop(&mut self) -> Result<std::process::ExitStatus> {
        if let Some(status) = self.0.try_wait()? {
            return Ok(status);
        }
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.0.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                bail!("process {} did not exit after SIGKILL", self.0.id());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn build() -> Result<()> {
    build_to(Path::new(".lab/build.log"))
}
fn build_to(log: &Path) -> Result<()> {
    build_package_to("lab", log)
}

fn build_package_to(package: &str, log: &Path) -> Result<()> {
    fs::create_dir_all(".lab/build")?;
    let link = format!(".lab/build/{package}");
    println!("Building pinned artifacts (log: {})", log.display());
    let out = fs::File::create(log)?;
    let mut child = ManagedChild::spawn(
        Command::new("nix")
            .args([
                "build",
                &format!(".#{package}"),
                "--out-link",
                &link,
                "-L",
                "--no-update-lock-file",
            ])
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out),
    )?;
    let deadline = Instant::now() + Duration::from_secs(3600);
    loop {
        check_cancel()?;
        if let Some(status) = child.0.try_wait()? {
            if !status.success() {
                bail!("Nix build failed ({status}); see {}", log.display());
            }
            return Ok(());
        }
        if Instant::now() > deadline {
            bail!("Nix build timed out; see {}", log.display());
        }
        thread::sleep(Duration::from_millis(200));
    }
}
