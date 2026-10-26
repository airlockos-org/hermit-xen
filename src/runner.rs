use crate::{ManagedChild, protocol::Channel};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct Options {
    guest: String,
    repeat: u8,
    cycles: u8,
    ipc_case: String,
    kernel: Option<PathBuf>,
    initrd: Option<String>,
    cmdline: String,
    expect: String,
    timeout: u64,
    no_build: bool,
}
impl Options {
    pub fn parse(args: Vec<String>) -> Result<Self> {
        let guest = args
            .windows(2)
            .find(|a| a[0] == "--guest")
            .map(|a| a[1].as_str())
            .unwrap_or("linux");
        ensure!(
            matches!(guest, "linux" | "hermit" | "ipc"),
            "--guest must be linux, hermit or ipc"
        );
        let hermit = guest != "linux";
        let mut result = Self {
            guest: guest.into(),
            repeat: 1,
            cycles: 1,
            ipc_case: "normal".into(),
            kernel: None,
            initrd: hermit.then(|| "none".into()),
            cmdline: if hermit {
                ""
            } else {
                "console=hvc0 earlyprintk=xen panic=-1 rdinit=/init"
            }
            .into(),
            expect: if guest == "ipc" {
                "HERMIT_IPC_PASS"
            } else if hermit {
                "HERMIT_SMOKE_PASS"
            } else {
                "PVH_SMOKE_PASS"
            }
            .into(),
            timeout: 180,
            no_build: false,
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if arg == "--no-build" {
                result.no_build = true;
                continue;
            }
            let value = args
                .next()
                .with_context(|| format!("missing value for {arg}"))?;
            match arg.as_str() {
                "--guest" => ensure!(value == result.guest, "conflicting --guest options"),
                "--repeat" => result.repeat = value.parse()?,
                "--cycles" => result.cycles = value.parse()?,
                "--ipc-case" => result.ipc_case = value,
                "--kernel" => result.kernel = Some(PathBuf::from(value)),
                "--initrd" => result.initrd = Some(value),
                "--cmdline" => result.cmdline = value,
                "--expect" => result.expect = value,
                "--timeout" => result.timeout = value.parse()?,
                _ => bail!("unknown test option {arg}"),
            }
        }
        ensure!(
            (1..=100).contains(&result.repeat),
            "--repeat must be 1..100"
        );
        ensure!(
            (1..=100).contains(&result.cycles),
            "--cycles must be 1..100"
        );
        ensure!(
            matches!(
                result.ipc_case.as_str(),
                "normal" | "malformed" | "peer-failure"
            ),
            "invalid IPC case"
        );
        if result.guest == "ipc" && result.cmdline.is_empty() {
            result.cmdline = format!(" -- {}", result.ipc_case);
        }
        ensure!(!result.expect.is_empty(), "--expect must be nonempty");
        ensure!(
            (5..=1800).contains(&result.timeout),
            "timeout must be 5..1800 seconds"
        );
        ensure!(
            !result.cmdline.contains(['\n', '\r']),
            "cmdline cannot contain newlines"
        );
        Ok(result)
    }
}

pub fn run(options: Options) -> Result<()> {
    for attempt in 1..=options.repeat {
        println!("Test run {attempt}/{}", options.repeat);
        run_once(&options)?;
    }
    Ok(())
}

fn run_once(options: &Options) -> Result<()> {
    reset()?; // reap only a verified orphan from a previous interrupted controller
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
    let run = std::env::current_dir()?.join(format!(".lab/runs/{stamp}-{}", std::process::id()));
    fs::create_dir_all(&run)?;
    fs::write(".lab/latest", run.to_string_lossy().as_bytes())?;
    let start = Instant::now();
    let mut summary = json!({"schema_version":1,"started_at":chrono::Utc::now().to_rfc3339(),"status":"running", "stage":"preflight", "run_dir":run, "assertions":[]});
    save(&run, &summary)?;
    println!("Run artifacts: {}", run.display());
    let outcome = execute(options, &run, &mut summary);
    if options.guest != "linux" {
        hermit_diagnostics(&run, &mut summary, outcome.is_ok());
    }
    let serial = fs::read(run.join("serial.log")).unwrap_or_default();
    let serial = String::from_utf8_lossy(&serial);
    summary["boot_progress"] = json!({
        "xen_banner":serial.contains("(XEN) Xen version"),
        "hardware_virtualization":serial.contains("SVM enabled") || serial.contains("VMX enabled"),
        "dom0_linux":serial.contains("Linux version"),
        "dom0_rpc_ready":serial.contains("LAB_RPC_READY")
    });
    summary["status"] = json!(if outcome.is_ok() { "passed" } else { "failed" });
    summary["elapsed_seconds"] = json!(start.elapsed().as_secs_f64());
    summary["finished_at"] = json!(chrono::Utc::now().to_rfc3339());
    if let Err(ref e) = outcome {
        summary["error"] = json!(format!("{e:#}"));
    }
    save(&run, &summary)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    outcome
}

fn save(run: &Path, summary: &Value) -> Result<()> {
    fs::write(
        run.join("summary.json.tmp"),
        serde_json::to_vec_pretty(summary)?,
    )?;
    fs::rename(run.join("summary.json.tmp"), run.join("summary.json"))?;
    Ok(())
}
fn stage(run: &Path, summary: &mut Value, name: &str) -> Result<()> {
    summary["stage"] = json!(name);
    println!("[{name}]");
    save(run, summary)
}
fn assertion(summary: &mut Value, name: &str, passed: bool) -> Result<()> {
    summary["assertions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":name,"passed":passed}));
    ensure!(passed, "assertion failed: {name}");
    Ok(())
}
fn record(run: &Path, name: &str, value: &Value) -> Result<()> {
    fs::write(
        run.join(format!("{name}.json")),
        serde_json::to_vec_pretty(value)?,
    )?;
    if let Some(out) = value["stdout"].as_str() {
        fs::write(run.join(format!("{name}.txt")), out)?;
    }
    Ok(())
}

fn execute(options: &Options, run: &Path, summary: &mut Value) -> Result<()> {
    crate::doctor()?;
    fs::copy(".lab/doctor.json", run.join("doctor.json"))?;
    if !options.no_build {
        stage(run, summary, "build")?;
        crate::build_to(&run.join("build.log"))?;
        if options.guest != "linux" && options.kernel.is_none() {
            crate::build_package_to(
                if options.guest == "ipc" {
                    "hermit-ipc"
                } else {
                    "hermit-smoke"
                },
                &run.join("hermit-build.log"),
            )?;
        }
    }
    if options.guest == "ipc" && !options.no_build {
        crate::build_package_to("ipc-peer", &run.join("peer-build.log"))?;
    }
    let lab = fs::canonicalize(".lab/build/lab").context("build the lab first")?;
    fs::copy("flake.lock", run.join("flake.lock"))?;
    fs::write(run.join("lab-path.txt"), lab.to_string_lossy().as_bytes())?;
    let inputs = run.join("inputs");
    fs::create_dir(&inputs)?;
    let _inputs_cleanup = Disposable(inputs.clone());
    let kernel = options.kernel.clone().unwrap_or_else(|| {
        if options.guest == "ipc" {
            PathBuf::from(".lab/build/hermit-ipc/kernel.elf")
        } else if options.guest == "hermit" {
            PathBuf::from(".lab/build/hermit-smoke/kernel.elf")
        } else {
            lab.join("guest-kernel")
        }
    });
    fs::copy(&kernel, inputs.join("kernel"))
        .context("guest kernel: build it first or supply --kernel")?;
    if options.guest == "ipc" {
        fs::copy(
            ".lab/build/ipc-peer/bin/xen-ipc-peer",
            inputs.join("ipc-peer"),
        )?;
        fs::copy(".lab/build/ipc-peer/bin/xen-ipc-peer", run.join("ipc-peer"))?;
        diagnostic_command(run, "peer-sha256", "sha256sum", &["ipc-peer"]);
    }
    if options.guest != "linux" {
        fs::copy(&kernel, run.join("kernel.elf"))?;
        fs::write(
            run.join("kernel-source.txt"),
            fs::canonicalize(&kernel)?.to_string_lossy().as_bytes(),
        )?;
        if options.kernel.is_none() {
            fs::copy(
                if options.guest == "ipc" {
                    ".lab/build/hermit-ipc/provenance.json"
                } else {
                    ".lab/build/hermit-smoke/provenance.json"
                },
                run.join("hermit-provenance.json"),
            )?;
        }
        diagnostic_command(run, "elf", "readelf", &["-h", "-l", "-n", "kernel.elf"]);
        diagnostic_command(run, "kernel-sha256", "sha256sum", &["kernel.elf"]);
    }
    let has_initrd = options.initrd.as_deref() != Some("none");
    if has_initrd {
        fs::copy(
            options
                .initrd
                .as_deref()
                .map(Path::new)
                .unwrap_or(&lab.join("guest-initrd")),
            inputs.join("initrd"),
        )?;
    }
    let cfg = format!(
        "name = 'pvh-smoke'\ntype = 'pvh'\nmemory = 256\nvcpus = 1\nkernel = '/inputs/kernel'\n{}extra = {}\non_poweroff = 'preserve'\non_reboot = 'preserve'\non_crash = 'preserve'\n",
        if has_initrd {
            "ramdisk = '/inputs/initrd'\n"
        } else {
            ""
        },
        serde_json::to_string(&options.cmdline)?
    );
    fs::write(inputs.join("guest.cfg"), cfg)?;
    fs::copy(inputs.join("guest.cfg"), run.join("guest.cfg"))?;
    fs::copy(lab.join("versions.json"), run.join("versions.json"))?;
    summary["guest_elf_bytes"] = json!(fs::metadata(&kernel)?.len());
    summary["guest"] = json!({"kind":options.guest,"kernel":kernel,"initrd":options.initrd,"cmdline":options.cmdline,"expected_marker":options.expect,"timeout_seconds":options.timeout});
    let sockets = std::env::temp_dir().join(format!(
        "hx-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));
    fs::create_dir(&sockets)?;
    let _socket_cleanup = Disposable(sockets.clone());
    let qmp_sock = sockets.join("qmp");
    let rpc_sock = sockets.join("rpc");
    let output = run.join("dom0");
    fs::create_dir(&output)?;
    let args = vec![
        "-name".into(),
        "hermit-xen-lab".into(),
        "-machine".into(),
        "q35,accel=kvm".into(),
        "-cpu".into(),
        "host".into(),
        "-smp".into(),
        "4".into(),
        "-m".into(),
        "6144".into(),
        "-display".into(),
        "none".into(),
        "-no-reboot".into(),
        "-nic".into(),
        "none".into(),
        "-cdrom".into(),
        lab.join("boot.iso").display().to_string(),
        "-boot".into(),
        "d".into(),
        "-serial".into(),
        format!("file:{}", run.join("serial.log").display()),
        "-qmp".into(),
        format!("unix:{},server=on,wait=off", qmp_sock.display()),
        "-chardev".into(),
        format!(
            "socket,id=control,path={},server=on,wait=off",
            rpc_sock.display()
        ),
        "-device".into(),
        "virtio-serial-pci".into(),
        "-device".into(),
        "virtserialport,chardev=control,name=lab.control".into(),
        "-virtfs".into(),
        format!(
            "local,path={},mount_tag=inputs,security_model=none,readonly=on",
            inputs.display()
        ),
        "-virtfs".into(),
        format!(
            "local,path={},mount_tag=artifacts,security_model=none",
            output.display()
        ),
    ];
    record(
        run,
        "qemu-command",
        &json!({"program":lab.join("qemu"),"args":args}),
    )?;
    stage(run, summary, "qemu-start")?;
    let mut child = ManagedChild::spawn(
        Command::new(lab.join("qemu"))
            .args(&args)
            .stdin(Stdio::null())
            .stdout(fs::File::create(run.join("qemu.stdout.log"))?)
            .stderr(fs::File::create(run.join("qemu.stderr.log"))?),
    )?;
    let active = json!({"pid":child.0.id(),"start_time":process_start(child.0.id())?,"sockets":sockets,"run":run});
    fs::write(".lab/active.json", serde_json::to_vec_pretty(&active)?)?;
    let result = test_vm(options, run, summary, &mut child, &qmp_sock, &rpc_sock);
    // Always capture the real QEMU status, including early exits and forced termination.
    let already_exited = child.0.try_wait()?.is_some();
    let status = child.stop()?;
    summary["qemu_exit"] = json!({"code":status.code(),"signal":status.signal()});
    let _ = fs::remove_file(".lab/active.json");
    if already_exited && !status.success() && result.is_err() {
        return result.context(format!("QEMU exited unexpectedly: {status}"));
    }
    if result.is_ok() {
        ensure!(
            status.success(),
            "QEMU failed to shut down normally: {status}"
        );
        stage(run, summary, "complete")?;
    }
    result
}

struct Disposable(PathBuf);
impl Drop for Disposable {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn test_vm(
    options: &Options,
    run: &Path,
    summary: &mut Value,
    child: &mut ManagedChild,
    qmp_sock: &Path,
    rpc_sock: &Path,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(180);
    while !qmp_sock.exists() {
        check_alive(child)?;
        ensure!(Instant::now() < deadline, "QEMU startup timed out");
        thread::sleep(Duration::from_millis(100));
    }
    let mut qmp = Channel::connect(qmp_sock, &run.join("qmp.jsonl"), true)?;
    record(run, "qmp-greeting", &qmp.greeting()?)?;
    qmp.qmp("qmp_capabilities")?;
    let kvm = qmp.qmp("query-kvm")?;
    record(run, "kvm", &kvm)?;
    assertion(
        summary,
        "QEMU KVM enabled",
        kvm["return"]["enabled"] == true,
    )?;
    stage(run, summary, "dom0-boot")?;
    loop {
        check_alive(child)?;
        let serial = fs::read(run.join("serial.log")).unwrap_or_default();
        let serial = String::from_utf8_lossy(&serial);
        ensure!(
            !serial.contains("Kernel panic - not syncing") && !serial.contains("Panic on CPU"),
            "boot crash detected; inspect serial.log"
        );
        if serial.contains("LAB_RPC_READY") {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "dom0 boot timed out; inspect serial.log"
        );
        thread::sleep(Duration::from_millis(200));
    }
    let mut rpc = Channel::connect(rpc_sock, &run.join("rpc.jsonl"), false)?;
    // Once RPC works, gather diagnostics after every test outcome.
    let outcome = (|| {
        summary["cycles"] = json!([]);
        for cycle in 1..=options.cycles {
            if cycle > 1 {
                rpc.checked(
                    &[
                        "truncate",
                        "-s",
                        "0",
                        "/lab/xen-console/guest-pvh-smoke.log",
                    ],
                    5,
                )?;
                let _ = rpc.exec(&["systemctl", "reset-failed", "lab-ipc-peer.service"], 5);
                rpc.checked(&["rm", "-f", "/lab/peer.log"], 5)?;
            }
            let cycle_run = run.join(format!("cycle-{cycle}"));
            fs::create_dir(&cycle_run)?;
            let started = Instant::now();
            let result = smoke(options, &cycle_run, summary, &mut rpc, child);
            summary["cycles"].as_array_mut().unwrap().push(json!({"cycle":cycle,"passed":result.is_ok(),"seconds":started.elapsed().as_secs_f64()}));
            // Keep the latest console at the established diagnostic path too.
            let _ = fs::copy(
                cycle_run.join("guest-console.txt"),
                run.join("guest-console.txt"),
            );
            if result.is_err() {
                let _ = rpc.exec(&["xl", "pause", "pvh-smoke"], 5);
                let _ = snapshot(&cycle_run, &mut rpc);
                for name in ["guest-registers.txt", "guest-registers.json"] {
                    let _ = fs::copy(cycle_run.join(name), run.join(name));
                }
            }
            let mut cycle_summary = summary.clone();
            cycle_summary["run_dir"] = json!(cycle_run);
            cycle_summary["status"] = json!(if result.is_ok() { "passed" } else { "failed" });
            cycle_summary["elapsed_seconds"] = json!(started.elapsed().as_secs_f64());
            if let Err(ref error) = result {
                cycle_summary["error"] = json!(format!("{error:#}"));
            }
            save(&cycle_run, &cycle_summary)?;
            result?;
        }
        Ok(())
    })();
    if options.guest == "ipc" {
        let _ = rpc.exec(&["systemctl", "stop", "lab-ipc-peer.service"], 5);
    }
    if outcome.is_err() {
        let _ = rpc.exec(&["xl", "pause", "pvh-smoke"], 5);
    }
    let _ = snapshot(run, &mut rpc);
    let _ = qmp
        .qmp("query-status")
        .and_then(|r| record(run, "qmp-final-status", &r));
    // Ask dom0 to power off Xen; then bounded QMP quit if shutdown is unsuccessful.
    let _ = rpc.exec(&["xl", "destroy", "pvh-smoke"], 5);
    let _ = rpc.exec(&["systemctl", "poweroff", "--no-block"], 5);
    let shutdown_deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < shutdown_deadline && child.0.try_wait()?.is_none() {
        if crate::check_cancel().is_err() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if child.0.try_wait()?.is_none() {
        let _ = qmp.qmp("quit");
        let quit_deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < quit_deadline && child.0.try_wait()?.is_none() {
            thread::sleep(Duration::from_millis(50));
        }
    }
    outcome
}
fn check_alive(child: &mut ManagedChild) -> Result<()> {
    crate::check_cancel()?;
    if let Some(status) = child.0.try_wait()? {
        bail!("QEMU exited unexpectedly: {status}");
    }
    Ok(())
}

fn smoke(
    options: &Options,
    run: &Path,
    summary: &mut Value,
    rpc: &mut Channel,
    child: &mut ManagedChild,
) -> Result<()> {
    stage(run, summary, "xen-baseline")?;
    let info = rpc.checked(&["xl", "info"], 10)?;
    record(run, "xl-info", &info)?;
    let text = info["stdout"].as_str().unwrap_or_default();
    assertion(
        summary,
        "real Xen with PVH capability",
        text.contains("xen_major") && text.contains("hvm"),
    )?;
    let domains = rpc.checked(&["xl", "list"], 10)?;
    record(run, "xl-list-before", &domains)?;
    assertion(
        summary,
        "dom0 operational",
        domains["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("Domain-0"),
    )?;
    stage(run, summary, "pvh-create")?;
    // Start paused so the declared PVH mode can be checked before execution.
    let create = rpc.exec(&["xl", "-vvv", "create", "-p", "/inputs/guest.cfg"], 30)?;
    record(run, "xl-create", &create)?;
    ensure!(create["code"] == 0, "PVH create failed: {create}");
    let config = rpc.checked(&["xl", "list", "-l", "pvh-smoke"], 5)?;
    record(run, "xl-list-paused", &config)?;
    let config: Value =
        serde_json::from_str(config["stdout"].as_str().context("missing domain config")?)?;
    assertion(
        summary,
        "domU type is PVH",
        config[0]["config"]["c_info"]["type"] == "pvh",
    )?;
    rpc.checked(&["xl", "unpause", "pvh-smoke"], 5)?;
    if options.guest == "ipc" {
        ipc_start(options, run, summary, rpc, child)?;
    }
    stage(run, summary, "pvh-smoke")?;
    let deadline = Instant::now() + Duration::from_secs(options.timeout);
    let mut events = fs::File::create(run.join("lifecycle.jsonl"))?;
    loop {
        check_alive(child)?;
        let list = rpc.checked(&["xl", "list", "-v", "pvh-smoke"], 5)?;
        let text = list["stdout"].as_str().unwrap_or_default();
        let fields: Vec<_> = text
            .lines()
            .find(|l| l.starts_with("pvh-smoke "))
            .context("PVH domain disappeared unexpectedly")?
            .split_whitespace()
            .collect();
        ensure!(fields.len() >= 8, "invalid xl list -v row: {text}");
        let state = fields[4];
        writeln!(
            events,
            "{}",
            json!({"at":chrono::Utc::now().to_rfc3339(),"state":state,"reason":fields[7],"xl_list":text})
        )?;
        if state.as_bytes().get(3) == Some(&b's') {
            record(run, "xl-list-shutdown", &list)?;
            summary["guest_shutdown_reason"] = json!(fields[7]);
            assertion(
                summary,
                "guest shutdown reason is poweroff",
                fields[7] == "0",
            )?;
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "PVH guest timed out; inspect dom0/xen-console/guest-pvh-smoke.log and xl-dmesg.txt"
        );
        thread::sleep(Duration::from_millis(200));
    }
    // Shutdown can precede xenconsoled draining the last ring bytes.
    // Read via dom0 as well, avoiding host/9p metadata visibility races.
    let drain_deadline = Instant::now() + Duration::from_secs(5);
    let console = loop {
        let response = rpc.exec(&["cat", "/lab/xen-console/guest-pvh-smoke.log"], 5)?;
        let text = response["stdout"].as_str().unwrap_or_default().to_owned();
        let drained = text.lines().any(|l| l.trim() == options.expect)
            && (options.guest != "hermit"
                || HERMIT_MARKERS
                    .iter()
                    .all(|marker| text.lines().any(|line| line.trim() == *marker)));
        if drained || Instant::now() >= drain_deadline {
            record(run, "guest-console", &response)?;
            break text;
        }
        check_alive(child)?;
        thread::sleep(Duration::from_millis(100));
    };
    rpc.checked(&["sync"], 5)?;
    assertion(
        summary,
        "guest console expected marker",
        console.lines().any(|l| l.trim() == options.expect),
    )?;
    assertion(
        summary,
        "no guest failure marker",
        !console.contains("PVH_SMOKE_FAIL"),
    )?;
    if options.guest == "hermit" {
        let mut cursor = 0;
        for marker in HERMIT_MARKERS {
            let found = console[cursor..].lines().any(|line| line.trim() == *marker);
            assertion(summary, marker, found)?;
            cursor += console[cursor..].find(marker).unwrap() + marker.len();
        }
        assertion(summary, "no Hermit panic", !console.contains("panicked at"))?;
    }
    if options.guest == "ipc" {
        ipc_finish(options, run, summary, rpc, &console)?;
    }
    rpc.checked(&["xl", "destroy", "pvh-smoke"], 5)?;
    let list = rpc.checked(&["xl", "list"], 5)?;
    record(run, "xl-list-after", &list)?;
    assertion(
        summary,
        "domU removed cleanly",
        !list["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("pvh-smoke"),
    )?;
    stage(run, summary, "guest-validated")
}

fn snapshot(run: &Path, rpc: &mut Channel) -> Result<()> {
    if let Ok(id) = rpc.checked(&["xl", "domid", "pvh-smoke"], 5)
        && let Some(id) = id["stdout"].as_str()
        && let Ok(value) = rpc.exec(&["xenctx", id.trim(), "0"], 5)
    {
        record(run, "guest-registers", &value)?;
    }
    let commands: &[(&str, &[&str])] = &[
        (
            "guest-console",
            &["cat", "/lab/xen-console/guest-pvh-smoke.log"],
        ),
        ("peer-console", &["cat", "/lab/peer.log"]),
        (
            "peer-status",
            &[
                "systemctl",
                "show",
                "lab-ipc-peer.service",
                "-p",
                "ExecMainStatus",
                "-p",
                "ExecMainCode",
                "-p",
                "SubState",
            ],
        ),
        ("xl-dmesg", &["xl", "dmesg"]),
        ("xl-list-final", &["xl", "list", "-l"]),
        ("xl-vcpu-list", &["xl", "vcpu-list"]),
        ("xenstore", &["xenstore-ls", "-f"]),
        ("dom0-dmesg", &["dmesg"]),
        (
            "dom0-journal",
            &["journalctl", "-b", "--no-pager", "-o", "short-monotonic"],
        ),
        ("systemd-failed", &["systemctl", "--failed", "--no-pager"]),
    ];
    for (name, argv) in commands {
        match rpc.exec(argv, 10) {
            Ok(value) => record(run, name, &value)?,
            Err(e) => {
                fs::write(run.join(format!("{name}.error")), format!("{e:#}"))?;
                break;
            }
        }
    }
    let _ = rpc.exec(&["cp", "-r", "/var/log/xen", "/lab/xen-logs"], 10);
    let _ = rpc.exec(&["sync"], 5);
    Ok(())
}

fn process_start(pid: u32) -> Result<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    Ok(stat
        .rsplit_once(')')
        .context("invalid process stat")?
        .1
        .split_whitespace()
        .nth(19)
        .context("missing start time")?
        .into())
}

pub fn reset() -> Result<()> {
    let path = Path::new(".lab/active.json");
    if !path.exists() {
        return Ok(());
    }
    let active: Value = serde_json::from_slice(&fs::read(path)?)?;
    let pid = active["pid"].as_u64().context("invalid active PID")? as u32;
    if process_start(pid).is_ok_and(|s| active["start_time"] == s) {
        let cmdline = fs::read(format!("/proc/{pid}/cmdline"))?;
        ensure!(
            String::from_utf8_lossy(&cmdline).contains("hermit-xen-lab"),
            "refusing to stop unrecognized PID {pid}"
        );
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
    if let Some(sockets) = active["sockets"].as_str() {
        let sockets = Path::new(sockets);
        if sockets.parent() == Some(std::env::temp_dir().as_path())
            && sockets
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("hx-"))
        {
            let _ = fs::remove_dir_all(sockets);
        }
    }
    if let Some(run) = active["run"].as_str() {
        let run = Path::new(run);
        let runs = fs::canonicalize(".lab/runs")?;
        if run.parent() == Some(runs.as_path()) {
            let _ = fs::remove_dir_all(run.join("inputs"));
            if let Ok(data) = fs::read(run.join("summary.json")) {
                let mut summary: Value = serde_json::from_slice(&data)?;
                if summary["status"] == "running" {
                    summary["status"] = json!("interrupted");
                    summary["error"] =
                        json!("controller terminated without completing; reset reaped orphan");
                    summary["finished_at"] = json!(chrono::Utc::now().to_rfc3339());
                    summary["qemu_exit"] = json!({"code":null,"signal":null,"observed":false});
                    save(run, &summary)?;
                }
            }
        }
    }
    fs::remove_file(path)?;
    Ok(())
}

const HERMIT_MARKERS: &[&str] = &[
    "HERMIT_XEN_PVH_ENTRY",
    "HERMIT_XEN_CPU_READY",
    "HERMIT_XEN_MEMORY_READY",
    "HERMIT_KERNEL_READY",
    "HERMIT_APP_ENTER",
    "HERMIT_APP_ALLOC_OK",
    "HERMIT_APP_THREAD_OK",
    "HERMIT_APP_TIMER_OK",
    "HERMIT_APP_CLOCK_OK",
    "HERMIT_SMOKE_PASS",
    "HERMIT_XEN_SHUTDOWN",
];

// Diagnostics never overwrite the test outcome, but tool failures remain visible.
fn diagnostic_command(run: &Path, name: &str, program: &str, args: &[&str]) {
    let result = (|| -> Result<()> {
        let output = fs::File::create(run.join(format!("{name}.txt")))?;
        let mut child = ManagedChild::spawn(
            Command::new(program)
                .args(args)
                .current_dir(run)
                .stdin(Stdio::null())
                .stdout(output.try_clone()?)
                .stderr(output),
        )?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.0.try_wait()? {
                ensure!(status.success(), "{program}: {status}");
                return Ok(());
            }
            ensure!(Instant::now() < deadline, "{program}: diagnostic timeout");
            thread::sleep(Duration::from_millis(20));
        }
    })();
    if let Err(error) = result {
        let _ = fs::write(run.join(format!("{name}.error")), format!("{error:#}"));
    }
}

fn hermit_diagnostics(run: &Path, summary: &mut Value, passed: bool) {
    let console = fs::read_to_string(run.join("guest-console.txt"))
        .or_else(|_| fs::read_to_string(run.join("dom0/xen-console/guest-pvh-smoke.log")))
        .unwrap_or_default();
    let markers: Vec<_> = HERMIT_MARKERS
        .iter()
        .filter(|m| console.lines().any(|l| l.trim() == **m))
        .copied()
        .collect();
    let stage = if passed {
        "complete"
    } else if console.contains("HERMIT_SMOKE_PASS") || console.contains("HERMIT_IPC_PASS") {
        "shutdown"
    } else if console.contains("HERMIT_IPC_CONNECTED") {
        "ipc-events-timers-or-messaging"
    } else if console.contains("XEN_IPC_OFFER") {
        "ipc-peer-establishment"
    } else if console.contains("HERMIT_KERNEL_READY") {
        "application"
    } else if console.contains("HERMIT_XEN_MEMORY_READY") {
        "kernel-initialization"
    } else if console.contains("HERMIT_XEN_CPU_READY") {
        "memory-setup"
    } else if console.contains("HERMIT_XEN_PVH_ENTRY") {
        "early-cpu-initialization"
    } else if run.join("cycle-1/xl-list-paused.json").exists() {
        "early-entry-before-console"
    } else {
        "loading-or-lab-boot"
    };
    summary["hermit"] = json!({"stage":stage,"observed_markers":markers});
    if let Ok(registers) = fs::read_to_string(run.join("guest-registers.txt"))
        && let Some(rip) = registers
            .lines()
            .find_map(|l| l.strip_prefix("rip: "))
            .and_then(|s| s.split_whitespace().next())
    {
        diagnostic_command(
            run,
            "guest-rip-symbol",
            "addr2line",
            &["-i", "-f", "-C", "-e", "kernel.elf", &format!("0x{rip}")],
        );
    }
}

fn ipc_start(
    options: &Options,
    run: &Path,
    summary: &mut Value,
    rpc: &mut Channel,
    child: &mut ManagedChild,
) -> Result<()> {
    stage(run, summary, "ipc-establishment")?;
    let deadline = Instant::now() + Duration::from_secs(options.timeout.min(15));
    let offer = loop {
        check_alive(child)?;
        let value = rpc.exec(&["cat", "/lab/xen-console/guest-pvh-smoke.log"], 5)?;
        let text = value["stdout"].as_str().unwrap_or_default();
        if let Some(line) = text.lines().find(|l| l.starts_with("XEN_IPC_OFFER ")) {
            break line.to_owned();
        }
        ensure!(
            !text.contains("panicked at"),
            "Hermit panicked before IPC offer: {text}"
        );
        ensure!(
            Instant::now() < deadline,
            "no IPC offer; guest output: {text}"
        );
        thread::sleep(Duration::from_millis(100));
    };
    let field = |name: &str| -> Result<String> {
        let value = offer
            .split_whitespace()
            .find_map(|f| f.strip_prefix(&format!("{name}=")))
            .context("invalid IPC offer")?;
        value.parse::<u64>()?;
        Ok(value.into())
    };
    let port = field("port")?;
    let grant = field("grant")?;
    let session = field("session")?;
    let domid = rpc.checked(&["xl", "domid", "pvh-smoke"], 5)?;
    let domid = domid["stdout"]
        .as_str()
        .context("missing domid")?
        .trim()
        .to_owned();
    let sample = |rpc: &mut Channel| -> Result<(f64, u64, String, Value)> {
        let v = rpc.checked(&["xl", "list", "pvh-smoke"], 5)?;
        let line = v["stdout"]
            .as_str()
            .unwrap_or_default()
            .lines()
            .find(|l| l.starts_with("pvh-smoke "))
            .context("guest missing")?;
        let f: Vec<_> = line.split_whitespace().collect();
        Ok((f[5].parse()?, f[2].parse()?, f[4].into(), v))
    };
    let (before, memory, _, before_raw) = sample(rpc)?;
    record(run, "idle-before", &before_raw)?;
    let start = Instant::now();
    let xentop = rpc.checked(&["xentop", "-b", "-i", "2", "-d", "2"], 6)?;
    record(run, "idle-xentop", &xentop)?;
    let row = xentop["stdout"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .rfind(|l| l.split_whitespace().next() == Some("pvh-smoke"))
        .context("xentop omitted IPC domain")?;
    let fields: Vec<_> = row.split_whitespace().collect();
    let cpu_percent: f64 = fields
        .get(3)
        .context("xentop missing CPU percent")?
        .parse()?;
    let resident_kib: u64 = fields.get(4).context("xentop missing memory")?.parse()?;
    let (after, _, state, after_raw) = sample(rpc)?;
    record(run, "idle-after", &after_raw)?;
    let seconds = start.elapsed().as_secs_f64();
    let upper = (after - before + 0.1).max(0.0) / seconds * 100.0;
    summary["idle"] = json!({"sample_seconds":seconds,"cpu_seconds_delta":after-before,"cpu_percent_upper_bound":upper,"xl_resolution_seconds":0.1,"state":state,"allocated_memory_mib":memory});
    summary["idle"]["xentop_cpu_percent"] = json!(cpu_percent);
    summary["idle"]["resident_kib"] = json!(resident_kib);
    assertion(summary, "xentop idle CPU under 1%", cpu_percent < 1.0)?;
    assertion(
        summary,
        "idle vCPU blocked",
        state.as_bytes().get(1) == Some(&b'b'),
    )?;
    assertion(
        summary,
        "idle CPU below 6% (xl quantization bound)",
        upper < 6.0,
    )?;
    record(
        run,
        "ipc-offer",
        &json!({"domid":domid,"port":port,"grant":grant,"session":session,"authorized_peer":0,"case":options.ipc_case}),
    )?;
    // The service manager owns the peer process, so RPC never leaves an untracked child.
    let response = rpc.checked(
        &[
            "systemd-run",
            "--unit=lab-ipc-peer",
            "--property=Type=exec",
            "--property=RemainAfterExit=yes",
            "--property=RuntimeMaxSec=30",
            "--property=TimeoutStopSec=2",
            "--property=StandardOutput=append:/lab/peer.log",
            "--property=StandardError=append:/lab/peer.log",
            "/inputs/ipc-peer",
            &domid,
            &port,
            &grant,
            &session,
            &options.ipc_case,
        ],
        5,
    )?;
    record(run, "peer-start", &response)
}
fn ipc_finish(
    options: &Options,
    run: &Path,
    summary: &mut Value,
    rpc: &mut Channel,
    console: &str,
) -> Result<()> {
    for marker in [
        "XEN_PVCLOCK_READY",
        "XEN_EVENTS_READY",
        "HERMIT_IPC_RUNNING_CLOCK",
        "HERMIT_KERNEL_READY",
        "HERMIT_IPC_CONNECTED",
        "HERMIT_IPC_TIMEOUT_OK",
        "HERMIT_IPC_REVOCATION_BUSY_OK",
        "XEN_IPC_RELEASED grants=0 maps=0",
        "HERMIT_IPC_PASS",
    ] {
        assertion(summary, marker, console.contains(marker))?;
        if options.ipc_case == "normal" {
            assertion(
                summary,
                "response backpressure exercised",
                console.contains("HERMIT_IPC_BACKPRESSURE_OK"),
            )?;
            assertion(
                summary,
                "83 typed requests processed",
                console.contains("HERMIT_IPC_STATS requests=83 "),
            )?;
            assertion(
                summary,
                "cancellation frame processed",
                console.contains("HERMIT_IPC_CANCEL_OK"),
            )?;
        }
    }
    let marker = match options.ipc_case.as_str() {
        "malformed" => "HERMIT_IPC_MALFORMED_OK",
        "peer-failure" => "HERMIT_IPC_PEER_FAILURE_OK",
        _ => "HERMIT_IPC_MASK_OK",
    };
    assertion(summary, marker, console.contains(marker))?;
    if options.ipc_case == "normal" {
        assertion(
            summary,
            "response backpressure exercised",
            console.contains("HERMIT_IPC_BACKPRESSURE_OK"),
        )?;
        assertion(
            summary,
            "83 typed requests processed",
            console.contains("HERMIT_IPC_STATS requests=83 "),
        )?;
        assertion(
            summary,
            "cancellation frame processed",
            console.contains("HERMIT_IPC_CANCEL_OK"),
        )?;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        let status = rpc.checked(
            &[
                "systemctl",
                "show",
                "lab-ipc-peer.service",
                "-p",
                "ExecMainStatus",
                "-p",
                "ExecMainCode",
                "-p",
                "SubState",
            ],
            5,
        )?;
        if !status["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("SubState=running")
        {
            break status;
        }
        ensure!(Instant::now() < deadline, "peer did not exit");
        thread::sleep(Duration::from_millis(50));
    };
    record(run, "peer-status", &status)?;
    let text = status["stdout"].as_str().unwrap_or_default();
    let expected = if options.ipc_case == "peer-failure" {
        42
    } else {
        0
    };
    assertion(
        summary,
        "peer expected exit status",
        text.lines()
            .any(|l| l == format!("ExecMainStatus={expected}"))
            && text.contains("ExecMainCode=1"),
    )?;
    let log = rpc.checked(&["cat", "/lab/peer.log"], 5)?;
    record(run, "peer-console", &log)?;
    let log = log["stdout"].as_str().unwrap_or_default();
    if expected == 0 {
        assertion(
            summary,
            "IPC_PEER_PASS",
            log.lines().any(|l| l == "IPC_PEER_PASS"),
        )?;
    }
    summary["peer_events"] = json!(
        log.lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .collect::<Vec<_>>()
    );
    rpc.checked(&["systemctl", "stop", "lab-ipc-peer.service"], 5)?;
    Ok(())
}
